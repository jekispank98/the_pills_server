mod config;
mod db;
mod dtos;
mod error;
mod handlers;
mod models;
mod routes;
mod state;
mod utils;
mod mail;

use crate::routes::create_routes;
use chrono::NaiveDateTime;
use config::Config;
use dotenv;
use models::user::User;
use serde::Deserialize;
use sqlx::PgPool;
use state::{AppState, SharedState};
use std::env;
use tokio::net::TcpListener;

/*#[derive(Deserialize)]
struct UserRequest {
    id: String,
    login: String,
    password: String,
    name: String,
    token_expires_at: NaiveDateTime,
    verified: bool,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
    verification_token: String,
    subscribed: bool
}*/

async fn root() -> &'static str {
    "Hello World!"
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    let database_url = env::var("DATABASE_URL").expect("Database url is not set in env file");

    let db_pool = PgPool::connect(&database_url)
        .await
        .expect("Failed to connect to Postgres");

    let config = Config::init();

    let app_state = SharedState::new(AppState::new(db_pool, config));

    let app = create_routes(app_state);

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();

    axum::serve(listener, app).await.unwrap()
}
