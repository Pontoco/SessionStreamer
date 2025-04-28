use anyhow::{Result, anyhow};
use axum_test::TestServer;
use server::{DataMessage, ServerSignal};
use std::sync::Mutex;
use std::{fs::File, io::BufReader, path::PathBuf, sync::Arc, time::Duration};
use test_log::test;
use tokio::sync::{mpsc, watch};
use tokio::time::timeout;
use tracing::{info, info_span, instrument, Instrument, Level};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};
use webrtc::api::media_engine;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::media::io::h264_reader;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::{
    api::{
        APIBuilder, interceptor_registry::register_default_interceptors, media_engine::MediaEngine,
    },
    interceptor::registry::Registry,
    media::Sample,
    peer_connection::{
        configuration::RTCConfiguration, sdp::session_description::RTCSessionDescription,
    },
    rtp_transceiver::rtp_codec::RTCRtpCodecCapability,
    track::track_local::track_local_static_sample::TrackLocalStaticSample,
};

// This test creates a local webrtc client and streams an h264 file to the server as if it were a game
// session.
#[tokio::test]
async fn test_send_h264_stream() -> Result<()> {
    // let console_layer = console_subscriber::ConsoleLayer::builder().spawn();
    let fmt_layer = fmt::layer().with_target(true).with_level(true);

    let filter_layer = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter_layer)
        // .with(console_layer)
        .with(fmt_layer)
        .init();

    async {
        let app = TestServer::new(server::create_server()?)?;

        info!("created test server");

        // Default API setup.
        let mut m = MediaEngine::default();
        m.register_default_codecs()?;
        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut m)?;

        info!("building api");
        let api = APIBuilder::new()
            .with_media_engine(m)
            .with_interceptor_registry(registry)
            .build();

        info!("creating peer connection");

        // Setup the peer connection on the client side
        let peer = api.new_peer_connection(RTCConfiguration::default()).await?;

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

        info!("creating data channel.");
        // Creates a data channel track and adds it to the peer.
        let data_channel = peer.create_data_channel("general", None).await?;
        let (data_tx, mut data_rx) = mpsc::unbounded_channel();

        data_channel.on_message(Box::new(move |message| {
            let data_tx = data_tx.clone();
            Box::pin(async move {
                let string = String::from_utf8(message.data.to_vec()).unwrap();
                let data = serde_json::from_str(&string).unwrap();
                info!("Received message from server: {:?}", data);
                data_tx.send(data).unwrap();
            })
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
            .post("/whip?session_id=test_whip_success")
            .content_type("application/sdp")
            .bytes(latest_local_description.into())
            .await;

        offer_response.assert_status_success();

        peer.set_remote_description(RTCSessionDescription::answer(offer_response.text())?)
            .await?;

        // Wait for the connection to connect.
        // todo

        // Start streaming the data!
        let h264_data = BufReader::new(File::open("./h264-sample.h264")?);
        let mut h264reader = h264_reader::H264Reader::new(h264_data, 1024 * 1024);
        println!("Start streaming h264 file.");

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
            // Don't sleep. We want to be able to stream data really fast I think! Faster than realtime.
        }

        println!(
            "Finished streaming file. Sent {} samples (nal units).",
            sample_count
        );

        info!("Closing client peer connection");

        added_track.stop().await?;
        peer.remove_track(&added_track).await?;
        // peer.close().await?;

        println!("Waiting for server to send a SessionComplete message..");

        // Wait for the server to send a SessionComplete message.
        timeout(Duration::from_secs(30), async move {
            while let Some(msg) = data_rx.recv().await {
                if let DataMessage::ServerSignal(signal) = msg {
                    if signal == ServerSignal::SessionComplete {
                        return Ok(());
                    }
                }
            }

            anyhow::bail!("didn't find session message");
        })
        .await??;

        Ok(())
    }.instrument(info_span!("running_test")).await
}

/// Waits for the ICE connection to complete, meaning we are ready to be able to stream frames of video on the track.
fn wait_for_ice_connection(
    pc: Arc<RTCPeerConnection>,
) -> impl Future<Output = Result<()>> + Send + 'static {
    // Create a watch channel initialized with the current state
    let (tx, mut rx) = watch::channel(pc.ice_connection_state());

    // Setup the callback to send updates into the watch channel's sender
    pc.on_ice_connection_state_change(Box::new(move |state: RTCIceConnectionState| {
        // Send the new state. Ignore error (if receiver dropped, future was dropped)
        let _ = tx.send(state);
        // Callback needs to return a pinned future
        Box::pin(async {})
    }));

    // Return a future that waits on the receiver
    async move {
        loop {
            // Check the current state known by the receiver
            match *rx.borrow() {
                RTCIceConnectionState::Connected | RTCIceConnectionState::Completed => {
                    return Ok(());
                }
                RTCIceConnectionState::Failed
                | RTCIceConnectionState::Disconnected
                | RTCIceConnectionState::Closed => {
                    return Err(anyhow!(
                        "ICE connection reached terminal state: {:?}",
                        *rx.borrow()
                    ));
                }
                _ => {} // Other states (New, Checking) - wait for change
            }

            // Wait efficiently for the state to change
            if rx.changed().await.is_err() {
                // Error means sender (callback closure) was dropped, likely because
                // the PeerConnection was closed.
                return Err(anyhow!(
                    "PeerConnection closed while waiting for ICE connection"
                ));
            }
        }
    }
}
