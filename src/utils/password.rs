use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

use crate::error::ErrorMessage;

const MAX_PASSWORD_LENGTH: usize = 128;

pub async fn hash_password(password: String) -> Result<String, ErrorMessage> {
    validate_length(&password)?;

    spawn_hashing(move || {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map_err(|_| ErrorMessage::HashingError)
            .map(|hash| hash.to_string())
    })
    .await
}

pub async fn verify_password(password: String, hash: String) -> Result<bool, ErrorMessage> {
    validate_length(&password)?;

    spawn_hashing(move || {
        if is_bcrypt_hash(&hash) {
            return bcrypt::verify(&password, &hash).map_err(|_| ErrorMessage::InvalidHashFormat);
        }

        let parsed = PasswordHash::new(&hash).map_err(|_| ErrorMessage::InvalidHashFormat)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
}

pub async fn verify_dummy_password(password: String) {
    let _ = verify_password(password, dummy_hash().to_string()).await;
}

fn dummy_hash() -> &'static str {
    static HASH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        let salt = SaltString::generate(&mut OsRng);
        Argon2::default()
            .hash_password(b"dummy-password-for-timing-safety", &salt)
            .expect("hashing the dummy password must not fail")
            .to_string()
    })
}

pub fn needs_rehash(hash: &str) -> bool {
    is_bcrypt_hash(hash)
}

fn is_bcrypt_hash(hash: &str) -> bool {
    hash.starts_with("$2a$") || hash.starts_with("$2b$") || hash.starts_with("$2y$")
}

fn validate_length(password: &str) -> Result<(), ErrorMessage> {
    if password.is_empty() {
        return Err(ErrorMessage::EmptyPassword);
    }
    if password.len() > MAX_PASSWORD_LENGTH {
        return Err(ErrorMessage::ExceededMaxPasswordLength(MAX_PASSWORD_LENGTH));
    }
    Ok(())
}

async fn spawn_hashing<T, F>(work: F) -> Result<T, ErrorMessage>
where
    F: FnOnce() -> Result<T, ErrorMessage> + Send + 'static,
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result,
        Err(error) => {
            tracing::error!(error = %error, "password hashing task panicked");
            Err(ErrorMessage::HashingError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn argon2_roundtrip() {
        let hash = hash_password("correct horse".into()).await.unwrap();
        assert!(
            verify_password("correct horse".into(), hash.clone())
                .await
                .unwrap()
        );
        assert!(!verify_password("wrong horse".into(), hash).await.unwrap());
    }

    #[tokio::test]
    async fn verifies_legacy_bcrypt_hashes() {
        let legacy = bcrypt::hash("legacy pass", 4).unwrap();
        assert!(needs_rehash(&legacy));
        assert!(
            verify_password("legacy pass".into(), legacy.clone())
                .await
                .unwrap()
        );
        assert!(!verify_password("nope".into(), legacy).await.unwrap());
    }

    #[tokio::test]
    async fn argon2_hash_is_not_flagged_for_rehash() {
        let hash = hash_password("whatever".into()).await.unwrap();
        assert!(!needs_rehash(&hash));
    }

    #[tokio::test]
    async fn rejects_empty_and_overlong_passwords() {
        assert_eq!(
            hash_password(String::new()).await.unwrap_err(),
            ErrorMessage::EmptyPassword
        );
        assert_eq!(
            hash_password("x".repeat(MAX_PASSWORD_LENGTH + 1))
                .await
                .unwrap_err(),
            ErrorMessage::ExceededMaxPasswordLength(MAX_PASSWORD_LENGTH)
        );
    }

    #[tokio::test]
    async fn dummy_verification_does_not_panic() {
        verify_dummy_password("anything".into()).await;
    }

    #[tokio::test]
    async fn malformed_hash_is_reported_as_such() {
        assert_eq!(
            verify_password("pass".into(), "not-a-hash".into())
                .await
                .unwrap_err(),
            ErrorMessage::InvalidHashFormat
        );
    }
}
