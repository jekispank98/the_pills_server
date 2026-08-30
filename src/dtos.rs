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
    pub subscribed: bool,
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

/// Профиль текущего пользователя. Пароль и токен верификации сюда не входят —
/// `User` и не реализует `Serialize` именно чтобы такую утечку нельзя было
/// сделать по ошибке (см. models/user.rs).
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

/// `PATCH /users/me`. Все поля опциональны и означают частичное обновление —
/// отсутствующее поле остаётся как было (см. `UserProfileUpdate`/`COALESCE` в db.rs).
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
        }
    }
}

/// `PUT /persons/{id}` — идемпотентный upsert по client-generated `id`.
/// `is_user`/`relation=SELF` сюда не входят: SELF-профиль заводится только при
/// регистрации (см. `register_user_with_self_person`), апсерт его не создаёт и не меняет.
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
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_person_request_defaults_match_the_domain_model_defaults() {
        // Person.kt: relation = OTHER, gender = UNSPECIFIED, isActive = true —
        // клиент вправе не присылать эти поля вовсе.
        let request: UpsertPersonRequest =
            serde_json::from_str(r#"{"name":"Mom"}"#).unwrap();

        assert_eq!(request.relation, PersonRelation::Other);
        assert_eq!(request.gender, Gender::Unspecified);
        assert!(request.is_active);
        assert_eq!(request.name.as_deref(), Some("Mom"));
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
        // `User` не реализует `Serialize` (см. models/user.rs) именно чтобы это
        // проверялось на уровне типов, а не рантайма — но проверим и содержимое
        // `UserDto`, чтобы регрессия (случайно добавленное поле) не прошла тихо.
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
}
