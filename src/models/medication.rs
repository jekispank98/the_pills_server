use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Фиксированный каталог форм выпуска — зеркалит Android `PillsForm.id`
/// (util/Constants.kt: "pellets"/"liquid"/"injection"/"powder"/"drops", каждая
/// со своей единицей измерения дозы на клиенте). Значения — lowercase, 1-в-1
/// с `PillsForm.id`, а не UPPERCASE как у `PersonRelation`/`Gender`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum MedicationForm {
    Pellets,
    Liquid,
    Injection,
    Powder,
    Drops,
}

/// Модель строки таблицы `medications`. Соответствует Android
/// `domain.models.Medication_new`/`MedicationEntity_new`. `color_tag` остаётся
/// свободной hex-строкой (Android `MedicationColor.hex`), не enum: Android сам
/// мягко обрабатывает незнакомый hex (fallback на `PEARL_GRAY`), сервер не
/// должен быть строже клиента на этот счёт.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Medication {
    /// Задаётся клиентом при создании (offline-first) и не меняется после синка.
    pub id: Uuid,
    pub user_account_id: Uuid,
    pub name: String,
    pub dosage: f64,
    pub form: MedicationForm,
    pub instruction: String,
    pub color_tag: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// Оптимистичная конкуренция (OCC), тот же паттерн, что у `Person::version`.
    pub version: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Значения должны совпадать с Android `PillsForm.id` 1-в-1 — иначе DTO
    // с бэкенда не распарсится клиентом.
    #[test]
    fn medication_form_matches_android_pills_form_ids() {
        let cases = [
            (MedicationForm::Pellets, "\"pellets\""),
            (MedicationForm::Liquid, "\"liquid\""),
            (MedicationForm::Injection, "\"injection\""),
            (MedicationForm::Powder, "\"powder\""),
            (MedicationForm::Drops, "\"drops\""),
        ];
        for (value, expected_json) in cases {
            assert_eq!(serde_json::to_string(&value).unwrap(), expected_json);
            assert_eq!(
                serde_json::from_str::<MedicationForm>(expected_json).unwrap(),
                value
            );
        }
    }
}
