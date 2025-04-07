use anyhow::{Context, ensure};
use axum::extract::{Query, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum_extra::headers::ContentType;
use axum_extra::TypedHeader;
use clap::Parser;
use clap::arg;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI8;
use tokio::net::TcpListener;
// use mp4::{AvcConfig, FourCC, MediaConfig, Mp4Config, TrackConfig};
// use mp4::MediaConfig::AvcConfig;
use tracing::{error, info};
use tracing::instrument;
use webrtc::api::media_engine::MIME_TYPE_H264;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp::codecs::h264::H264Packet;
use webrtc::rtp::packetizer::Depacketizer;
use webrtc::track::track_remote::TrackRemote;

#[derive(Parser, Debug)]
struct CommandLineArgs {
    #[arg(default_value = "./data/")]
    pub data_path: PathBuf,
}

#[derive(Debug)]
enum TrackType {
    Video,
    Audio,
}

#[derive(Clone)]
struct AppState {
    // You can add shared state here if needed
    pub rtc: Arc<webrtc::api::API>,
    pub data_path: PathBuf,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt().init();

    let args = CommandLineArgs::parse();
    let rtc_api = webrtc::api::APIBuilder::new().build();

    // Setup the axum server.
    let state = AppState {
        rtc: Arc::new(rtc_api),
        data_path: PathBuf::from("./data"),
    };

    let app = axum::Router::new()
        .route("/whip", post(whip_post_handler))
        .with_state(state);

    axum::serve(TcpListener::bind("0.0.0.0:3000").await.unwrap(), app);

    // Simulate some async work
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    println!("Async work completed.");
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

    // todo: Verify this session_id hasn't already been created. Some kind of disk file to represent it?

    let config = RTCConfiguration {
        ..Default::default()
    };

    let peerConnection = state
        .rtc
        .new_peer_connection(config)
        .await
        .context("Failed to create a peer connection.")?;

    let offer = RTCSessionDescription::offer(body)?;

    peerConnection.set_remote_description(offer).await?;

    let answer = peerConnection.create_answer(None).await?;
    peerConnection.set_local_description(answer).await?;

    let local_description = peerConnection.local_description().await.ok_or_else(|| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            anyhow::anyhow!("Local description did not get set properly."),
        )
    })?;

    info!("WHIP session created.");

    let track_index = AtomicI8::new(0);

    peerConnection.on_track(Box::new(
        move |track: Arc<TrackRemote>, _receiver, _stream_id| {
            let id = track_index.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let session_id = session_id.clone();
            let state = state.clone();
            let f = async move || -> () {
                if let Err(e) = handle_track(id, track, &session_id, state).await {
                    error!("WebRTC track handling failed: {}", e);
                }
            };
            Box::pin(f())
        },
    ));

    Ok(Response::builder()
        .status(StatusCode::CREATED)
        .header(header::CONTENT_TYPE, "application/sdp")
        .body(local_description.sdp)?)
}

async fn handle_track<'a>(
    track_id: i8,
    track: Arc<TrackRemote>,
    session_id: &'a str,
    app_state: AppState,
) -> anyhow::Result<()> {
    info!(session_id = %session_id, track_id = %track.id(), kind = ?track.kind(), "Track received");

    let codec = track.codec();
    let mime_type = codec.capability.mime_type.to_lowercase();
    info!(%mime_type, clock_rate = codec.capability.clock_rate, "Track codec details");

    ensure!(
        mime_type.as_str() == MIME_TYPE_H264,
        "Video tracks must be in H264 format."
    );

    // Depacketize using h264writer.
    // Write to a temporary buffer.
    // Write to .h264 file.

    // Send that buffer to the h264 parser. Parse into samples.
    // Send that data and samples to our mp4 *writer*. Buuuut tbh, that's a fuckup because now we're re-encoding it I think? Ugh.
    // Nvm. We're not reencoding, just stuffing these samples somehow into

    let mut depacketizer = H264Packet::default();

    while let Ok((packet, _)) = track.read_rtp().await {
        // todo: do we need to handle the rror somehow, or just bubble up?
        let bytes = depacketizer.depacketize(&packet.payload)?;
    }

    // Track finished.
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
    T: std::error::Error,
{
    fn from(err: T) -> Self {
        AppError(StatusCode::INTERNAL_SERVER_ERROR, anyhow::anyhow!(err))
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, self.1.to_string()).into_response()
    }
}

fn ensure_app(condition: bool, status_code: StatusCode, msg: &'static str) -> Result<(), AppError> {
    if !condition {
        let s = msg.clone();
        return Err(AppError(status_code, anyhow::anyhow!(msg)));
    }
    Ok(())
}
