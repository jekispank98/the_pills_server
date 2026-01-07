use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GoogleTokenPayload {
    pub aud: String,
    pub iss: String,
    pub sub: String,
    pub email: String,
    pub name: Option<String>,
    pub exp: usize,
}

#[derive(Deserialize)]
pub struct GoogleJwk {
    pub kid: String,
    pub n: String,
    pub e: String,
}

#[derive(Deserialize)]
pub struct GoogleJwks {
    pub(crate) keys: Vec<GoogleJwk>,
}