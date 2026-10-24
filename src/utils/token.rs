use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::error::{ErrorMessage, HttpError};

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenClaims {
    pub sub: String,
    pub iat: usize,
    pub exp: usize,
}

pub fn create_token(
    user_id: &str,
    secret: &[u8],
    ttl: Duration,
) -> Result<String, jsonwebtoken::errors::Error> {
    if user_id.is_empty() {
        return Err(jsonwebtoken::errors::ErrorKind::InvalidSubject.into());
    }

    let now = Utc::now();
    let iat = now.timestamp() as usize;
    let exp = iat + ttl.as_secs() as usize;
    let claims = TokenClaims {
        sub: user_id.to_string(),
        iat,
        exp,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret),
    )
}

pub fn decode_token<T: Into<String>>(token: T, secret: &[u8]) -> Result<String, HttpError> {
    let decode = decode::<TokenClaims>(
        &token.into(),
        &DecodingKey::from_secret(secret),
        &Validation::new(Algorithm::HS256),
    );

    match decode {
        Ok(token) => Ok(token.claims.sub),
        Err(_) => Err(HttpError::from(ErrorMessage::InvalidToken)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-that-is-long-enough";

    #[test]
    fn roundtrip_recovers_subject() {
        let token = create_token("42", SECRET, Duration::from_secs(3600)).unwrap();
        assert_eq!(decode_token(token, SECRET).unwrap(), "42");
    }

    #[test]
    fn empty_subject_is_rejected() {
        assert!(create_token("", SECRET, Duration::from_secs(60)).is_err());
    }

    #[test]
    fn expired_token_is_rejected() {
        let now = Utc::now().timestamp() as usize;
        let claims = TokenClaims {
            sub: "1".to_string(),
            iat: now - 7200,
            exp: now - 3600,
        };
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap();
        assert!(decode_token(token, SECRET).is_err());
    }

    #[test]
    fn wrong_secret_is_rejected() {
        let token = create_token("1", SECRET, Duration::from_secs(3600)).unwrap();
        assert!(decode_token(token, b"a-completely-different-secret-x").is_err());
    }

    #[test]
    fn ttl_is_seconds_not_minutes() {
        let token = create_token("1", SECRET, Duration::from_secs(30)).unwrap();
        let claims = decode::<TokenClaims>(&token, &DecodingKey::from_secret(SECRET), &{
            let mut v = Validation::new(Algorithm::HS256);
            v.leeway = 0;
            v
        })
        .unwrap()
        .claims;
        assert_eq!(claims.exp - claims.iat, 30);
    }
}
