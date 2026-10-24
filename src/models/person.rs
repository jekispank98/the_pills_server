use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
#[derive(Default)]
pub enum PersonRelation {
    #[sqlx(rename = "SELF")]
    #[serde(rename = "SELF")]
    Myself,
    Child,
    Parent,
    Spouse,
    #[default]
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "UPPERCASE")]
#[serde(rename_all = "UPPERCASE")]
#[derive(Default)]
pub enum Gender {
    Male,
    Female,
    #[default]
    Unspecified,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Person {
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
    pub version: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

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
            assert_eq!(
                serde_json::from_str::<Gender>(expected_json).unwrap(),
                value
            );
        }
    }

    #[test]
    fn defaults_match_android_defaults() {
        assert_eq!(PersonRelation::default(), PersonRelation::Other);
        assert_eq!(Gender::default(), Gender::Unspecified);
    }
}
