use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Intake {
    pub id: Uuid,
    pub user_account_id: Uuid,
    pub medication_id: Uuid,
    pub time: NaiveDateTime,
    pub pre_time: Option<NaiveDateTime>,
    pub is_completed: bool,
    pub is_passed: bool,
    pub is_canceled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub version: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeStatus {
    Passed,
    Canceled,
    Completed,
    Uncompleted,
}
