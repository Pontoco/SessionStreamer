mod data_channels;
mod path_ext;
mod rest;
mod timestamped_bytes;
mod video_track;
mod webrtc_utils;

use anyhow::{Result, anyhow};
use axum::body::Body;
use axum::error_handling::HandleErrorLayer;
use axum::extract::{self, Query, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{HeaderValue, Request, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, get_service, post};
use axum::{Extension, Router};
use axum_extra::TypedHeader;
use axum_extra::headers::ContentType;
use bytes::Bytes;
use cookie::Cookie;
use futures::{StreamExt, TryFutureExt};
use http_body_util::BodyExt;
use http_body_util::combinators::{BoxBody, UnsyncBoxBody};
use locate_error::Locate;
use path_ext::PathExt;
use serde::{Deserialize, Serialize};
use std::backtrace;
use std::collections::HashMap;
use std::io::ErrorKind;
use std::panic::Location;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI8;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;
use thiserror::Error;
use tokio::fs::{self, File};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{Receiver, Sender, UnboundedSender};
use tokio::sync::{Mutex, mpsc};
use tokio::time::sleep;
use tokio_stream::wrappers::WatchStream;
use tokio_util::sync::CancellationToken;
use toml::Table;
use tower::filter::FilterLayer;
use tower::{BoxError, Service, ServiceBuilder, ServiceExt};
use tower_http::body::Full;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::services::fs::ServeFileSystemResponseBody;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::{self, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tower_http::validate_request::{ValidateRequest, ValidateRequestHeaderLayer};
use tower_oauth2_resource_server::server::OAuth2ResourceServer;
use tower_oauth2_resource_server::tenant::TenantConfiguration;
use tracing::{Instrument, Level, Span, debug, error, info};
use tracing::{trace, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};
use video_track::{determine_supported_video_tracks, handle_track};
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::{APIBuilder, interceptor_registry, media_engine};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::rtp_transceiver::RTCPFeedback;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTCRtpCodecParameters, RTPCodecType};
use webrtc::sdp::{MediaDescription, SessionDescription};
use webrtc_utils::StatefulPeerConnection;

#[derive(Clone)]
struct AppState {
    pub rtc: Arc<webrtc::api::API>,

    // The path on disk to store streamed data from game sessions.
    data_path: PathBuf,
}

#[derive(Clone)]
struct SessionState {
    pub app_state: AppState, // Global state associated with the runtime.
    pub session_id: String,  // GUID identifier for this session.
    pub data_path: PathBuf,  // The path where data for this session is stored.
    pub client_send_rx: Arc<Mutex<Option<Receiver<ServerMessage>>>>,
    pub client_send_tx: Sender<ServerMessage>, // Sends messages to the client via our data channel.
    pub messages_sink: UnboundedSender<String>, // Used to write Client and Server messages to disk as a text file.
    pub track_id: Arc<AtomicI8>,               // Incrementing track id for each connected video track.
    pub offered_video_tracks: Vec<MediaDescription>, // The set of media tracks the client sent in its offer. We expect to see each of them connect.
}

impl AppState {
    /// Retrieves the project folder path, or None if this user does not have access.
    pub async fn authorize_project_folder(&self, project_id: &str, email: &str) -> Result<PathBuf, LineTrackingError> {
        let auth_rules_txt = fs::read_to_string(self.data_path.join("project_authorization.toml")).await?;
        let rules: Table = auth_rules_txt.parse::<toml::Table>().map_err(|_| AuthError::RuleMalformed)?;

        let rule = rules.get(project_id).ok_or(AuthError::NoRuleForProject)?;

        let email_patterns = rule
            .get("authorized_emails")
            .and_then(|e| e.as_array())
            .ok_or(AuthError::RuleMalformed)?;

        for pattern in email_patterns {
            let glob = globset::Glob::new(pattern.as_str().ok_or(AuthError::RuleMalformed)?)?.compile_matcher();
            if glob.is_match(email) {
                return Ok(self.data_path.join_safe(project_id)?);
            }
        }

        debug!("User [{email}] is not authorized for project [{project_id}]");
        Err(AuthError::NotAuthorized)?
    }
}

/// Sent from the server to control or inform the client.
/// We can use this to control the client, to inform it of information, or to receive commands.
/// We have to avoid using complex enums here since C# can't easily parse those.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(tag = "kind")]
pub enum ServerMessage {
    Info { message: String },   // Info is casual, non-chatty information.
    Notice { message: String }, // A notice is a recovered error. It's a failure that is stronger than a warning, but non-fatal.
    Error { message: String },  // An error is fatal and should be displayed to the client. It implies something has really gone wrong.
    SessionComplete,            // Sent to the client after it requests a graceful shutdown of the session.
}

impl ServerMessage {
    pub fn info<M: Into<String>>(message: M) -> Self {
        ServerMessage::Info { message: message.into() }
    }

    pub fn notice<M: Into<String>>(message: M) -> Self {
        ServerMessage::Notice { message: message.into() }
    }
    pub fn error<M: Into<String>>(message: M) -> Self {
        ServerMessage::Error { message: message.into() }
    }
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

// pub fn project_id_auth_service<S, ReqBody, RespBody>(
//     inner_service: S,
// ) -> impl Service<
//     Request<ReqBody>,
//     Response = Response<Body>, // The final response type is http::Response<Body>
//     Error = BoxError,          // The final error type is BoxError due to Filter & MapResult
// > + Clone
// where
//     S: Service<Request<ReqBody>, Response = Response<RespBody>, Error = BoxError> + Clone + Send + 'static,
//     S::Error: Into<BoxError> + Send + Sync + 'static, // Inner service's error must be convertible
//     S::Future: Send + 'static,
//     ReqBody: Send + 'static,
//     RespBody: Send + 'static
// {
//     // Hard-coded list of approved project IDs
//     const APPROVED_PROJECT_IDS: &[&str] = &["proj_alpha", "proj_beta", "admin_proj"];

//     ServiceBuilder::new()
//         // Layer 1: Filter requests based on the project ID.
//         // If the predicate returns Err, the Filter service errors with that specific error.
//         .layer(FilterLayer::new(move |req: Request<ReqBody>| {
//             let path = req.uri().path();
//             let first_param = path.split_terminator('/').nth(1).unwrap_or_default();

//             if APPROVED_PROJECT_IDS.contains(&first_param) {
//                 Ok(req) // Request is authorized, pass it through
//             } else {
//                 // Request is not authorized, return our custom error
//                 Err(Box::new(AuthError::NotAuthorized) as BoxError)
//             }
//         }))
//         // Layer 2: Map the result of the Filtered service.
//         // This layer catches our custom error and transforms it into a 401 HTTP Response.
//         .map_result(|result: Result<Response<RespBody>, BoxError>| {
//             match result {
//                 Ok(response) => Ok(response), // Pass through successful responses
//                 Err(err) => {
//                     // Check if the error is our specific UnauthorizedByProjectIdError
//                     if err.is::<AuthError>() {
//                         // Transform the custom error into a 401 HTTP Response
//                         let response_401 = Response::builder()
//                             .status(StatusCode::UNAUTHORIZED)
//                             .header(CONTENT_TYPE, "text/plain; charset=utf-8")
//                             .body(Body::from(err.to_string())) // Use the error's message for the body
//                             .unwrap(); // Should not fail for a valid builder
//                         Ok(response_401)
//                     } else {
//                         // It's a different error (e.g., from the inner_service), propagate it.
//                         Err(err)
//                     }
//                 }
//             }
//         })
//         // Apply the layers to the inner service
//         .service(inner_service)
// }
/// Custom claims extractor to get only what you need, including email.
#[derive(Debug, Deserialize, Clone)]
pub struct UserEmailClaim {
    email: String,
}

pub async fn create_server(data_path: impl Into<PathBuf>, client_files: impl Into<PathBuf>) -> Result<axum::Router> {
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

    // Create an auth layer

    let client_path = client_files.into();
    info!("Serving client on route: [{}]", &client_path.display());

    let dir = ServeDir::new(data_path.clone());

    /// Axum middleware to extract a JWT from a cookie and set the Authorization header.
    ///
    /// It checks for a cookie named `auth_token` (you should change this name
    /// to match your cookie). If found, and if the `Authorization` header
    /// is not already present, it sets `Authorization: Bearer <token>`.
    async fn auth_cookie_to_header(mut req: extract::Request, next: Next) -> Response {
        let headers = req.headers_mut();
        debug!("Rewriting headers from cookies");

        // Only proceed if the Authorization header is NOT already set.
        if !headers.contains_key(header::AUTHORIZATION) {
            debug!("1 Rewriting headers from cookies");
            // Try to get the cookie header.
            if let Some(cookie_header) = headers.get(header::COOKIE) {
                debug!("Got cookie");
                // Convert the cookie header to a string.
                if let Ok(cookie_str) = cookie_header.to_str() {
                    debug!("cookie:{}", &cookie_str);
                    // Define the name of the cookie to look for.
                    const COOKIE_NAME: &str = "auth_token"; // <--- CHANGE THIS IF NEEDED

                    // Iterate through cookies, parse them, and find the one we need.
                    for cookie in Cookie::split_parse(cookie_str.to_string()) {
                        match cookie {
                            Err(err) => error!("Couldn't parse cookie: {}", err),
                            Ok(cookie) => {
                                if cookie.name() == COOKIE_NAME {
                                    debug!("found token:{}", &cookie.value());
                                    // ...try to create a "Bearer <token>" header value.
                                    if let Ok(value) = HeaderValue::from_str(&format!("Bearer {}", cookie.value())) {
                                        debug!("converted to auth:{:?}", &value);
                                        // ...and insert it into the request headers.
                                        headers.insert(header::AUTHORIZATION, value);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Pass the (potentially modified) request to the next middleware or handler.
        next.run(req).await
    }

    async fn middle(
        State(state): State<AppState>,
        extract::Path((project_id, _)): extract::Path<(String, String)>,
        claims: Extension<UserEmailClaim>,
        request: extract::Request,
        next: Next,
    ) -> Response {
        debug!("Authentication user for data access: {}", &claims.email);
        if let Err(err) = state.authorize_project_folder(&project_id, &claims.email).await {
            // Request is not authorized, return our custom error
            debug!("User [{}] is not authorized for project [{}]: {}", &claims.email, &project_id, err);
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .header(CONTENT_TYPE, "text/plain; charset=utf-8")
                .body(Body::from("Not authorized!")) // Use the error's message for the body
                .unwrap(); // Should not fail for a valid builder
        }

        info!("authorized, passing through");
        return next.run(request).await;
    }

    let auth_for_data_access = axum::middleware::from_fn_with_state(state.clone(), middle);

    let authorized_data_layer = ServiceBuilder::new()
        .layer(auth_for_data_access)
        .service(dir); // Serve the static data from the sessions.

    let oauth2_resource_server = OAuth2ResourceServer::<UserEmailClaim>::builder()
        .add_tenant(
            TenantConfiguration::builder(format!("https://accounts.google.com"))
                .audiences(&["250832464539-0m471qro1qad8108jel2kqu3dbcaldii.apps.googleusercontent.com"])
                .build()
                .await
                .expect("Failed to build tenant configuration"),
        )
        .build()
        .await
        .expect("Failed to build OAuth2 resource server");

    // --- Routes requiring OAuth2 ---
    let protected_routes = Router::new()
        .nest("/data", Router::new().route_service("/{project_id}/{*path}", authorized_data_layer))
        .nest("/rest", rest::routes().await)
        .layer(ServiceBuilder::new().layer(oauth2_resource_server.into_layer()))
        .layer(axum::middleware::from_fn(auth_cookie_to_header));

    // --- Main application router ---
    Ok(Router::new()
        .route("/whip", post(whip_post_handler)) // WHIP route - NO OAuth2
        .merge(protected_routes) // Merge the protected routes
        .fallback(get_service(
            ServeDir::new(&client_path).fallback(get_service(ServeFile::new(client_path.join_safe("index.html")?))),
        ))
        // --- Global Layers (Apply to ALL routes) ---
        .layer(
            trace::TraceLayer::new_for_http()
                .make_span_with(trace::DefaultMakeSpan::new().level(Level::INFO))
                .on_response(trace::DefaultOnResponse::new().level(Level::INFO))
        )
        .with_state(state))
}

#[derive(Error, Debug)]
enum AuthError {
    #[error("Can't load rules file")]
    CantLoadRules,
    #[error("No rule for given project")]
    NoRuleForProject,
    #[error("Rules file was malformed")]
    RuleMalformed,
    #[error("User is not authorized")]
    NotAuthorized,
}

impl SessionState {
    /// Sends a message to the client.
    /// If the send fails, logs an error.
    pub async fn send_client_info<M: Into<String>>(&self, msg: M) {
        let msg = msg.into();
        if let Err(err) = self.client_send_tx.send(ServerMessage::Error { message: msg.clone() }).await {
            error!("Failed to send message to client: [{}] [{}]", msg, err);
        }
    }

    pub async fn send_client(&self, msg: ServerMessage) {
        if let Err(err) = self.client_send_tx.send(msg.clone()).await {
            error!("Failed to send message to client: [{:?}] [{}]", msg, err);
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

    if content_type != "application/sdp".parse().unwrap() {
        return Err((StatusCode::UNSUPPORTED_MEDIA_TYPE, "Content-Type must be application/sdp."))?;
    }

    ensure_app(
        query_params.contains_key("session_id"),
        StatusCode::BAD_REQUEST,
        "Query parameter missing: [session_id]",
    )?;

    ensure_app(
        query_params.contains_key("project_id"),
        StatusCode::BAD_REQUEST,
        "Query parameter missing: [project_id]",
    )?;

    let (client_send, client_send_rx) = tokio::sync::mpsc::channel::<ServerMessage>(100);

    // todo: Verify this session_id hasn't already been created. Some kind of disk file to represent it?
    let session_id = query_params["session_id"].clone();
    let project_id = query_params["project_id"].clone();
    let data_path = state
        .data_path
        .join_safe(&project_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "project_id invalid"))?
        .join_safe(&session_id)
        .map_err(|_| (StatusCode::BAD_REQUEST, "project_id invalid"))?;

    if fs::try_exists(&data_path).await? {
        return Err((
            StatusCode::CONFLICT,
            format!("Session ID already exists! [{}] path: [{:?}]", &session_id, &data_path),
        ))?;
    }

    debug!("Creating session directory: {}", data_path.display());
    fs::create_dir_all(&data_path).await?;

    info!(
        "Created new game streaming session. project={} session={} path={}",
        &project_id,
        &session_id,
        data_path.display()
    );

    // todo! It's important that this gets cancelled on drop.
    // Right now we have a memory leak if we hit an error, because the spawned tasks will not be killed.
    let graceful_shutdown_src = CancellationToken::new();

    let (messages_sink, mut messages_sink_rx) = mpsc::unbounded_channel::<String>();

    let mut session_state = SessionState {
        client_send_tx: client_send.clone(),
        client_send_rx: Arc::new(Mutex::new(Some(client_send_rx))),
        messages_sink: messages_sink,
        app_state: state.clone(),
        track_id: Arc::new(AtomicI8::new(0)),
        data_path: data_path,
        session_id: session_id.clone(),
        offered_video_tracks: vec![],
    };

    // Dump the metadata in the data path.
    fs::write(session_state.data_path.join("metadata.json"), serde_json::to_string(&query_params)?).await?;

    // Write any messages we send or receive to a disk file.
    let mut messages_file = File::create(session_state.data_path.join("messages.txt")).await?;
    tokio::spawn(
        async move {
            if let Err(err) = (async || {
                while let Some(text) = messages_sink_rx.recv().await {
                    messages_file.write(text.as_bytes()).await?;
                    messages_file.write(b"\n").await?;
                }
                Ok::<_, LineTrackingError>(())
            })()
            .await
            {
                error!("{}", err);
            }
        }
        .instrument(Span::current()),
    );

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

    let peer = StatefulPeerConnection::new(state.rtc.new_peer_connection(config).await?);

    debug!("Peer Connection created.");

    // Buffer an initial message that will be sent once our general data channel is connected.
    session_state
        .client_send_tx
        .send(ServerMessage::Info {
            message: "Connected to general data channel!".into(),
        })
        .await
        .map_err(|_| "Channel closed")?;

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

    trace!("Received offer: {}", &body);
    let offer = RTCSessionDescription::offer(body)?;
    let (supported_tracks, notices) = determine_supported_video_tracks(&offer.unmarshal()?);
    for notice in notices {
        session_state.send_client(notice).await;
    }
    session_state.offered_video_tracks.extend(supported_tracks);

    debug!("Setting remote and local descriptions.");
    peer.set_remote_description(offer).await?;

    let answer = peer.create_answer(None).await?;
    peer.set_local_description(answer).await?;

    peer.on_gathering_complete().await.recv().await;

    let answer = peer
        .local_description()
        .await
        .ok_or("Local description did not get set properly.")?;

    trace!("Sending answer: {}", &answer.sdp);

    validate_track_encodings(&session_state, &answer.unmarshal()?).await;

    info!("ICE gathering complete. Waiting to connect...");

    // Close the connection manually if the client requests it.
    // This is necessary because webrtc-rs does not properly handle the close_notify alert on the underlying data transport.
    // https://github.com/webrtc-rs/webrtc/issues/672
    let _ = tokio::spawn(
        async move {
            // Wait for either:
            //   - graceful shutdown request
            //   - connection state closed
            let mut graceful = false;
            loop {
                tokio::select! {
                    _ = graceful_shutdown_src.cancelled() => {
                        info!("Starting graceful shutdown of session: {}", session_state.session_id);
                        graceful = true;
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
            let track_num = tracks.len();

            info!("Waiting for [{}] tracks to finish.", tracks.len());
            for handle in tracks {
                handle.await?;
            }

            // Send the final OK, if it was graceful!
            // Session Done.

            // Send a graceful ack if we can.
            if graceful {
                // Send any errors about media tracks we never saw.
                let expected_track_num = session_state.offered_video_tracks.len();
                if track_num != expected_track_num {
                    error!(
                        "Expected [{}] media tracks to connect during the session, but [{}] connected.",
                        expected_track_num, track_num
                    );
                    session_state
                        .send_client(ServerMessage::error(format!(
                            "Expected [{}] media tracks to connect during the session, but [{}] connected.",
                            expected_track_num, track_num
                        )))
                        .await;
                }

                info!("Sending SessionComplete message.");
                if let Err(err) = session_state.client_send_tx.send(ServerMessage::SessionComplete).await {
                    error!(
                        "Failed to send SessionComplete on general channel while closing gracefully.. [{}]",
                        err
                    );
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
        .body(answer.sdp)?)
}

// Verifies that the SDP Answer we're sending back includes a valid codec.
// If the media track id is set to 0, or we couldn't find a codec, that's a sign that there were
// no codecs shared between the client and the server.
async fn validate_track_encodings(session_state: &SessionState, answer: &SessionDescription) {
    for md_answer in answer.media_descriptions.iter() {
        let media_type = md_answer.media_name.media.to_lowercase();
        let mid_answer = md_answer.attribute("mid");

        if md_answer.media_name.port.value == 0 {
            info!(
                "Media section for type '{}' (MID: {:?}) in the answer has port 0. This indicates it's disabled,
                 likely due to no common codecs or explicit rejection.",
                media_type, mid_answer
            );
            session_state
                .send_client_info("Could not find common codec. Media track cannot connect.")
                .await;
            // Here, you know that the transport for this media section will not be established.
        } else if !md_answer.has_attribute("rtpmap") && media_type != "application" {
            // Another check: if it's not port 0, but has no rtpmap (codec) lines, it's also problematic for RTP-based media.
            // Data channels ("application") don't use rtpmap.
            info!(
                "Media section for type '{}' (MID: {:?}) in the answer has no rtpmap attributes.
                 This indicates no codecs were included for this stream.",
                media_type, mid_answer
            );
            session_state
                .send_client_info("Could not find common codec. Media track cannot connect.")
                .await;
        }
    }
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

#[derive(Error, Debug, Locate)]
enum AppError {
    #[error("{}", 1)]
    Manual(StatusCode, String),
    #[error("webrtc error: {0}")]
    WebRtc(#[locate_from] webrtc::Error, locate_error::Location),
    #[error("io error: {0}")]
    Io(#[locate_from] std::io::Error, locate_error::Location),
    #[error("http error: {0}")]
    Http(#[from] axum::http::Error),
    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl<T> From<(StatusCode, T)> for AppError
where
    T: Into<String>,
{
    fn from((status_code, err): (StatusCode, T)) -> Self {
        AppError::Manual(status_code, err.into())
    }
}

impl From<String> for AppError {
    fn from(msg: String) -> Self {
        AppError::Manual(StatusCode::INTERNAL_SERVER_ERROR, msg)
    }
}

impl From<&str> for AppError {
    fn from(msg: &str) -> Self {
        AppError::Manual(StatusCode::INTERNAL_SERVER_ERROR, msg.to_string())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        error!("into: {:?}", self);
        match self {
            AppError::Manual(status_code, msg) => (status_code, msg).into_response(),
            e => (StatusCode::INTERNAL_SERVER_ERROR, format!("Internal Server Error: {}", e)).into_response(),
        }
    }
}

fn ensure_app(condition: bool, status_code: StatusCode, msg: &'static str) -> Result<(), AppError> {
    if !condition {
        return Err((status_code, msg))?;
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

pub fn configure_logging(use_structured_logging: bool) {
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
