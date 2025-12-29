use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Serialize, FromRow, Debug)]
pub struct User {
    pub id: i32,
    pub email: String,
    pub password: String,
    pub name: String,
    pub token_expires_at: Option<NaiveDateTime>,
    pub verified: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: Option<NaiveDateTime>,
    pub verification_token: Option<String>,
    pub subscribed: bool
}