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
    // `tower-http` с фичей "trace" был объявлен в Cargo.toml, но нигде не
    // подключён — ни один запрос не оставлял следа в консоли. По умолчанию
    // TraceLayer пишет на DEBUG (тише, чем "info"-фильтр из main.rs), поэтому
    // уровень явно поднят до INFO — иначе не увидите ничего без RUST_LOG.
    // Тело запроса/ответа сюда намеренно не входит (в отличие от Android
    // LogLevel.ALL) — body/register и /login несёт пароль в открытом виде,
    // светить его в серверных логах не стоит даже в dev.
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
