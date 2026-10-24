use crate::db::DbClient;
use moka::future::Cache;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone)]
pub struct AppState {
    pub db_pool: PgPool,
    pub db_client: DbClient,
    pub config: crate::config::Config,
    pub google_keys_cache: moka::future::Cache<String, String>,
}

impl AppState {
    pub fn new(db_pool: PgPool, config: crate::config::Config) -> Self {
        let cache = Cache::builder()
            .max_capacity(1)
            .time_to_live(Duration::from_secs(3600 * 24))
            .build();
        Self {
            db_pool: db_pool.clone(),
            db_client: DbClient::new(db_pool),
            config,
            google_keys_cache: cache,
        }
    }
}

pub type SharedState = Arc<AppState>;
