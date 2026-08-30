use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

/// Значения хранятся в БД как TEXT (см. CHECK-constraint в миграции), а не как
/// нативный Postgres ENUM — типы легче переименовать/добавить значение, и это
/// совпадает с тем, как Android хранит `PersonRelation`/`Gender` (`.name` в Room).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum PersonRelation {
    /// `Self` — зарезервированное слово в Rust, поэтому `Myself` + явный rename.
    #[sqlx(rename = "SELF")]
    #[serde(rename = "SELF")]
    Myself,
    Child,
    Parent,
    Spouse,
    Other,
}

impl Default for PersonRelation {
    fn default() -> Self {
        PersonRelation::Other
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
pub enum Gender {
    Male,
    Female,
    Unspecified,
}

impl Default for Gender {
    fn default() -> Self {
        Gender::Unspecified
    }
}

/// Модель строки таблицы `persons`. Полностью соответствует Android
/// `domain.models.Person` (см. pills_models_and_methods.md) — поля и enum-имена
/// (`SELF`/`CHILD`/... , `MALE`/`FEMALE`/`UNSPECIFIED`) специально не переименованы,
/// чтобы DTO мапились без конвертации на клиенте.
#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Person {
    /// Задаётся клиентом при создании (offline-first) и не меняется после синка.
    pub id: Uuid,
    pub user_account_id: Uuid,
    pub name: String,
    pub relation: PersonRelation,
    pub is_user: bool,
    pub birth_date: Option<NaiveDateTime>,
    pub gender: Gender,
    pub weight_kg: Option<f64>,
    pub color_tag: Option<i64>,
    pub notes: Option<String>,
    pub is_active: bool,
    pub photo_url: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Значения должны совпадать с Android enum'ами `PersonRelation`/`Gender`
    // 1-в-1 (см. pills_models_and_methods.md) — иначе DTO с бэкенда не
    // распарсится клиентом.
    #[test]
    fn person_relation_matches_android_json_names() {
        let cases = [
            (PersonRelation::Myself, "\"SELF\""),
            (PersonRelation::Child, "\"CHILD\""),
            (PersonRelation::Parent, "\"PARENT\""),
            (PersonRelation::Spouse, "\"SPOUSE\""),
            (PersonRelation::Other, "\"OTHER\""),
        ];
        for (value, expected_json) in cases {
            assert_eq!(serde_json::to_string(&value).unwrap(), expected_json);
            assert_eq!(
                serde_json::from_str::<PersonRelation>(expected_json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn gender_matches_android_json_names() {
        let cases = [
            (Gender::Male, "\"MALE\""),
            (Gender::Female, "\"FEMALE\""),
            (Gender::Unspecified, "\"UNSPECIFIED\""),
        ];
        for (value, expected_json) in cases {
            assert_eq!(serde_json::to_string(&value).unwrap(), expected_json);
            assert_eq!(serde_json::from_str::<Gender>(expected_json).unwrap(), value);
        }
    }

    #[test]
    fn defaults_match_android_defaults() {
        // Person.kt: relation = PersonRelation.OTHER, gender = Gender.UNSPECIFIED
        assert_eq!(PersonRelation::default(), PersonRelation::Other);
        assert_eq!(Gender::default(), Gender::Unspecified);
    }
}
