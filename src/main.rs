use pills_server_test::config::Config;
use pills_server_test::migrations;
use pills_server_test::routes::create_routes;
use pills_server_test::state::{AppState, SharedState};
use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    init_logging();

    let config = Config::from_env().expect("invalid configuration");
    let listen_addr = config.listen_addr;

    let db_pool = PgPoolOptions::new()
        .max_connections(config.database_max_connections)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&config.database_url)
        .await
        .expect("failed to connect to Postgres");

    migrations::run(&db_pool, config.migrations_baseline)
        .await
        .expect("failed to apply database migrations");

    let app_state = SharedState::new(AppState::new(db_pool, config));
    let app = create_routes(app_state);

    let listener = TcpListener::bind(listen_addr)
        .await
        .expect("failed to bind listen address");

    tracing::info!(%listen_addr, "starting server");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
    tracing::info!("server stopped");
}

/// JSON lines when LOG_FORMAT=json, or by default with APP_ENV=production;
/// human-readable text otherwise.
fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let json = match std::env::var("LOG_FORMAT") {
        Ok(format) => format.eq_ignore_ascii_case("json"),
        Err(_) => matches!(
            std::env::var("APP_ENV").as_deref(),
            Ok("production") | Ok("prod")
        ),
    };

    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder
            .json()
            .flatten_event(true)
            .with_current_span(true)
            .with_span_list(false)
            .init();
    } else {
        builder.init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to listen for Ctrl+C");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to listen for SIGTERM")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received, finishing in-flight requests");
}
