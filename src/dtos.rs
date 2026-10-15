use crate::models::intake::Intake;
use crate::models::medication::{Medication, MedicationForm};
use crate::models::person::{Gender, Person, PersonRelation};
use crate::models::user::User;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterUserDto {
    #[validate(required(message = "Email is required"))]
    #[validate(length(min = 3, max = 50, message = "Email must be 3-50 characters"))]
    pub email: Option<String>,

    #[validate(required(message = "Password is required"))]
    #[validate(length(min = 6, message = "Password must be at least 6 characters"))]
    pub password: Option<String>,

    #[validate(required(message = "Name is required"))]
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginUserDto {
    #[validate(required(message = "Email is required"))]
    #[validate(length(
        min = 3,
        max = 50,
        message = "Email must be between 3 and 50 characters"
    ))]
    pub email: Option<String>,

    #[validate(required(message = "Password is required"))]
    #[validate(length(min = 6, message = "Password must be at least 6 characters"))]
    pub password: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct GoogleLoginDto {
    #[validate(required(message = "Google ID token is required"))]
    #[validate(length(min = 10, message = "Invalid Google ID token"))]
    pub id_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponseDto {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub token: String,
    pub refresh_token: String,
    pub subscribed: bool,
}

#[derive(Debug, Deserialize, Validate)]
pub struct RefreshTokenRequestDto {
    #[validate(required(message = "Refresh token is required"))]
    #[validate(length(min = 1, message = "Refresh token is required"))]
    pub refresh_token: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RefreshTokenResponseDto {
    pub token: String,
    pub refresh_token: String,
}

#[derive(Serialize, Deserialize)]
pub struct Response {
    pub status: &'static str,
    pub message: String,
}

#[derive(Serialize, Deserialize, Validate)]
pub struct VerifyEmailQueryDto {
    #[validate(length(min = 1, message = "Token is required."),)]
    pub token: String,
}

#[derive(Debug, Serialize)]
pub struct UserDto {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub photo_url: Option<String>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
    pub is_email_verified: bool,
    pub subscribed: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: Option<NaiveDateTime>,
}

impl From<User> for UserDto {
    fn from(user: User) -> Self {
        UserDto {
            id: user.id,
            email: user.email,
            name: user.name,
            photo_url: user.photo_url,
            time_zone: user.time_zone,
            locale: user.locale,
            is_email_verified: user.verified,
            subscribed: user.subscribed,
            created_at: user.created_at,
            updated_at: user.updated_at,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpdateUserRequest {
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: Option<String>,
    pub photo_url: Option<String>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PersonDto {
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

impl From<Person> for PersonDto {
    fn from(person: Person) -> Self {
        PersonDto {
            id: person.id,
            user_account_id: person.user_account_id,
            name: person.name,
            relation: person.relation,
            is_user: person.is_user,
            birth_date: person.birth_date,
            gender: person.gender,
            weight_kg: person.weight_kg,
            color_tag: person.color_tag,
            notes: person.notes,
            is_active: person.is_active,
            photo_url: person.photo_url,
            created_at: person.created_at,
            updated_at: person.updated_at,
            version: person.version,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertPersonRequest {
    #[validate(required(message = "Name is required"))]
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: Option<String>,
    #[serde(default)]
    pub relation: PersonRelation,
    pub birth_date: Option<NaiveDateTime>,
    #[serde(default)]
    pub gender: Gender,
    pub weight_kg: Option<f64>,
    pub color_tag: Option<i64>,
    pub notes: Option<String>,
    #[serde(default = "default_true")]
    pub is_active: bool,
    pub photo_url: Option<String>,
    #[serde(default)]
    pub base_version: i32,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Serialize)]
pub struct MedicationDto {
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub person_ids: Option<Vec<Uuid>>,
}

impl MedicationDto {
    pub fn with_person_ids(medication: Medication, person_ids: Vec<Uuid>) -> Self {
        MedicationDto {
            person_ids: Some(person_ids),
            ..MedicationDto::from(medication)
        }
    }
}

impl From<Medication> for MedicationDto {
    fn from(medication: Medication) -> Self {
        MedicationDto {
            id: medication.id,
            user_account_id: medication.user_account_id,
            name: medication.name,
            dosage: medication.dosage,
            form: medication.form,
            instruction: medication.instruction,
            color_tag: medication.color_tag,
            created_at: medication.created_at,
            updated_at: medication.updated_at,
            version: medication.version,
            person_ids: None,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertMedicationRequest {
    #[validate(required(message = "Name is required"))]
    #[validate(length(min = 1, max = 200, message = "Name must be 1-200 characters"))]
    pub name: Option<String>,
    #[validate(required(message = "Dosage is required"))]
    pub dosage: Option<f64>,
    pub form: MedicationForm,
    #[serde(default)]
    pub instruction: String,
    #[validate(required(message = "Color tag is required"))]
    pub color_tag: Option<String>,
    #[serde(default)]
    pub base_version: i32,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LinkPersonsRequest {
    #[validate(length(min = 1, message = "At least one person id is required"))]
    pub person_ids: Vec<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct IntakeDto {
    pub id: Uuid,
    pub medication_id: Uuid,
    pub time: NaiveDateTime,
    pub pre_time: Option<NaiveDateTime>,
    pub person_ids: Vec<Uuid>,
    pub is_completed: bool,
    pub is_passed: bool,
    pub is_canceled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub version: i32,
}

impl IntakeDto {
    pub fn from_intake(intake: Intake, person_ids: Vec<Uuid>) -> Self {
        IntakeDto {
            id: intake.id,
            medication_id: intake.medication_id,
            time: intake.time,
            pre_time: intake.pre_time,
            person_ids,
            is_completed: intake.is_completed,
            is_passed: intake.is_passed,
            is_canceled: intake.is_canceled,
            created_at: intake.created_at,
            updated_at: intake.updated_at,
            version: intake.version,
        }
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct UpsertIntakeRequest {
    #[validate(required(message = "Medication id is required"))]
    pub medication_id: Option<Uuid>,
    #[validate(required(message = "Time is required"))]
    pub time: Option<NaiveDateTime>,
    pub pre_time: Option<NaiveDateTime>,
    #[validate(length(min = 1, message = "At least one person id is required"))]
    pub person_ids: Vec<Uuid>,
    #[serde(default)]
    pub is_completed: bool,
    #[serde(default)]
    pub is_passed: bool,
    #[serde(default)]
    pub is_canceled: bool,
    #[serde(default)]
    pub base_version: i32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IntakeStatusRequest {
    Passed,
    Canceled,
    Completed,
    Uncompleted,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_person_request_defaults_match_the_domain_model_defaults() {
        let request: UpsertPersonRequest =
            serde_json::from_str(r#"{"name":"Mom"}"#).unwrap();

        assert_eq!(request.relation, PersonRelation::Other);
        assert_eq!(request.gender, Gender::Unspecified);
        assert!(request.is_active);
        assert_eq!(request.name.as_deref(), Some("Mom"));
        assert_eq!(request.base_version, 0);
    }

    #[test]
    fn upsert_person_request_rejects_missing_name() {
        let request: UpsertPersonRequest = serde_json::from_str(r#"{}"#).unwrap();
        assert!(request.validate().is_err());
    }

    #[test]
    fn upsert_person_request_rejects_overlong_name() {
        let body = format!(r#"{{"name":"{}"}}"#, "x".repeat(101));
        let request: UpsertPersonRequest = serde_json::from_str(&body).unwrap();
        assert!(request.validate().is_err());
    }

    #[test]
    fn user_dto_never_carries_password_or_verification_token() {
        let user = User {
            id: Uuid::nil(),
            email: "a@b.c".into(),
            password: Some("$argon2id$secret".into()),
            name: "A".into(),
            verification_token_expires_at: None,
            verified: true,
            created_at: NaiveDateTime::default(),
            updated_at: None,
            verification_token: Some("secret-token".into()),
            subscribed: false,
            google_sub: None,
            photo_url: None,
            time_zone: None,
            locale: None,
        };

        let json = serde_json::to_string(&UserDto::from(user)).unwrap();
        assert!(!json.contains("argon2id"));
        assert!(!json.contains("secret-token"));
        assert!(!json.contains("password"));
    }

    #[test]
    fn upsert_medication_request_defaults_match_the_domain_model_defaults() {
        let request: UpsertMedicationRequest = serde_json::from_str(
            r##"{"name":"Aspirin","dosage":100.0,"form":"pellets","color_tag":"#6A94E8"}"##,
        )
        .unwrap();

        assert_eq!(request.instruction, "");
        assert_eq!(request.base_version, 0);
        assert_eq!(request.name.as_deref(), Some("Aspirin"));
    }

    #[test]
    fn upsert_medication_request_rejects_missing_name() {
        let request: UpsertMedicationRequest =
            serde_json::from_str(r##"{"dosage":1.0,"form":"pellets","color_tag":"#000"}"##)
                .unwrap();
        assert!(request.validate().is_err());
    }

    #[test]
    fn medication_form_rejects_unknown_values() {
        let result: Result<UpsertMedicationRequest, _> = serde_json::from_str(
            r##"{"name":"X","dosage":1.0,"form":"syrup","color_tag":"#000"}"##,
        );
        assert!(result.is_err());
    }

    #[test]
    fn upsert_intake_request_rejects_an_empty_person_list() {
        let request: UpsertIntakeRequest = serde_json::from_str(&format!(
            r#"{{"medication_id":"{}","time":"2026-01-01T00:00:00","person_ids":[]}}"#,
            Uuid::nil()
        ))
        .unwrap();
        assert!(request.validate().is_err());
    }

    #[test]
    fn intake_status_request_matches_the_four_android_dao_transitions() {
        for (json, expected) in [
            ("\"PASSED\"", IntakeStatusRequest::Passed),
            ("\"CANCELED\"", IntakeStatusRequest::Canceled),
            ("\"COMPLETED\"", IntakeStatusRequest::Completed),
            ("\"UNCOMPLETED\"", IntakeStatusRequest::Uncompleted),
        ] {
            let parsed: IntakeStatusRequest = serde_json::from_str(json).unwrap();
            assert_eq!(
                std::mem::discriminant(&parsed),
                std::mem::discriminant(&expected)
            );
        }
    }
}
