use chrono::NaiveDateTime;
use sqlx::FromRow;
use std::fmt;

/// Модель строки таблицы `users`.
///
/// Намеренно **не** реализует `Serialize`: раньше это позволяло случайно отдать
/// клиенту хеш пароля. Наружу пользователь уходит только через DTO.
#[derive(FromRow)]
pub struct User {
    pub id: i32,
    pub email: String,
    /// `None` у аккаунтов, созданных через Google.
    pub password: Option<String>,
    pub name: String,
    /// Срок жизни токена подтверждения email. К сессии отношения не имеет.
    pub verification_token_expires_at: Option<NaiveDateTime>,
    pub verified: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: Option<NaiveDateTime>,
    pub verification_token: Option<String>,
    pub subscribed: bool,
    /// Claim `sub` из Google ID-токена: стабилен, в отличие от email.
    pub google_sub: Option<String>,
}

impl User {
    pub fn has_password_login(&self) -> bool {
        self.password.is_some()
    }
}

/// Ручной `Debug` — производный печатал хеш пароля и токен подтверждения
/// в каждый лог, куда попадал `User`.
impl fmt::Debug for User {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("User")
            .field("id", &self.id)
            .field("email", &self.email)
            .field("name", &self.name)
            .field("password", &self.password.as_ref().map(|_| "<redacted>"))
            .field(
                "verification_token",
                &self.verification_token.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "verification_token_expires_at",
                &self.verification_token_expires_at,
            )
            .field("verified", &self.verified)
            .field("subscribed", &self.subscribed)
            .field("google_sub", &self.google_sub.as_ref().map(|_| "<set>"))
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> User {
        User {
            id: 1,
            email: "user@example.com".into(),
            password: Some("$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$hash".into()),
            name: "User".into(),
            verification_token_expires_at: None,
            verified: true,
            created_at: chrono::NaiveDateTime::default(),
            updated_at: None,
            verification_token: Some("secret-token".into()),
            subscribed: false,
            google_sub: None,
        }
    }

    #[test]
    fn debug_output_hides_secrets() {
        let rendered = format!("{:?}", sample());
        assert!(!rendered.contains("argon2id"), "hash leaked: {rendered}");
        assert!(!rendered.contains("secret-token"), "token leaked: {rendered}");
        assert!(rendered.contains("<redacted>"));
    }
}
