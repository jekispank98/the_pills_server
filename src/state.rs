use crate::db::DbClient;
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub db_client: DbClient,
    pub config: crate::config::Config,
}

impl AppState {
    pub fn new(db_pool: PgPool, config: crate::config::Config) -> Self {
        Self {
            db_pool: db_pool.clone(),
            db_client: DbClient::new(db_pool),
            config,
        }
    }
}

pub type SharedState = Arc<AppState>;
