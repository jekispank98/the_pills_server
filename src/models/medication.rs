use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

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

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Medication {
    pub id: Uuid,
    pub user_account_id: Uuid,
    pub name: String,
    pub dosage: f64,
    pub form: MedicationForm,
    pub instruction: String,
    pub color_tag: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub version: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

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
