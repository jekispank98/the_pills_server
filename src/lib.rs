pub mod config;
pub mod db;
pub mod dtos;
pub mod error;
pub mod extractors;
pub mod handlers;
pub mod mail;
pub mod migrations;
pub mod models;
pub mod routes;
pub mod state;
pub mod utils;

pub async fn root() -> &'static str {
    "Hello World!"
}
