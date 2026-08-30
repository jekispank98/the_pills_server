use serde::{Deserialize, Serialize};
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
    pub id: i32,
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
