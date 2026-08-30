mod config;
mod db;
mod dtos;
mod error;
mod extractors;
mod handlers;
mod models;
mod routes;
mod state;
mod utils;
mod mail;

use crate::routes::create_routes;
use config::Config;
use sqlx::PgPool;
use state::{AppState, SharedState};
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

async fn root() -> &'static str {
    "Hello World!"
}

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
