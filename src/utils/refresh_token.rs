use rand::RngCore;
use rand::rngs::OsRng;
use sha2::{Digest, Sha256};

pub fn generate() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_tokens_are_long_and_unique() {
        let first = generate();
        assert_eq!(first.len(), 64);
        assert_ne!(first, generate());
    }

    #[test]
    fn hash_is_stable_and_differs_from_the_token() {
        let token = generate();
        assert_eq!(hash(&token), hash(&token));
        assert_ne!(hash(&token), token);
        assert_ne!(hash(&token), hash(&generate()));
    }
}
