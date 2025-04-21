use anyhow::{anyhow, Result};
use axum_test::TestServer;
use server::DataMessage;
use tokio::sync::watch;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use std::sync::Mutex;
use std::{fs::File, io::BufReader, path::PathBuf, sync::Arc, time::Duration};
use test_log::test;
use webrtc::api::media_engine;
use webrtc::media::io::h264_reader;
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
#[test(tokio::test)]
async fn test_send_h264_stream() -> Result<()> {
    let app = TestServer::new(server::create_server()?)?;

    // Default API setup.
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Setup the peer connection on the client side
    let peer = api.new_peer_connection(RTCConfiguration::default()).await?;

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
    let data_channel = peer.create_data_channel("general", None).await?;
    let data_logs : Arc<Mutex<Vec<DataMessage>>> = Arc::new(Mutex::new(vec![]));

    let d = data_logs.clone();
    data_channel.on_message(Box::new(move |message| {
        let d = d.clone();
        Box::pin(async move {
            let string = String::from_utf8(message.data.to_vec()).unwrap();
            let data = serde_json::from_str(&string).unwrap();
            d.lock().unwrap().push(data);
        })
    }));

    // Start creating offer...
    let sdp_offer = peer.create_offer(None).await?;
    peer.set_local_description(sdp_offer).await?;

    // Wait for ICE gathering to finish. Wait for channel to close.
    peer.gathering_complete_promise().await.recv().await;

    let latest_local_description = peer.local_description().await.unwrap().sdp;

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

    added_track.stop().await?;

    // Wait for the server to receive and process the stream. Hm.

    peer.close().await?;

    assert!(data_logs.lock().unwrap().len() > 0);

    for log in data_logs.lock().unwrap().iter() {
        match log {
            DataMessage::Error(err) => {
                panic!("Received error from server: {}", err);
            }
            DataMessage::Info(info) => {
                println!("Received info from server: {}", info);
            }
        }
    }

    Ok(())
}

/// Waits for the ICE connection to complete, meaning we are ready to be able to stream frames of video on the track.
fn wait_for_ice_connection(pc: Arc<RTCPeerConnection>)
    -> impl Future<Output = Result<()>> + Send + 'static
{
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
                RTCIceConnectionState::Failed |
                RTCIceConnectionState::Disconnected |
                RTCIceConnectionState::Closed => {
                    return Err(anyhow!("ICE connection reached terminal state: {:?}", *rx.borrow()));
                }
                _ => {} // Other states (New, Checking) - wait for change
            }

            // Wait efficiently for the state to change
            if rx.changed().await.is_err() {
                // Error means sender (callback closure) was dropped, likely because
                // the PeerConnection was closed.
                return Err(anyhow!("PeerConnection closed while waiting for ICE connection"));
            }
        }
    }
}