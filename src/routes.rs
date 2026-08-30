use crate::handlers::auth::auth_router;
use crate::handlers::persons::persons_router;
use crate::handlers::users::users_router;
use crate::state::SharedState;
use axum::routing::Router;
use axum::Extension;

pub fn create_routes(app_state: SharedState) -> Router {
    let api_route = Router::new()
        .nest("/auth", auth_router())
        .nest("/users", users_router())
        .nest("/persons", persons_router())
        .layer(Extension(app_state.clone()));

    Router::new().merge(api_route)
}
