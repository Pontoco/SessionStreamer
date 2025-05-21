mod webrtc_utils;
mod rest;
mod timestamped_bytes;
mod data_channels;

use anyhow::{Result, anyhow};
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum_extra::TypedHeader;
use axum_extra::headers::ContentType;
use futures::{StreamExt, TryFutureExt};
use gstreamer::{ClockTime, MessageType, prelude::*};
use serde::{Deserialize, Serialize};
use tower_http::services::ServeDir;
use std::collections::{HashMap, VecDeque};
use std::panic::Location;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI8;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::sleep;
use tokio_stream::wrappers::WatchStream;
use tokio_util::sync::CancellationToken;
use tower_http::trace;
use tracing::{Instrument, Level, Span, debug, error, info};
use tracing::{trace, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, interceptor_registry, media_engine};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::rtp_transceiver::RTCPFeedback;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType};
use webrtc_utils::{StatefulDataChannel, StatefulPeerConnection, StatefulTrack};

#[derive(Clone)]
struct AppState {
    pub rtc: Arc<webrtc::api::API>,

    // The path on disk to store streamed data from game sessions.
    pub data_path: PathBuf,
}

#[derive(Clone)]
struct SessionState {
    pub app_state: AppState,                   // Global state associated with the runtime.
    pub session_id: String,                    // GUID identifier for this session.
    pub data_path: PathBuf,                    // The path where data for this session is stored.
    pub client_send_tx: Sender<ServerMessage>, // Sends messages to the client via our data channel.
    pub track_id: Arc<AtomicI8>,               // Incrementing track id for each connected video track.
    pub client_send_rx: Arc<Mutex<Option<Receiver<ServerMessage>>>>,
}

/// Sent from the server to control or inform the client.
/// We can use this to control the client, to inform it of information, or to receive commands.
/// We have to avoid using complex enums here since C# can't easily parse those.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum ServerMessage {
    Info { message: String },
    Error { message: String },
    SessionComplete, // Sent to the client after it requests a graceful shutdown of the session.
}

/// Sent from the client to inform the server. Usually these are best effort, since the client can
/// disappear at any time.
#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "kind")]
pub enum ClientMessage {
    Info { message: String },
    Error { message: String },
    SessionEnding, // Sent when the client is done sending video for the session.
}

impl<E> From<E> for ServerMessage
where
    E: Into<LineTrackingError>,
{
    fn from(value: E) -> Self {
        ServerMessage::Error {
            message: format!("{}", value.into()),
        }
    }
}

pub fn create_server(data_path: impl Into<PathBuf>) -> Result<axum::Router> {
    let mut m = MediaEngine::default();

    // We use a default media engine, but we only support H264 codecs.
    for codec in supported_codecs() {
        debug!("Registering codec: {:?}", codec);
        m.register_codec(codec, RTPCodecType::Video)?;
    }

    // Use the default interceptor registry. (from webrtc samples)
    let mut registry = Registry::new();
    registry = interceptor_registry::register_default_interceptors(registry, &mut m)?;

    let api = APIBuilder::new().with_media_engine(m).with_interceptor_registry(registry).build();

    // Setup the axum server.
    let data_path = data_path.into();
    let state = AppState {
        rtc: Arc::new(api),
        data_path: data_path.clone(),
    };

    Ok(axum::Router::new()
        .nest_service("/data", ServeDir::new(data_path))
        .route("/whip", post(whip_post_handler))
        .nest("/rest", rest::routes())
        .layer(
            trace::TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO)),
        )
        .with_state(state))
}

impl SessionState {
    /// Sends a message to the client.
    /// If the send fails, logs an error.
    pub async fn send_client_or_log_error<M: Into<String>>(&self, msg: M) {
        let msg = msg.into();
        if let Err(err) = self
            .client_send_tx
            .send(ServerMessage::Error { message: msg.clone() })
            .await
        {
            error!("Failed to send message to client: [{}] [{}]", msg, err);
        }
    }
}

#[axum::debug_handler]
async fn whip_post_handler(
    State(state): State<AppState>,
    TypedHeader(content_type): TypedHeader<ContentType>,
    Query(query_params): Query<HashMap<String, String>>,
    body: String,
) -> Result<impl IntoResponse, AppError> {
    debug!("Received WHIP request.");

    ensure_app(
        content_type == "application/sdp".parse()?,
        StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "Content-Type must be application/sdp.",
    )?;

    ensure_app(
        query_params.contains_key("session_id"),
        StatusCode::BAD_REQUEST,
        "Query parameter missing: [session_id]",
    )?;

    let (client_send, client_send_rx) = tokio::sync::mpsc::channel::<ServerMessage>(100);

    // todo: Verify this session_id hasn't already been created. Some kind of disk file to represent it?
    let session_id = query_params["session_id"].clone();
    let data_path = state.data_path.join(&session_id);
    if fs::try_exists(&data_path).await? {
        return Err(AppError::new(
            StatusCode::CONFLICT,
            anyhow!("Session ID already exists! [{}] path: [{:?}]", &session_id, &data_path),
        ));
    }
    fs::create_dir(&data_path).await?;

    info!("Created new game streaming session: {}", &session_id);

    // todo! It's important that this gets cancelled on drop.
    // Right now we have a memory leak if we hit an error, because the spawned tasks will not be killed.
    let graceful_shutdown_src = CancellationToken::new();

    let session_state = SessionState {
        client_send_tx: client_send.clone(),
        client_send_rx: Arc::new(Mutex::new(Some(client_send_rx))),
        app_state: state.clone(),
        track_id: Arc::new(AtomicI8::new(0)),
        data_path: data_path,
        session_id: session_id.clone(),
    };

    // Dump the metadata in the data path.
    fs::write(session_state.data_path.join("metadata.json"), serde_json::to_string(&query_params)?).await?;

    // Boot up a new WebRTC peer connection to attach to the client.
    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec![
                "stun:stun.l.google.com:19302".to_owned(),
                "stun:stun1.l.google.com:19302".to_owned(),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };

    debug!("Using STUN servers: {:?}", &config.ice_servers);

    let peer = StatefulPeerConnection::new(
        state
            .rtc
            .new_peer_connection(config)
            .await
            .map_err(|e| AppError::new(StatusCode::INTERNAL_SERVER_ERROR, e.into()))?,
    );

    debug!("Peer Connection created.");

    // Buffer an initial message that will be sent once our general data channel is connected.
    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Connected to general data channel!".into(),
        })
        .await?;

    // Echo state changes
    let mut state_stream = WatchStream::new(peer.state.clone());
    tokio::spawn(
        async move {
            while let Some(conn_state) = state_stream.next().await {
                info!("Server peer connection state changed: {:?}", conn_state);
            }
        }
        .instrument(Span::current()),
    );

    let (mut peer, mut data_channels, mut tracks) = peer.get_channels();

    // Branch off a new task for each data channel.
    let state = session_state.clone();
    let graceful_shutdown = graceful_shutdown_src.clone();
    let final_channels = tokio::spawn(
        async move {
            let mut channels_tasks = vec![];
            while let Some(channel) = graceful_shutdown.run_until_cancelled(data_channels.next()).await.flatten() {
                let state = state.clone();
                let request_graceful_shutdown = graceful_shutdown.clone();
                let task = tokio::spawn(
                    async move {
                        let result = data_channels::handle_data_channel(state.clone(), channel, request_graceful_shutdown);

                        if let Err(err) = result.await {
                            error!("{}", err);
                            let _ = state
                                .client_send_tx
                                .send(ServerMessage::from(err))
                                .await
                                .map_err(|err| error!("{}", err));
                        }
                    }
                    .instrument(Span::current()),
                );

                channels_tasks.push(task);
            }

            channels_tasks
        }
        .instrument(Span::current()),
    );

    // Branch off a new task for each video/audio track.
    let state = session_state.clone();
    let graceful_shutdown = graceful_shutdown_src.clone();
    let final_tracks = tokio::spawn(
        async move {
            let mut track_tasks = vec![];
            while let Some(track) = graceful_shutdown.run_until_cancelled(tracks.next()).await.flatten() {
                let state = state.clone();
                let graceful_shutdown = graceful_shutdown.clone();
                let task = tokio::spawn(
                    async move {
                        let result = handle_track(state.track_id.fetch_add(1, SeqCst), track, state.clone(), graceful_shutdown.clone());

                        if let Err(err) = result.await {
                            error!("{}", err);
                            let _ = state
                                .client_send_tx
                                .send(ServerMessage::from(err))
                                .await
                                .map_err(|err| error!("{}", err));
                        }
                    }
                    .instrument(Span::current()),
                );
                track_tasks.push(task);
            }

            track_tasks
        }
        .instrument(Span::current()),
    );

    debug!("Setting remote and local descriptions.");

    trace!("Received offer: {}", &body);
    let offer = RTCSessionDescription::offer(body)?;
    peer.set_remote_description(offer).await?;

    let answer = peer.create_answer(None).await?;
    peer.set_local_description(answer).await?;

    peer.on_gathering_complete().await.recv().await;

    let local_description = peer.local_description().await.ok_or_else(|| {
        AppError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::anyhow!("Local description did not get set properly."),
        )
    })?;

    info!("ICE gathering complete. Waiting to connect...");

    // Close the connection manually if the client requests it.
    // This is necessary because webrtc-rs does not properly handle the close_notify alert on the underlying data transport.
    // https://github.com/webrtc-rs/webrtc/issues/672
    let _ = tokio::spawn(
        async move {
            // Wait for either:
            //   - graceful shutdown request
            //   - connection state closed
            loop {
                tokio::select! {
                    _ = graceful_shutdown_src.cancelled() => {
                        info!("Starting graceful shutdown of session: {}", session_state.session_id);
                        break;
                    },

                    _ = peer.state.changed() => {
                        let state = *peer.state.borrow();
                        if matches!(state,
                            RTCPeerConnectionState::Closed
                            | RTCPeerConnectionState::Disconnected
                            | RTCPeerConnectionState::Failed) {
                            info!("Connection interrupted. Starting shutdown of session: {}", session_state.session_id);
                            break;
                        }
                    }
                }
            }

            // Start graceful shutdown if we haven't already.
            graceful_shutdown_src.cancel();

            // Wait for all data channel tasks to cleanup and finish.
            // Wait for all track tasks to cleanup and finish.
            info!("Waiting for channel spawner to finish.");
            let channels = final_channels.await?;

            info!("Waiting for [{}] channels to finish.", channels.len());
            for handle in channels {
                handle.await?;
            }

            info!("Waiting for tracks to close.");
            let tracks = final_tracks.await?;

            info!("Waiting for [{}] tracks to finish.", tracks.len());
            for handle in tracks {
                handle.await?;
            }

            // Send the final OK, if it was graceful!
            // Session Done.

            // Send a graceful ack if we can.
            if graceful_shutdown_src.is_cancelled() {
                info!("Sending SessionComplete message.");
                if let Err(err) = session_state.client_send_tx.send(ServerMessage::SessionComplete).await {
                    error!("Failed to send SessionComplete on general channel while closing. [{}]", err);
                }
            }

            // todo: Really, we should wait for the SessionComplete message to percolate through the entire system.
            // However, that's intensely hard to do with webrtc-rs, because I don't know how to get access to
            // the underlying transport that guarantees it's sent. Another way would be to get an ack from the client,
            // but oof, that's a lot of acks, lol.
            warn!("Using sleep. If we see missing SessionComplete messages under high load, this is why.");
            sleep(Duration::from_millis(500)).await;

            if let Err(err) = peer.close().await {
                error!("Failed to close PeerConnection: [{}]", err);
            }

            info!("Gracefully shutdown session: [{}]", session_state.session_id);
            Ok::<_, LineTrackingError>(())
        }
        .map_err(|err| error!("Failed to shutdown session: [{}]", err))
        .instrument(Span::current()),
    );

    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/sdp")
        .header(header::LOCATION, format!("whip/{session_id}"))
        .body(local_description.sdp)?)
}


/// Handles video tracks that the client sends to us.
async fn handle_track(
    track_id: i8,
    track: StatefulTrack,
    session_state: SessionState,
    graceful_shutdown: CancellationToken,
) -> Result<(), LineTrackingError> {
    info!(session_state.session_id, track_id = %track.id(), kind = ?track.kind(), "Track connected.");

    let codec = track.codec();
    let mime_type = codec.capability.mime_type.to_lowercase();
    info!(%mime_type, clock_rate = codec.capability.clock_rate, "Track codec details.");

    if !mime_type.as_str().eq_ignore_ascii_case(media_engine::MIME_TYPE_H264) {
        return Err(anyhow!("Video tracks must be in H264 format. [{}] id=[{}]", mime_type, track.id()).into());
    }

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
        .map_err(|e| anyhow!("Data path [{e:?}] provided for session streamer was not valid UTF8 string."))?;

    let pipeline_str = format!("appsrc name=appsrc ! h264parse ! mp4mux name=mux ! filesink location={output_file}",);

    info!("Starting pipeline [{}]", &pipeline_str);

    let pipeline = gstreamer::parse::launch(&pipeline_str)
        .unwrap()
        .downcast::<gstreamer::Pipeline>()
        .unwrap();

    // Get the appsrc element
    let appsrc = pipeline.by_name("appsrc").unwrap().downcast::<gstreamer_app::AppSrc>().unwrap();

    // Configure the appsrc
    appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);

    let rtp_caps = gstreamer::Caps::builder("video/x-h264")
        .field("stream-format", "byte-stream")
        .build();

    appsrc.set_caps(Some(&rtp_caps));
    appsrc.set_format(gstreamer::Format::Time); // Says that our buffers will have timestamps.
    appsrc.set_is_live(true); // A flag telling downstream pads that we are optimizing for live streaming content.

    // We manually reconstruct the timestamps from received RTP packets. See below.
    appsrc.set_do_timestamp(false);

    // Start the pipeline
    pipeline.set_state(gstreamer::State::Playing)?;

    info!("Started gstreamer pipeline: {:?}", pipeline);

    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Video stream connected. Streaming begin.".into(),
        })
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
    let mut rtp_packets = 0;

    let id = track.id();
    let mut packet_stream = track.into_rtp_stream();
    while let Some(data) = graceful_shutdown.run_until_cancelled(packet_stream.next()).await.flatten() {
        match data {
            Ok((packet, _)) => {
                rtp_packets += 1;
                trace!(
                    "Got rtp packet with sequence number {}, timestamp {}, and payload length {}",
                    packet.header.sequence_number,
                    packet.header.timestamp,
                    packet.payload.len()
                );

                if rtp_packets % 1000 == 0 {
                    info!("Video track [{}] has received {} packets total..", id, rtp_packets);
                }

                if packet.payload.len() == 0 {
                    trace!("Received RTP packet of size 0. Skipping.");
                    continue;
                }

                let h264_bytes = depacketizer.depacketize(&packet.payload)?;

                if h264_bytes.len() > 0 {
                    let mut buffer = gstreamer::Buffer::from_slice(h264_bytes);

                    // Our RTP stream timestamps are given in a specific clock rate, passed in above.
                    // That rate is in Hz, ie 90000Hz, so we convert to nanoseconds.
                    let timestamp_nanos = ClockTime::from_nseconds(
                        (packet.header.timestamp as u64 * ClockTime::SECOND.nseconds()) / codec.capability.clock_rate as u64,
                    );

                    buffer.get_mut().unwrap().set_pts(Some(timestamp_nanos));
                    buffer.get_mut().unwrap().set_dts(Some(timestamp_nanos));
                    appsrc.push_buffer(buffer).expect("Failed to push buffer");
                }
            }
            Err(err) => {
                error!("Error when reading RTP stream for video. [{:?}]", err);
                break;
            }
        };
    }

    info!("Finished receiving RTP stream. Received {rtp_packets} packets.");

    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Video stream closed.".into(),
        })
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

    info!("Finished receiving video track. Saved to {output_file}");

    Ok(())
}

/// The codecs that this server supports. We only support H264.
/// This is copied from webrtc-rs, because we only support codecs supported by webrtc-rc.
fn supported_codecs() -> Vec<RTCRtpCodecParameters> {
    let video_rtcp_feedback = vec![
        RTCPFeedback {
            typ: "goog-remb".to_owned(),
            parameter: "".to_owned(),
        },
        RTCPFeedback {
            typ: "ccm".to_owned(),
            parameter: "fir".to_owned(),
        },
        RTCPFeedback {
            typ: "nack".to_owned(),
            parameter: "".to_owned(),
        },
        RTCPFeedback {
            typ: "nack".to_owned(),
            parameter: "pli".to_owned(),
        },
    ];

    vec![
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42001f".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 102,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42001f".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 127,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 125,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42e01f".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 108,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=0;profile-level-id=42001f".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 127,
            ..Default::default()
        },
        RTCRtpCodecParameters {
            capability: RTCRtpCodecCapability {
                mime_type: media_engine::MIME_TYPE_H264.to_owned(),
                clock_rate: 90000,
                channels: 0,
                sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=640032".to_owned(),
                rtcp_feedback: video_rtcp_feedback.clone(),
            },
            payload_type: 123,
            ..Default::default()
        },
    ]
}

// Error handling
// -----

struct AppError(StatusCode, anyhow::Error);
impl AppError {

    fn new(status_code: StatusCode, err: anyhow::Error) -> Self {
        info!("Responding with error: {}", err.to_string());
        AppError(status_code, err)
    }
}

impl<T> From<T> for AppError
where
    T: std::error::Error + Send + Sync + 'static,
{
    fn from(err: T) -> Self {
        info!("Responding with error: {}", err.to_string());
        AppError(StatusCode::INTERNAL_SERVER_ERROR, anyhow::Error::msg(err.to_string()))
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

#[derive(Debug)]
pub struct LineTrackingError {
    pub error: anyhow::Error,
    pub location: &'static Location<'static>,
}

impl<E> From<E> for LineTrackingError
where
    E: Into<anyhow::Error>,
{
    #[track_caller]
    #[inline]
    fn from(error: E) -> Self {
        Self {
            error: error.into(),
            location: Location::caller(),
        }
    }
}

impl std::fmt::Display for LineTrackingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}, {}", self.error, self.location)
    }
}

pub fn default_process_setup(use_structured_logging: bool) {
    // Send top-level panics to tracing log (via log).
    log_panics::init();

    // Command line logging settings:
    // By default, set gstreamer to warn because it's quite noisy.
    // Filter using the environment vairable RUST_LOG)
    let cli_log_settings = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,gstreamer=warn"));

    let format_layer = if use_structured_logging {
        // tracing_subscriber::fmt::layer().json().boxed()
        tracing_stackdriver::layer().boxed()
    } else {
        tracing_subscriber::fmt::layer().compact().boxed()
    };

    tracing_subscriber::registry()
        .with(format_layer.with_filter(cli_log_settings))
        .init();
}
