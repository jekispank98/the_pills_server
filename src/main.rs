use pills_server_test::config::Config;
use pills_server_test::routes::create_routes;
use pills_server_test::state::{AppState, SharedState};
use sqlx::PgPool;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let config = Config::from_env().expect("invalid configuration");
    let listen_addr = config.listen_addr;

    let db_pool = PgPool::connect(&config.database_url)
        .await
        .expect("failed to connect to Postgres");

    let app_state = SharedState::new(AppState::new(db_pool, config));
    let app = create_routes(app_state);

    let listener = TcpListener::bind(listen_addr)
        .await
        .expect("failed to bind listen address");

    tracing::info!(%listen_addr, "starting server");
    axum::serve(listener, app).await.expect("server error");
}
