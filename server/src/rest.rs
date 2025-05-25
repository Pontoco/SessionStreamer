use std::path::PathBuf;
use std::{fs, path};
use std::{fs::File, panic::Location};

use axum::Router;
use axum::extract::{Path, Query};
use axum::routing::get;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{error, warn};

use crate::AppState;

type SessionMetadata = serde_json::Value;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/session", get(get_session_info))
        .route("/list", get(get_session_list))
}

#[derive(Serialize, Deserialize)]
struct ListQuery {
    pub project_id: String,
}

/// Gets a list of all sessions under this project. Returns each session's metadata json object.
pub async fn get_session_list(
    State(app_state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<SessionMetadata>>, RestError> {
    let mut metadata_list = Vec::new();

    let project_path = &app_state
        .data_path
        .join_safe(query.project_id)
        .ok_or_else(|| RestError::new(StatusCode::BAD_REQUEST, "project_id not valid"))?;

    for entry in fs::read_dir(&project_path)? {
        let session_path = entry?.path();
        if session_path.is_dir() {
            let metadata_path = session_path.join("metadata.json");
            if metadata_path.exists() {
                let metadata_file = File::open(metadata_path)?;
                metadata_list.push(serde_json::from_reader(metadata_file)?);
            } else {
                warn!("metadata.json for [{}] not found", session_path.display());
            }
        } else {
            warn!(
                "unexpected file [{}] in data directory [{}]",
                session_path.display(),
                &app_state.data_path.display()
            );
        }
    }
    Ok(Json(metadata_list))
}

#[derive(Serialize, Deserialize)]
struct SessionInfoQuery {
    pub session_id: String,
    pub project_id: String,
}

/// Gets the data for display on the individual session page. Usually fairly small, because the bigger files are on separate URLs.
/// Eventually we may feed these urls through to GCS, but for now we stream them through the server to retain local debugging simplicity.
pub async fn get_session_info(
    State(app_state): State<AppState>,
    Query(query): Query<SessionInfoQuery>,
) -> Result<Json<SessionData>, RestError> {
    let metadata_path = app_state
        .data_path
        .join_safe(&query.project_id)
        .ok_or(RestError::new(StatusCode::BAD_REQUEST, "project_id invalid"))?
        .join_safe(&query.session_id)
        .ok_or(RestError::new(StatusCode::BAD_REQUEST, "session_id invalid"))?
        .join("metadata.json");

    let metadata_file = match File::open(metadata_path) {
        Ok(file) => file,
        Err(_) => return Err(RestError::new(StatusCode::BAD_REQUEST, "session does not exist")),
    };

    let metadata: Value = serde_json::from_reader(metadata_file)?;
    let video_url = format!("/data/{}/game_capture_0.mp4", query.session_id);
    let log_url = format!("/data/{}/data_unity_log.txt", query.session_id);

    Ok(Json(SessionData {
        metadata,
        video_url,
        log_url,
    }))
}

#[derive(Serialize, Deserialize)]
pub struct SessionData {
    pub metadata: SessionMetadata,
    pub video_url: String,
    pub log_url: String,
}

pub struct RestError(StatusCode, String);
impl RestError {
    fn new<T>(status_code: StatusCode, body: T) -> Self
    where
        T: Into<String>,
    {
        RestError(status_code, body.into())
    }
}

// Internal server errors should print error but return simple message.
impl<T> From<T> for RestError
where
    T: std::error::Error,
{
    /// Converts a library error into a top-level error, which can be returned as a Response.
    #[track_caller]
    #[inline]
    fn from(err: T) -> Self {
        // Display with 'alternate' format should print the maximal information.
        // We prefix it too, so that we know it's coming from a top-level handler.
        let caller = Location::caller();
        error!("ISE({}:{}): {:#}", caller.file(), caller.line(), err);

        // For now, pass through top-level message.
        // Eventually we may want to lock this down for security.
        let body = format!("{}", err);
        RestError(StatusCode::INTERNAL_SERVER_ERROR, body)
    }
}

impl IntoResponse for RestError {
    fn into_response(self) -> Response {
        (self.0, self.1).into_response()
    }
}

trait PathExt {
    fn join_safe<P>(&self, relative: P) -> Option<PathBuf>
    where
        P: AsRef<std::path::Path>;
}

impl<P> PathExt for P
where
    P: AsRef<std::path::Path>,
{
    // to prevent directory traversal attacks we ensure the path consists of exactly one normal
    // component
    fn join_safe<P2>(&self, relative: P2) -> Option<PathBuf>
    where
        P2: AsRef<std::path::Path>,
    {
        let p = path::absolute(self.as_ref().join(relative)).ok()?;

        let mut components = p.components().peekable();

        if let Some(first) = components.peek() {
            if !matches!(first, std::path::Component::Normal(_)) || components.count() != 1 {
                return None;
            }
        }

        Some(p)
    }
}
