use anyhow::{Context, Result};
use axum_test::TestServer;
use server::{ClientMessage, ServerMessage};
use std::{fs::File, io::BufReader, sync::Arc, time::Duration};
use tempfile::TempDir;
use tokio::fs;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout};
use tracing::{Instrument, Span, info, info_span, instrument};
use webrtc::api::media_engine;
use webrtc::data_channel::RTCDataChannel;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::media::io::h264_reader;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::{
    api::{APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine},
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription},
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

struct TestPeerSetup(
    RTCPeerConnection,
    Arc<TrackLocalStaticSample>,
    Arc<RTCDataChannel>,
    UnboundedReceiver<ServerMessage>,
);

async fn connect_peer(app: &TestServer, session_id: &str) -> Result<TestPeerSetup> {
    // Default API setup.
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    info!("building api");
    let api = APIBuilder::new().with_media_engine(m).with_interceptor_registry(registry).build();

    info!("creating peer connection");

    // Setup the peer connection on the client side
    let peer = api
        .new_peer_connection(RTCConfiguration::default())
        .instrument(info_span!("client_webrtc"))
        .await?;

    info!("creating video track.");

    // Creates a video track and adds it to the peer.
    let track = Arc::new(TrackLocalStaticSample::new(
        RTCRtpCodecCapability {
            mime_type: media_engine::MIME_TYPE_H264.to_owned(),
            ..Default::default()
        },
        "video".to_string(),
        "webrtc-test".to_string(),
    ));
    let added_track = peer.add_track(track.clone()).await?;

    // Creates a data channel track and adds it to the peer.
    info!("creating data channel.");
    let data_channel = peer.create_data_channel("general", None).await?;
    let (data_tx, data_rx) = mpsc::unbounded_channel();

    let span = Span::current().clone();
    data_channel.on_message(Box::new(move |message| {
        let data_tx = data_tx.clone();
        let span = span.clone();
        Box::pin(
            async move {
                let string = String::from_utf8(message.data.to_vec()).unwrap();
                let data: ServerMessage = serde_json::from_str(&string).unwrap();
                info!("Received message from server: {:?}", data);
                data_tx.send(data).unwrap();
            }
            .instrument(span),
        )
    }));

    info!("creating offer.");
    // Start creating offer...
    let sdp_offer = peer.create_offer(None).await?;

    info!("set locl descripion");
    peer.set_local_description(sdp_offer).await?;

    info!("gathering...");
    // Wait for ICE gathering to finish. Wait for channel to close.
    peer.gathering_complete_promise().await.recv().await;

    info!("gather complete");
    let latest_local_description = peer.local_description().await.unwrap().sdp;

    info!("issueing whip");

    let offer_response = app
        .post(&format!("/whip?session_id={session_id}"))
        .content_type("application/sdp")
        .bytes(latest_local_description.into())
        .await;

    offer_response.assert_status_success();

    peer.set_remote_description(RTCSessionDescription::answer(offer_response.text())?)
        .await?;

    // Wait for the connection to connect.
    // todo
    while peer.connection_state() != RTCPeerConnectionState::Connected {
        sleep(Duration::from_micros(50)).await
    }

    Ok(TestPeerSetup(peer, track, data_channel, data_rx))
}

// This test creates a local webrtc client and streams an h264 file to the server as if it were a game
// session.
#[tokio::test]
#[instrument(skip_all)]
async fn test_send_h264_stream() -> Result<()> {
    server::default_process_setup(false);

    let temp_output_dir = TempDir::with_prefix("session_streamer_data")?;
    let temp_output_path = temp_output_dir.path();
    let session_id = "GameSession_0001_Test";
    let app = TestServer::new(server::create_server(temp_output_path)?)?;

    info!("Created test server storing data at [{temp_output_path:?}].");

    let TestPeerSetup(peer, track, data_channel, mut server_messages) = connect_peer(&app, session_id).await?;

    // Connect a new data channel and send some test text.
    let log_data_channel = peer.create_data_channel("test_logs", None).await?;

    info!("Waiting for logs data channel to open.");
    while log_data_channel.ready_state() != RTCDataChannelState::Open {
        sleep(Duration::from_micros(50)).await;
    }

    info!("Sending test log data.");
    log_data_channel.send_text("Test line 1 with a bunch of data.").await?;
    log_data_channel.send_text("Continuation of line 1 with a bunch of data.\n").await?;
    log_data_channel.send_text("Test line 2 with a more data").await?;

    // Start streaming the data!
    let h264_data = BufReader::new(File::open("./h264-sample.h264")?);
    let mut h264reader = h264_reader::H264Reader::new(h264_data, 1024 * 1024);
    let mut sample_count = 0;
    while let Ok(nal) = h264reader.next_nal() {
        track
            .write_sample(&Sample {
                data: nal.data.freeze(),
                duration: Duration::from_millis(1000 / 30),
                ..Default::default()
            })
            .await?;
        sample_count += 1;
        if sample_count > 200 {
            break;
        }
        // Don't sleep. We want to be able to stream data really fast I think! Faster than realtime.
    }

    info!("Finished streaming file. Sent {} samples (nal units).", sample_count);

    info!("Sending SessingEnding message to server.");
    data_channel
        .send_text(serde_json::to_string(&ClientMessage::SessionEnding)?)
        .await?;

    info!("Waiting for server to send a SessionComplete message..");

    // Wait for the server to send a SessionComplete message.
    timeout(Duration::from_secs(5), async move {
        while let Some(msg) = server_messages.recv().await {
            if let ServerMessage::SessionComplete = msg {
                return Ok(());
            }
        }

        anyhow::bail!("didn't find session message");
    })
    .await??;

    info!("Closing down peer connection.");
    peer.close().await?;
    drop(peer);

    // Check that we wrote the video out to the correct spot.
    let video_path = temp_output_path.join(session_id).join("game_capture_0.mp4");
    let video = fs::File::open(&video_path)
        .await
        .context(format!("Can't find path: {video_path:?}"))?;
    assert_eq!(video.metadata().await?.len(), 498807); // This is the size we see observed. Regression test.

    Ok(())
}
