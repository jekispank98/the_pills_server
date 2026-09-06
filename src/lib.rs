// Экспортирует модули как библиотеку — нужно, чтобы integration-тесты в
// tests/ (например tests/persons_version_conflict.rs) могли собрать AppState/
// DbClient напрямую, не поднимая отдельный бинарь. main.rs использует эти же
// модули через этот крейт вместо собственных `mod` объявлений.
pub mod config;
pub mod db;
pub mod dtos;
pub mod error;
pub mod extractors;
pub mod handlers;
pub mod mail;
pub mod models;
pub mod routes;
pub mod state;
pub mod utils;

/// Placeholder-хендлер для ещё не реализованных маршрутов (`/google_register`,
/// `/refresh_token`, см. handlers/auth.rs) — жил в main.rs, переехал сюда вместе
/// с остальными модулями, т.к. main.rs больше не крейт-рут для них.
pub async fn root() -> &'static str {
    "Hello World!"
}
