use crate::models::user::User;
use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterUserDto {
    #[validate(required(message = "Login is required"))]
    #[validate(length(min = 3, max = 50, message = "Login must be 3-50 characters"))]
    pub login: Option<String>,

    #[validate(required(message = "Password is required"))]
    #[validate(length(min = 6, message = "Password must be at least 6 characters"))]
    pub password: Option<String>,

    #[validate(required(message = "Name is required"))]
    #[validate(length(min = 1, max = 100, message = "Name must be 1-100 characters"))]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginUserDto {
    #[validate(required(message = "Login is required"))]
    #[validate(length(
        min = 3,
        max = 50,
        message = "Login must be between 3 and 50 characters"
    ))]
    pub login: Option<String>,

    #[validate(required(message = "Password is required"))]
    #[validate(length(min = 6, message = "Password must be at least 6 characters"))]
    pub password: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginResponseDto {
    pub id: String,
    pub login: String,
    pub name: String,
    pub token: String,
    pub token_expires_at: NaiveDateTime,
    pub subscribed: bool,
}

impl From<User> for LoginResponseDto {
    fn from(user: User) -> Self {
        LoginResponseDto {
            id: user.id,
            login: user.login,
            name: user.name,
            token: "".to_string(),
            token_expires_at: user.token_expires_at,
            subscribed: user.subscribed,
        }
    }
}
