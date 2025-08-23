use chrono::{NaiveDate, NaiveDateTime};
use serde::Serialize;
use sqlx::FromRow;

#[derive(Serialize, FromRow, Debug)]
pub struct User {
    pub id: String,
    pub login: String,
    pub password: String,
    pub name: String,
    pub token_expires_at: NaiveDateTime,
    pub subscribed: bool
}