use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use include_dir::{include_dir, Dir};
use serde::Deserialize;
use serde::Serialize;
use tokio::time::timeout;

use crate::address;
use crate::config::Config;
use crate::store::{Message, MessageSummary, Store};

static EMBEDDED_PUBLIC_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/public");

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<Config>,
    pub store: Store,
}

#[derive(Debug, Deserialize)]
struct EmailQuery {
    email: Option<String>,
}

#[derive(Debug, Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
}

#[derive(Debug, Serialize)]
struct ListResponse {
    mailbox: String,
    email: String,
    count: usize,
    messages: Vec<MessageSummary>,
}

#[derive(Debug, Serialize)]
struct DetailResponse {
    mailbox: String,
    email: String,
    message: Message,
}

#[derive(Debug, Serialize)]
struct DeleteResponse {
    mailbox: String,
    email: String,
    deleted: bool,
}

#[derive(Debug, Serialize)]
struct ClearResponse {
    mailbox: String,
    email: String,
    removed: usize,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .route("/api/messages", get(list_by_email))
        .route("/api/messages/{id}", get(get_by_email))
        .route(
            "/api/mailboxes/{mailbox}/messages",
            get(list_by_mailbox).delete(clear_mailbox),
        )
        .route(
            "/api/mailboxes/{mailbox}/messages/{id}",
            get(get_by_mailbox).delete(delete_by_mailbox),
        )
        .route(
            "/api/mailboxes/{mailbox}/events/next",
            get(next_mailbox_event),
        )
        .fallback(get(serve_embedded_static))
        .with_state(state)
}

async fn serve_embedded_static(uri: Uri) -> Response {
    let normalized_path = normalize_static_path(uri.path());
    let candidates = if normalized_path.is_empty() {
        vec!["index.html".to_string()]
    } else if normalized_path.ends_with('/') {
        vec![
            format!("{}index.html", normalized_path),
            normalized_path.trim_end_matches('/').to_string(),
        ]
    } else {
        vec![
            normalized_path.clone(),
            format!("{}/index.html", normalized_path),
        ]
    };

    for candidate in candidates {
        if let Some(file) = EMBEDDED_PUBLIC_DIR.get_file(&candidate) {
            let mime = mime_guess::from_path(&candidate)
                .first_or_octet_stream()
                .essence_str()
                .to_string();
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime)],
                file.contents().to_vec(),
            )
                .into_response();
        }
    }

    (
        StatusCode::NOT_FOUND,
        Json(HashMap::from([(
            "error",
            format!("static file not found: {}", uri.path()),
        )])),
    )
        .into_response()
}

fn normalize_static_path(path: &str) -> String {
    if path.trim().is_empty() {
        return String::new();
    }

    path.trim()
        .trim_start_matches('/')
        .split('/')
        .filter(|seg| !seg.is_empty() && *seg != "." && *seg != "..")
        .collect::<Vec<_>>()
        .join("/")
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn list_by_email(
    State(state): State<AppState>,
    Query(query): Query<EmailQuery>,
) -> Result<Json<ListResponse>, ApiError> {
    let email_input = query
        .email
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("missing email query parameter"))?;

    write_message_list(&state, email_input).await
}

async fn get_by_email(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<EmailQuery>,
) -> Result<Json<DetailResponse>, ApiError> {
    let email_input = query
        .email
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| ApiError::bad_request("missing email query parameter"))?;

    write_message_detail(&state, email_input, &id).await
}

async fn list_by_mailbox(
    State(state): State<AppState>,
    Path(mailbox): Path<String>,
) -> Result<Json<ListResponse>, ApiError> {
    write_message_list(&state, &mailbox).await
}

async fn get_by_mailbox(
    State(state): State<AppState>,
    Path((mailbox, id)): Path<(String, String)>,
) -> Result<Json<DetailResponse>, ApiError> {
    write_message_detail(&state, &mailbox, &id).await
}

async fn delete_by_mailbox(
    State(state): State<AppState>,
    Path((mailbox, id)): Path<(String, String)>,
) -> Result<Json<DeleteResponse>, ApiError> {
    let (mailbox, email) =
        address::normalize_mailbox(&mailbox, &state.cfg.domain).map_err(ApiError::bad_request)?;

    let message_id = id.trim();
    if message_id.is_empty() {
        return Err(ApiError::bad_request("missing message id"));
    }

    let deleted = state.store.delete(&mailbox, message_id).await;
    Ok(Json(DeleteResponse {
        mailbox,
        email,
        deleted,
    }))
}

async fn clear_mailbox(
    State(state): State<AppState>,
    Path(mailbox): Path<String>,
) -> Result<Json<ClearResponse>, ApiError> {
    let (mailbox, email) =
        address::normalize_mailbox(&mailbox, &state.cfg.domain).map_err(ApiError::bad_request)?;
    let removed = state.store.clear(&mailbox).await;

    Ok(Json(ClearResponse {
        mailbox,
        email,
        removed,
    }))
}

async fn next_mailbox_event(
    State(state): State<AppState>,
    Path(mailbox): Path<String>,
) -> Result<Response, ApiError> {
    let (mailbox, _) =
        address::normalize_mailbox(&mailbox, &state.cfg.domain).map_err(ApiError::bad_request)?;

    let mut receiver = state.store.subscribe();
    loop {
        match timeout(Duration::from_secs(25), receiver.recv()).await {
            Ok(Ok(event)) => {
                if event.mailbox == mailbox {
                    return Ok((StatusCode::OK, Json(event)).into_response());
                }
            }
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                return Err(ApiError::service_unavailable("event stream closed"));
            }
            Err(_) => return Ok(StatusCode::NO_CONTENT.into_response()),
        }
    }
}

async fn write_message_list(
    state: &AppState,
    mailbox_input: &str,
) -> Result<Json<ListResponse>, ApiError> {
    let (mailbox, email) = address::normalize_mailbox(mailbox_input, &state.cfg.domain)
        .map_err(ApiError::bad_request)?;

    let messages = state.store.list(&mailbox).await;
    let summaries = messages
        .iter()
        .map(|item| item.summary())
        .collect::<Vec<_>>();

    Ok(Json(ListResponse {
        mailbox,
        email,
        count: summaries.len(),
        messages: summaries,
    }))
}

async fn write_message_detail(
    state: &AppState,
    mailbox_input: &str,
    message_id: &str,
) -> Result<Json<DetailResponse>, ApiError> {
    let (mailbox, email) = address::normalize_mailbox(mailbox_input, &state.cfg.domain)
        .map_err(ApiError::bad_request)?;

    let message_id = message_id.trim();
    if message_id.is_empty() {
        return Err(ApiError::bad_request("missing message id"));
    }

    let message = state
        .store
        .get(&mailbox, message_id)
        .await
        .ok_or_else(|| ApiError::not_found("message not found"))?;

    Ok(Json(DetailResponse {
        mailbox,
        email,
        message,
    }))
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }

    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(HashMap::from([("error", self.message)]));
        (self.status, body).into_response()
    }
}
