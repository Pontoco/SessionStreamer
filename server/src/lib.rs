use anyhow::{Result, ensure};
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum_extra::TypedHeader;
use axum_extra::headers::ContentType;
use clap::Parser;
use clap::arg;
use gstreamer::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::AtomicI8;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::{Arc, mpsc};
use tokio::net::TcpListener;
use tokio::sync::mpsc::Sender;
use tracing::instrument;
use tracing::{error, info};
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, interceptor_registry, media_engine};
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::rtp_transceiver::RTCRtpTransceiver;
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::track::track_remote::TrackRemote;

#[derive(Clone)]
struct AppState {
    pub rtc: Arc<webrtc::api::API>,

    // The path on disk to store streamed data from game sessions.
    pub data_path: PathBuf,
}

#[derive(Clone)]
struct SessionState {
    pub client_channel: Sender<DataMessage>,
    pub app_state: AppState,
}

/// The data type of messages that get passed back and forth across our data channel with the client.
/// We can use this to control the client, to inform it of information, or to receive commands.
#[derive(Serialize, Deserialize, Debug)]
pub enum DataMessage {
    Info(String),
    Error(String),
    ServerSignal(ServerSignal)
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ServerSignal {
    VideoStreamComplete
}

impl<E> From<E> for DataMessage
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        DataMessage::Error(format!("{}", value.into()))
    }
}

/// Server-side state that lives while a specific session is being recorded.
struct Session {
    pub client_data_send: mpsc::Sender<DataMessage>,
}

pub fn create_server() -> Result<axum::Router> {
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
        data_path: PathBuf::from("./data"),
    };

    Ok(axum::Router::new()
        .route("/whip", post(whip_post_handler))
        .with_state(state))
}

#[derive(Deserialize)]
struct QueryParams {
    pub session_id: String,
}

#[instrument(skip_all)]
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

    let session_id = query.session_id;

    let (tx, rx) = tokio::sync::mpsc::channel::<DataMessage>(100);
    let session_state = SessionState {
        client_channel: tx.clone(),
        app_state: state.clone(),
    };

    // todo: Verify this session_id hasn't already been created. Some kind of disk file to represent it?

    let config = RTCConfiguration {
        ..Default::default()
    };

    let peer = state
        .rtc
        .new_peer_connection(config)
        .await
        .map_err(|e| AppError(StatusCode::INTERNAL_SERVER_ERROR, e.into()))?;

    // Send anything to this channel you want sent to the client via our data channel.
    tx.send(DataMessage::Info("Data channel connected!".into()))
        .await?;

    let mut rx_slot = Some(rx);

    // Wait for the 'general' data channel to connect and receive messages on the other end of the above channel.
    peer.on_data_channel(Box::new(move |channel| {
        info!("Data channel connected: {}", &channel.label());

        if channel.label() != "general" {
            error!("Unexpected data channel [{}]", channel.label());
            return Box::pin(async {});
        }

        if let Some(mut rx) = rx_slot.take() {
            // Make sure to log any errors with the data channel.
            channel.on_error(Box::new(|error| {
                Box::pin(async move {
                    error!("Data channel had error: {}", error);
                })
            }));

            let sender = channel.clone();
            channel.on_open(Box::new(move || {
                Box::pin(async move {
                    // Our primary 'send stuff to the client' channel.
                    while let Some(data) = rx.recv().await {
                        match serde_json::to_string(&data) {
                            Ok(json) => match sender.send_text(json).await {
                                Ok(_) => info!("Sent message to client [{:?}]", &data),
                                Err(err) => error!("Error sending message to client [{}]", err),
                            },
                            Err(err) => {
                                error!("Failed to serialize data message. {} {:?}", err, data)
                            }
                        }
                    }
                })
            }));
        } else {
            error!("Data channel already initialized [{}]", channel.label());
        }

        Box::pin(async {})
    }));

    let track_index = AtomicI8::new(0);
    peer.on_track(Box::new(
        move |track: Arc<TrackRemote>,
              _receiver: Arc<RTCRtpReceiver>,
              _tranceiver: Arc<RTCRtpTransceiver>| {
            let index = track_index.fetch_add(1, SeqCst);
            let sess_id = session_id.clone();
            let state = session_state.clone();
            let tx = tx.clone();
            Box::pin(async move {
                if let Err(err) = handle_track(index, track, &sess_id, state).await {
                    let _ = tx
                        .send(DataMessage::from(err))
                        .await
                        .map_err(|err| error!("{}", err));
                }
            })
        },
    ));

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
    session_state: SessionState,
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

    // Depacketize using h264writer.
    // Write to a temporary buffer.
    // Write to .h264 file.

    // Send that buffer to the h264 parser. Parse into samples.
    // Send that data and samples to our mp4 *writer*. Buuuut tbh, that's a fuckup because now we're re-encoding it I think? Ugh.
    // Nvm. We're not reencoding, just stuffing these samples somehow into

    let mut depacketizer = H264Packet::default();

    // Initialize GStreamer
    gstreamer::init()?;
    info!("Initializing GStreamer pipeline for track {track_id}.");

    // Build the GStreamer pipeline
    let pipeline_str = format!(
        "appsrc name=appsrc ! h264parse ! mp4mux ! filesink location=output_{track_id}.mp4"
    );
    let pipeline = gstreamer::parse_launch(&pipeline_str)
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
    let caps = gstreamer::Caps::builder("video/x-h264").build();
    appsrc.set_caps(Some(&caps));
    appsrc.set_stream_type(gstreamer_app::AppStreamType::Stream);
    appsrc.set_format(gstreamer::Format::Time);

    // Start the pipeline
    pipeline.set_state(gstreamer::State::Playing).unwrap();

    info!("Started gstreamer pipeline: {:?}", pipeline);

    session_state
        .client_channel
        .send(DataMessage::Info("Video stream connected. Streaming begin.".into()))
        .await?;

    while let Ok((packet, _)) = track.read_rtp().await {
        let bytes = depacketizer.depacketize(&packet.payload)?;

        // Create a GStreamer buffer from the H.264 data
        let buffer = gstreamer::Buffer::from_slice(bytes);

        // Push the buffer into the pipeline
        let _ = appsrc.push_buffer(buffer);
    }

    session_state
        .client_channel
        .send(DataMessage::Info("Video stream closed.".into()))
        .await?;

    // Signal end of stream
    appsrc.end_of_stream()?;

    // Wait for the pipeline to finish
    pipeline.set_state(gstreamer::State::Null).unwrap();


    Ok(())

    // Create a new mp4 file.
    // let config = Mp4Config {
    //     major_brand: FourCC::from_str("isom"),
    //     minor_version: 512,
    //     compatible_brands: vec![
    //         str::parse("isom").unwrap(),
    //         str::parse("iso2").unwrap(),
    //         str::parse("avc1").unwrap(),
    //         str::parse("mp41").unwrap(),
    //     ],
    //     timescale: 1000
    // };
    //
    // let file = File::create(format!("game_video_{track_id}.mp4"))?;

    // let mut writer = mp4::Mp4Writer::write_start(file, config)?;
    //
    //
    // // We need to parse these out of the h264 stream. Boo!
    // // Can we just dump the h264 stream to the mp4 in tact..?
    // let h264_config = AvcConfig {
    //     width: ,
    //     height: ,
    //     seq_param_set: ,
    //     pic_param_set: ,
    // };
    //
    // // add track
    // writer.add_track(TrackConfig {
    //     track_type: TrackType::Video,
    //     timescale: 90000,
    //     language: "und".to_string(),
    //     media_conf: MediaConfig::AvcConfig(h264_config)
    // })?;
    //
    // writer.write_sample();
    //
    // writer.write_end()?;

    // Depacketize from RTP and save h264 frames into a MP4.

    // Determine track type and create depacketizer
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
