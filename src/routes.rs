use crate::handlers::auth::auth_router;
use crate::handlers::health::health_router;
use crate::handlers::intakes::intakes_router;
use crate::handlers::medications::medications_router;
use crate::handlers::persons::persons_router;
use crate::handlers::users::users_router;
use crate::rate_limit::RateLimiter;
use crate::state::SharedState;
use axum::Extension;
use axum::extract::{DefaultBodyLimit, MatchedPath, Request};
use axum::http::StatusCode;
use axum::routing::Router;
use std::time::Duration;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 1024 * 1024;

/// Logs the path without the query string: `/auth/verify?token=...` must not
/// put the verification token into the logs. `route` is the matched route
/// template; `user_id` is filled in by the `CurrentUser` extractor.
fn request_span(request: &Request) -> tracing::Span {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(MatchedPath::as_str);
    tracing::info_span!(
        "request",
        method = %request.method(),
        path = %request.uri().path(),
        route,
        user_id = tracing::field::Empty,
    )
}

pub fn create_routes(app_state: SharedState) -> Router {
    let api_route = Router::new()
        .nest(
            "/auth",
            auth_router(RateLimiter::new(
                app_state.config.auth_rate_limit_per_minute,
                Duration::from_secs(60),
            )),
        )
        .nest("/users", users_router())
        .nest("/persons", persons_router())
        .nest("/medications", medications_router())
        .nest("/intakes", intakes_router())
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(request_span)
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
    use axum::extract::connect_info::MockConnectInfo;
    use axum::http::{Method, Request};
    use sqlx::postgres::PgPoolOptions;
    use std::net::SocketAddr;
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
        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
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

    #[tokio::test]
    async fn login_is_rate_limited_per_client() {
        let app = app_without_database()
            .layer(MockConnectInfo(SocketAddr::from(([203, 0, 113, 1], 40000))));
        let limit = Config::for_tests().auth_rate_limit_per_minute;
        let login = || {
            Request::builder()
                .method(Method::POST)
                .uri("/auth/login")
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap()
        };

        for _ in 0..limit {
            let response = app.clone().oneshot(login()).await.unwrap();
            assert_ne!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        }
        let response = app.clone().oneshot(login()).await.unwrap();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers()["retry-after"], "60");

        let health = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();
        assert_ne!(
            app.oneshot(health).await.unwrap().status(),
            StatusCode::TOO_MANY_REQUESTS
        );
    }
}
