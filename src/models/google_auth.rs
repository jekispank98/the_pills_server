use serde::Deserialize;

/// Claims Google ID-токена, которые нам нужны.
///
/// `aud`/`iss`/`exp` проверяет сама `jsonwebtoken` по `Validation`,
/// но держим их в структуре, чтобы можно было залогировать отвергнутый токен.
#[derive(Debug, Deserialize)]
pub struct GoogleTokenPayload {
    pub aud: String,
    pub iss: String,
    /// Стабильный идентификатор аккаунта — то, к чему привязываем пользователя.
    pub sub: String,
    pub email: String,
    /// Google отдаёт этот claim и как bool, и как строку "true"/"false".
    #[serde(default, deserialize_with = "deserialize_flexible_bool")]
    pub email_verified: bool,
    pub name: Option<String>,
    pub exp: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleJwk {
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoogleJwks {
    pub keys: Vec<GoogleJwk>,
}

impl GoogleJwks {
    pub fn find(&self, kid: &str) -> Option<&GoogleJwk> {
        self.keys.iter().find(|key| key.kid == kid)
    }
}

fn deserialize_flexible_bool<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum BoolOrString {
        Bool(bool),
        String(String),
    }

    Ok(match BoolOrString::deserialize(deserializer)? {
        BoolOrString::Bool(value) => value,
        BoolOrString::String(value) => value.eq_ignore_ascii_case("true"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_email_verified_as_bool_and_as_string() {
        let as_bool: GoogleTokenPayload = serde_json::from_str(
            r#"{"aud":"a","iss":"accounts.google.com","sub":"1","email":"a@b.c","email_verified":true,"exp":1}"#,
        )
        .unwrap();
        assert!(as_bool.email_verified);

        let as_string: GoogleTokenPayload = serde_json::from_str(
            r#"{"aud":"a","iss":"accounts.google.com","sub":"1","email":"a@b.c","email_verified":"true","exp":1}"#,
        )
        .unwrap();
        assert!(as_string.email_verified);
    }

    #[test]
    fn missing_email_verified_defaults_to_false() {
        let payload: GoogleTokenPayload = serde_json::from_str(
            r#"{"aud":"a","iss":"accounts.google.com","sub":"1","email":"a@b.c","exp":1}"#,
        )
        .unwrap();
        assert!(!payload.email_verified);
    }
}
