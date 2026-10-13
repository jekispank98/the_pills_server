use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

/// Модель строки таблицы `intakes`. Соответствует Android
/// `domain.models.Intake`/`IntakeEntity` — на клиенте `type`/`person` встроены
/// как полные объекты, здесь остаются нормализованными (`medication_id` — FK,
/// persons — отдельный join через `intake_person_cross_ref`, собираются на
/// чтении — см. `IntakeDto` в dtos.rs).
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Intake {
    /// Задаётся клиентом при создании (offline-first) и не меняется после синка.
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
    /// Оптимистичная конкуренция (OCC), тот же паттерн, что у `Person::version`.
    pub version: i32,
}

/// Переход статуса приёма — зеркалит четыре DAO-метода Android `IntakeDao`
/// (markAsPassed/markAsCanceled/markAsCompleted/markAsUncompleted), включая их
/// неочевидную асимметрию: `Passed` трогает только `is_passed`, остальные три
/// поля явно переустанавливают все три флага (см. `set_intake_status` в db.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntakeStatus {
    Passed,
    Canceled,
    Completed,
    Uncompleted,
}
