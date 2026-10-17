use crate::handlers::auth::auth_router;
use crate::handlers::health::health_router;
use crate::handlers::intakes::intakes_router;
use crate::handlers::medications::medications_router;
use crate::handlers::persons::persons_router;
use crate::handlers::users::users_router;
use crate::state::SharedState;
use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::routing::Router;
use axum::Extension;
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 1024 * 1024;

pub fn create_routes(app_state: SharedState) -> Router {
    let api_route = Router::new()
        .nest("/auth", auth_router())
        .nest("/users", users_router())
        .nest("/persons", persons_router())
        .nest("/medications", medications_router())
        .nest("/intakes", intakes_router())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    Router::new()
        .nest("/health", health_router())
        .merge(api_route)
        .layer(Extension(app_state))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::SERVICE_UNAVAILABLE,
            REQUEST_TIMEOUT,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::AppState;
    use axum::body::Body;
    use axum::http::{Method, Request};
    use sqlx::postgres::PgPoolOptions;
    use std::sync::Arc;
    use tower::util::ServiceExt;

    fn app_without_database() -> Router {
        let pool = PgPoolOptions::new()
            .acquire_timeout(Duration::from_millis(200))
            .connect_lazy("postgres://nobody:nothing@127.0.0.1:1/none")
            .unwrap();
        create_routes(Arc::new(AppState::new(pool, Config::for_tests())))
    }

    #[tokio::test]
    async fn health_reports_unavailable_database() {
        let request = Request::builder().uri("/health").body(Body::empty()).unwrap();
        let response = app_without_database().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn oversized_body_is_rejected() {
        let request = Request::builder()
            .method(Method::POST)
            .uri("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(vec![b' '; MAX_BODY_BYTES + 1]))
            .unwrap();
        let response = app_without_database().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }
}
