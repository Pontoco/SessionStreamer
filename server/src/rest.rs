use std::fs;
use std::{fs::File, panic::Location};

use axum::Router;
use axum::extract::Path;
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
        .route("/session/{session_id}", get(get_session_info))
        .route("/list", get(get_session_list))
}

/// Gets a list of all sessions under this project. Returns each session's metadata json object.
pub async fn get_session_list(State(app_state): State<AppState>) -> Result<Json<Vec<SessionMetadata>>, RestError> {
    let mut metadata_list = Vec::new();
    for entry in fs::read_dir(&app_state.data_path)? {
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

/// Gets the data for display on the individual session page. Usually fairly small, because the bigger files are on separate URLs.
/// Eventually we may feed these urls through to GCS, but for now we stream them through the server to retain local debugging simplicity.
pub async fn get_session_info(State(app_state): State<AppState>, Path(session_id): Path<String>) -> Result<Json<SessionData>, RestError> {
    let metadata_path = app_state.data_path.join(&session_id).join("metadata.json");

    let metadata_file = match File::open(metadata_path) {
        Ok(file) => file,
        Err(_) => return Err(RestError::new(StatusCode::BAD_REQUEST, "session_id does not exist")),
    };

    let metadata: Value = serde_json::from_reader(metadata_file)?;
    let video_url = format!("/data/{}/game_capture_0.mp4", session_id);
    let log_url = format!("/data/{}/data_unity_log.txt", session_id);

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
    T: std::error::Error
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
