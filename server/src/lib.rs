mod webrtc_utils;

use anyhow::{Context, Result, anyhow, ensure};
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum_extra::TypedHeader;
use axum_extra::headers::ContentType;
use gstreamer::{prelude::*, ClockTime, MessageType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicI8;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, mpsc};
use tokio::fs;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, Notify};
use tokio_stream::StreamExt;
use tracing::{error, info};
use tracing::{info_span, instrument, trace, warn};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, interceptor_registry, media_engine};
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::track::track_remote::TrackRemote;
use webrtc_utils::StatefulPeerConnection;

#[derive(Clone)]
struct AppState {
    pub rtc: Arc<webrtc::api::API>,

    // The path on disk to store streamed data from game sessions.
    pub data_path: PathBuf,
}

#[derive(Clone)]
struct SessionState {
    pub app_state: AppState, // Global state associated with the runtime.
    pub session_id: String,  // GUID identifier for this session.
    pub data_path: PathBuf,  // The path where data for this session is stored.
    pub client_send_tx: Sender<ServerMessage>, // Sends messages to the client via our data channel.
    pub connection_closed: Arc<Notify>, // Sent when the peer connection closes.
    pub track_id: Arc<AtomicI8>, // Incrementing track id for each connected video track.
    pub client_send_rx: Arc<Mutex<Option<Receiver<ServerMessage>>>>,
}

/// Sent from the server to control or inform the client.
/// We can use this to control the client, to inform it of information, or to receive commands.
#[derive(Serialize, Deserialize, Debug)]
pub enum ServerMessage {
    Info(String),
    Error(String),
    SessionComplete, // Sent to the client when the streaming is complete and all files are fully saved to disk.
}

/// Sent from the client to inform the server. Usually these are best effort, since the client can
/// disappear at any time.
#[derive(PartialEq, Serialize, Deserialize, Debug)]
pub enum ClientMessage {
    SessionEnding, // Sent when the client is done sending video for the session.
}

impl<E> From<E> for ServerMessage
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        ServerMessage::Error(format!("{}", value.into()))
    }
}

/// Server-side state that lives while a specific session is being recorded.
struct Session {
    pub client_data_send: mpsc::Sender<ServerMessage>,
}

pub fn create_server(data_path: impl Into<PathBuf>) -> Result<axum::Router> {
    // Setup the default codec support for things like h264.
    let mut m = MediaEngine::default();
    m.register_default_codecs()?;

    // Use the default interceptor registry. (from webrtc samples)
    let mut registry = Registry::new();
    registry = interceptor_registry::register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new()
        .with_media_engine(m)
        .with_interceptor_registry(registry)
        .build();

    // Setup the axum server.
    let state = AppState {
        rtc: Arc::new(api),
        data_path: data_path.into(),
    };

    Ok(axum::Router::new()
        .route("/whip", post(whip_post_handler))
        .with_state(state))
}

#[derive(Deserialize)]
struct QueryParams {
    pub session_id: String,
}

#[instrument("server", skip_all)]
#[axum::debug_handler]
async fn whip_post_handler(
    State(state): State<AppState>,
    TypedHeader(content_type): TypedHeader<ContentType>,
    Query(query): Query<QueryParams>,
    body: String,
) -> Result<impl IntoResponse, AppError> {
    ensure_app(
        content_type == "application/sdp".parse()?,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Content-Type must be application/sdp.",
    )?;

    let (client_send, client_send_rx) = tokio::sync::mpsc::channel::<ServerMessage>(100);

    let connection_closed = Arc::new(Notify::new());

    // todo: Verify this session_id hasn't already been created. Some kind of disk file to represent it?
    let data_path = state.data_path.join(&query.session_id);
    if fs::try_exists(&data_path).await? {
        return Err(AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow!("Session ID already exists! [{}] path: [{:?}]", &query.session_id, &data_path),
        ))
    }
    fs::create_dir(&data_path).await?;

    let session_state = SessionState {
        client_send_tx: client_send.clone(),
        client_send_rx: Arc::new(Mutex::new(Some(client_send_rx))),
        app_state: state.clone(),
        connection_closed: connection_closed.clone(),
        track_id: Arc::new(AtomicI8::new(0)),
        data_path: data_path,
        session_id: query.session_id,
    };

    let config = RTCConfiguration {
        ..Default::default()
    };

    let peer = state
        .rtc
        .new_peer_connection(config)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.into()))?;

    let peer = StatefulPeerConnection::new(peer, session_state.clone());

    // Send anything to this channel you want sent to the client via our data channel.
    session_state
        .client_send_tx
        .send(ServerMessage::Info("Data channel connected!".into()))
        .await?;

    peer.on_peer_connection_state_change(async |state, conn_state| {
        info!("Server peer connection state changed: {:?}", conn_state);
        if conn_state == RTCPeerConnectionState::Closed
            || conn_state == RTCPeerConnectionState::Disconnected
        {
            state.connection_closed.notify_waiters();
        }
    });

    // Wait for the 'general' data channel to connect and receive messages on the other end of the above channel.
    peer.on_data_channel(async |state, channel| {
        let span = info_span!("on_data_channel");
        let _ = span.enter();

        info!("Data channel connected: {}", &channel.label());

        if channel.label() != "general" {
            error!("Unexpected data channel [{}]", channel.label());
            return;
        }

        if let Some(mut rx) = state.client_send_rx.lock().await.take() {
            // Make sure to log any errors with the data channel.
            channel.on_error(async |_, error| {
                error!("Data channel had error: {}", error);
            });

            channel.on_open(async move |_, channel| {
                // Our primary 'send stuff to the client' channel.
                while let Some(data) = rx.recv().await {
                    match serde_json::to_string(&data) {
                        Ok(json) => match channel.send_text(json).await {
                            Ok(_) => info!("Sent message to client [{:?}]", &data),
                            Err(err) => error!("Error sending message to client [{}]", err),
                        },
                        Err(err) => {
                            error!("Failed to serialize data message. {} {:?}", err, data)
                        }
                    }
                }
            });

            channel.on_message(async |state, msg| {
                match serde_json::from_slice::<ClientMessage>(&msg.data) {
                    Err(e) => error!("Client did not send valid ClientMessage JSON. [{}]", e),
                    Ok(msg) => match msg {
                        ClientMessage::SessionEnding => {
                            state.connection_closed.notify_waiters();
                        }
                    },
                };
            });
        } else {
            error!("Data channel already initialized [{}]", channel.label());
        }
    });

    peer.on_track(async |mut state, track, _, _| {
        let result = handle_track(
            state.track_id.fetch_add(1, SeqCst),
            track,
            "sessionid",
            &mut state,
        );

        if let Err(err) = result.await {
            error!("{}", err);
            let _ = state
                .client_send_tx
                .send(ServerMessage::from(err))
                .await
                .map_err(|err| error!("{}", err));
        }
    });

    let offer = RTCSessionDescription::offer(body)?;
    peer.set_remote_description(offer).await?;

    let answer = peer.create_answer(None).await?;
    peer.set_local_description(answer).await?;

    peer.gathering_complete_promise().await.recv().await;
    info!("Local ICE gathering complete.");

    let local_description = peer.local_description().await.ok_or_else(|| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::anyhow!("Local description did not get set properly."),
        )
    })?;

    info!("WHIP session created.");

    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/sdp")
        .body(local_description.sdp)?)
}

/// Handles video tracks that the client sends to us.
async fn handle_track<'a>(
    track_id: i8,
    track: Arc<TrackRemote>,
    session_id: &'a str,
    session_state: &mut SessionState,
) -> anyhow::Result<()> {
    info!(session_id = %session_id, track_id = %track.id(), kind = ?track.kind(), "Track connected.");

    let codec = track.codec();
    let mime_type = codec.capability.mime_type.to_lowercase();
    info!(%mime_type, clock_rate = codec.capability.clock_rate, "Track codec details.");

    ensure!(
        mime_type
            .as_str()
            .eq_ignore_ascii_case(media_engine::MIME_TYPE_H264),
        "Video tracks must be in H264 format. [{}]",
        mime_type
    );

    // Initialize GStreamer
    gstreamer::log::remove_default_log_function(); // Removes printing to stdout.
    gstreamer::log::set_default_threshold(gstreamer::DebugLevel::Trace); // Sets default filtering on the gstreamer side.
    tracing_gstreamer::integrate_events(); // Merge gstreamer events into tracing.

    gstreamer::init()?;

    info!("Initializing GStreamer pipeline for track {track_id}.");

    // Build the GStreamer pipeline
    //  - Feed in raw NALU packets in H264 format.
    //  - Output to MP4
    let output_file = session_state
        .data_path
        .join(format!("game_capture_{track_id}.mp4"))
        .into_os_string()
        .into_string()
        .map_err(|e| {
            anyhow!("Data path [{e:?}] provided for session streamer was not valid UTF8 string.")
        })?;

    let pipeline_str = format!(
        "appsrc name=appsrc ! h264parse ! mp4mux name=mux ! filesink location={output_file}",
    );

    info!("Starting pipeline [{}]", &pipeline_str);

    let pipeline = gstreamer::parse::launch(&pipeline_str)
        .unwrap()
        .downcast::<gstreamer::Pipeline>()
        .unwrap();

    // Get the appsrc element
    let appsrc = pipeline
        .by_name("appsrc")
        .unwrap()
        .downcast::<gstreamer_app::AppSrc>()
        .unwrap();

    // Configure the appsrc
    appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);

    let rtp_caps = gstreamer::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .build();

    appsrc.set_caps(Some(&rtp_caps));
    appsrc.set_format(gstreamer::Format::Time); // Says that our buffers will have timestamps.
    appsrc.set_is_live(true); // A flag telling downstream pads that we are optimizing for live streaming content.

    // todo: This uses the live timestamp of this process to timestamp the buffers.
    // We definitely want to manually timestamp each sent buffer from the RTP stream.
    // Or use the clock rate instead.
    appsrc.set_do_timestamp(true);

    // Start the pipeline
    pipeline.set_state(gstreamer::State::Playing)?;

    info!("Started gstreamer pipeline: {:?}", pipeline);

    // In handle_track, after starting the pipeline:
    // let bus = pipeline.bus().expect("Failed to get pipeline bus");
    // let pipeline_weak = pipeline.downgrade(); // To avoid owning pipeline in the task

    // tokio::spawn(async move {
    //     let mut bus_stream = bus.stream_filtered();
    //     while let Some(msg) = bus_stream.next().await {
    //         if pipeline_weak.upgrade().is_none() {
    //             info!("Pipeline for bus monitoring no longer exists, exiting bus watch.");
    //             break;
    //         }
    //         match msg.view() {
    //             gstreamer::MessageView::Error(err) => {
    //                 error!(
    //                     "GStreamer Error from {}: {} ({:?})",
    //                     err.src()
    //                         .map_or_else(|| "None".to_string(), |s| s.path_string().to_string()),
    //                     err.error(),
    //                     err.debug()
    //                 );
    //             }
    //             gstreamer::MessageView::Warning(warning) => {
    //                 warn!(
    //                     "GStreamer Warning from {}: {} ({:?})",
    //                     warning
    //                         .src()
    //                         .map_or_else(|| "None".to_string(), |s| s.path_string().to_string()),
    //                     warning.error(),
    //                     warning.debug()
    //                 );
    //             }
    //             gstreamer::MessageView::Eos(eos_details) => {
    //                 info!(
    //                     "GStreamer EOS received on bus task: {:?}",
    //                     eos_details.src().map(|s| s.path_string())
    //                 );
    //                 // This task can notify the main task that EOS was seen on the bus.
    //             }
    //             // Ignore other messages.
    //             _ => {}
    //         }
    //     }
    //     info!("GStreamer bus monitoring task finished.");
    // });

    session_state
        .client_send_tx
        .send(ServerMessage::Info(
            "Video stream connected. Streaming begin.".into(),
        ))
        .await?;

    // Debugs the NAL units we've received.
    // let mut reader = AnnexBReader::accumulate(|nal: RefNal<'_>| {
    //     let nal_unit_type = nal.header().unwrap().nal_unit_type();
    //     // info!("H264: Saw start of {:?}", nal_unit_type);
    //     if nal.is_complete() {
    //         let mut data = vec![];
    //         nal.reader().read_to_end(&mut data).unwrap();
    //         // info!(
    //         //     "Server: Has NAL Unit: {:?} of size: {}",
    //         //     nal_unit_type,
    //         //     data.len()
    //         // );
    //     }
    //     NalInterest::Buffer
    // });

    let mut depacketizer = H264Packet::default();
    let mut packets = 0;

    loop {
        tokio::select! {
            data = track.read_rtp() => {
                match data {
                    Ok((packet, _)) => {
                        trace!("Got rtp packet with sequence number {}, timestamp {}, and payload length {}", packet.header.sequence_number, packet.header.timestamp, packet.payload.len());

                        let h264_bytes = depacketizer.depacketize(&packet.payload)?;
                        packets += 1;

                        if h264_bytes.len() > 0 {

                            let mut buffer = gstreamer::Buffer::from_slice(h264_bytes);

                            // Our RTP stream timestamps are given in a specific clock rate, passed in above.
                            // That rate is in Hz, ie 90000Hz, so we convert to nanoseconds.
                            let timestamp_nanos =  ClockTime::from_nseconds((packet.header.timestamp as u64 * ClockTime::SECOND.nseconds()) / codec.capability.clock_rate as u64);

                            buffer.get_mut().unwrap().set_pts(Some(timestamp_nanos));
                            buffer.get_mut().unwrap().set_dts(Some(timestamp_nanos));
                            appsrc.push_buffer(buffer).expect("Failed to push buffer");
                        }
                    }
                    Err(err) => {
                        error!("{:?}", err);
                        break;
                    }
                }
            }
            _ = session_state.connection_closed.notified() => {
                break;
            }
        }
    }

    info!("Finished receiving RTP stream. Received {packets} packets.");

    session_state
        .client_send_tx
        .send(ServerMessage::Info("Video stream closed.".into()))
        .await?;

    // Say we're done pushing data.
    info!("Sending EOS to GStreamer pipeline.");
    appsrc.end_of_stream()?;

    // todo: Technically we should wait for the pipeline to finish!
    // However, I haven't been able to get this to trigger. The filesink pad doesn't seem to want to close and emit the final EOS on the bus.
    info!("Waiting for bus to finish..");
    let bus = pipeline.bus().expect("Failed to get pipeline bus");
    bus.timed_pop_filtered(None, &[MessageType::Eos, MessageType::Error]);

    // Cleanup resources.
    pipeline.set_state(gstreamer::State::Null).unwrap();

    // For now, the session closes when the video is finished.
    session_state
        .client_send_tx
        .send(ServerMessage::SessionComplete)
        .await?;

    Ok(())
}

// Error handling
// -----

struct AppError(StatusCode, anyhow::Error);

impl<T> From<T> for AppError
where
    T: std::error::Error + Send + Sync + 'static,
{
    fn from(err: T) -> Self {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::Error::msg(err.to_string()),
        )
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, self.1.to_string()).into_response()
    }
}

fn ensure_app(condition: bool, status_code: StatusCode, msg: &'static str) -> Result<(), AppError> {
    if !condition {
        return Err(AppError(status_code, anyhow::anyhow!(msg)));
    }
    Ok(())
}

pub fn default_tracing_registry() {
    // Command line logging settings:
    // By default, set gstreamer to warn because it's quite noisy.
    let cli_log_settings =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,gstreamer=warn"));

    tracing_subscriber::registry()
        .with(cli_log_settings)
        .with(
            tracing_subscriber::fmt::layer()
                .with_file(true)
                .with_line_number(true),
        )
        .init();
}
