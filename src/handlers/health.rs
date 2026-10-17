use crate::state::SharedState;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Json, Router};
use serde_json::json;

pub fn health_router() -> Router {
    Router::new().route("/", get(health))
}

async fn health(Extension(state): Extension<SharedState>) -> impl IntoResponse {
    match sqlx::query("SELECT 1").execute(&state.db_pool).await {
        Ok(_) => (StatusCode::OK, Json(json!({ "status": "ok" }))),
        Err(error) => {
            tracing::error!(error = %error, "health check: database unavailable");
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "unavailable" })),
            )
        }
    }
}
