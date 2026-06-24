use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use axum::{Json, Router};
use meshlet_proto::messages::{SyncRequest, SyncResponse};
use tokio::sync::Mutex;

use crate::doc::ServerDoc;

pub struct AppState {
    pub doc: Mutex<ServerDoc>,
    pub token: Option<String>,
    pub data_dir: PathBuf,
}

pub fn app_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/sync", post(sync_handler))
        .with_state(state)
}

pub async fn sync_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(request): Json<SyncRequest>,
) -> impl IntoResponse {
    if let Some(ref expected_token) = state.token {
        let auth_header = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let token = auth_header.strip_prefix("Bearer ").unwrap_or("");

        if token != expected_token {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({"error": "unauthorized"})),
            )
                .into_response();
        }
    }

    let doc = state.doc.lock().await;

    let client_updates = match SyncRequest::updates(&request) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("base64 decode failed: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid base64 encoding"})),
            )
                .into_response();
        }
    };
    let client_vv = match SyncRequest::client_vv(&request) {
        Some(vv) => vv,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "invalid client_vv"})),
            )
                .into_response();
        }
    };

    if !client_updates.is_empty() {
        if let Err(e) = doc.import(&client_updates) {
            tracing::error!("import failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "import failed"})),
            )
                .into_response();
        }
    }

    let server_updates = match doc.export_updates_since(&client_vv) {
        Ok(u) => u,
        Err(e) => {
            tracing::error!("export failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "export failed"})),
            )
                .into_response();
        }
    };

    let server_vv = doc.oplog_vv();

    if let Err(e) = doc.save(&state.data_dir) {
        tracing::error!("save failed: {}", e);
    }

    let response = SyncResponse::new(&server_vv, &server_updates);
    (StatusCode::OK, Json(response)).into_response()
}