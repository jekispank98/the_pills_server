use crate::handlers::auth::auth_router;
use crate::handlers::intakes::intakes_router;
use crate::handlers::medications::medications_router;
use crate::handlers::persons::persons_router;
use crate::handlers::users::users_router;
use crate::state::SharedState;
use axum::routing::Router;
use axum::Extension;
use tower_http::trace::{DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse, TraceLayer};
use tracing::Level;

pub fn create_routes(app_state: SharedState) -> Router {
    let api_route = Router::new()
        .nest("/auth", auth_router())
        .nest("/users", users_router())
        .nest("/persons", persons_router())
        .nest("/medications", medications_router())
        .nest("/intakes", intakes_router())
        .layer(Extension(app_state.clone()))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(Level::INFO))
                .on_request(DefaultOnRequest::new().level(Level::INFO))
                .on_response(DefaultOnResponse::new().level(Level::INFO)),
        );

    Router::new().merge(api_route)
}
