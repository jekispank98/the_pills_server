use crate::handlers::{auth::auth_router /*users::users_handler*/};
use crate::state::SharedState;
use axum::routing::Router;
use axum::Extension;

pub fn create_routes(app_state: SharedState) -> Router {
    let api_route = Router::new()
        .nest("/auth", auth_router())
        .layer(Extension(app_state.clone()));

    Router::new().merge(api_route)
}
