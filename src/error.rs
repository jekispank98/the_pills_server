use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fmt::Display;

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub status: String,
    pub message: String,
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", serde_json::to_string(&self).unwrap_or_default())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorMessage {
    EmptyPassword,
    ExceededMaxPasswordLength(usize),
    InvalidHashFormat,
    HashingError,
    InvalidToken,
    ServerError,
    WrongCredentials,
    EmailExist,
    UserNoLongerExist,
    TokenNotProvided,
    PermissionDenied,
    UserNotAuthenticated,
    EmailNotVerified,
    VerificationTokenExpired,
    AlreadyVerified,
    PasswordLoginUnavailable,
    GoogleUnavailable,
    GoogleTokenRejected,
    TooManyRequests,
}

impl Display for ErrorMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl ErrorMessage {
    /// Возвращает `&'static str`: сообщения статичны, кроме одного параметризованного,
    /// поэтому прежняя аллокация `String` на каждом вызове была лишней.
    pub fn as_str(&self) -> &'static str {
        match self {
            ErrorMessage::ServerError => "Server Error. Please try again later",
            ErrorMessage::WrongCredentials => "Email or password is wrong",
            ErrorMessage::EmailExist => "A user with this email already exists",
            ErrorMessage::UserNoLongerExist => "User belonging to this token no longer exists",
            ErrorMessage::EmptyPassword => "Password cannot be empty",
            ErrorMessage::HashingError => "Error while hashing password",
            ErrorMessage::InvalidHashFormat => "Invalid password hash format",
            ErrorMessage::ExceededMaxPasswordLength(_) => "Password is too long",
            ErrorMessage::InvalidToken => "Authentication token is invalid or expired",
            ErrorMessage::TokenNotProvided => "You are not logged in, please provide a token",
            ErrorMessage::PermissionDenied => "You are not allowed to perform this action",
            ErrorMessage::UserNotAuthenticated => "Authentication required. Please log in.",
            ErrorMessage::EmailNotVerified => {
                "Please confirm your email address before signing in"
            }
            ErrorMessage::VerificationTokenExpired => {
                "This verification link has expired. Request a new one."
            }
            ErrorMessage::AlreadyVerified => "This email address is already confirmed",
            ErrorMessage::PasswordLoginUnavailable => {
                "This account was created via Google. Please sign in with Google."
            }
            ErrorMessage::GoogleUnavailable => {
                "Google sign-in is temporarily unavailable. Please try again."
            }
            ErrorMessage::GoogleTokenRejected => "Google sign-in token was rejected",
            ErrorMessage::TooManyRequests => "Too many requests. Please slow down.",
        }
    }
}

impl From<ErrorMessage> for String {
    fn from(value: ErrorMessage) -> Self {
        match value {
            ErrorMessage::ExceededMaxPasswordLength(max) => {
                format!("Password must not be more than {max} characters")
            }
            other => other.as_str().to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpError {
    pub message: String,
    pub status: StatusCode,
}

impl HttpError {
    pub fn new(message: impl Into<String>, status: StatusCode) -> Self {
        HttpError {
            message: message.into(),
            status,
        }
    }

    pub fn server_error(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::INTERNAL_SERVER_ERROR)
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::BAD_REQUEST)
    }

    pub fn unique_constraint_violation(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::CONFLICT)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::UNAUTHORIZED)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::SERVICE_UNAVAILABLE)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(message, StatusCode::TOO_MANY_REQUESTS)
    }

    pub fn into_http_response(self) -> Response {
        let json_response = Json(ErrorResponse {
            status: "fail".to_string(),
            message: self.message,
        });

        (self.status, json_response).into_response()
    }
}

impl From<ErrorMessage> for HttpError {
    fn from(value: ErrorMessage) -> Self {
        let status = match value {
            ErrorMessage::WrongCredentials
            | ErrorMessage::InvalidToken
            | ErrorMessage::TokenNotProvided
            | ErrorMessage::UserNotAuthenticated
            | ErrorMessage::UserNoLongerExist
            | ErrorMessage::GoogleTokenRejected
            | ErrorMessage::PasswordLoginUnavailable
            | ErrorMessage::EmailNotVerified => StatusCode::UNAUTHORIZED,
            ErrorMessage::PermissionDenied => StatusCode::FORBIDDEN,
            ErrorMessage::EmailExist => StatusCode::CONFLICT,
            ErrorMessage::EmptyPassword
            | ErrorMessage::ExceededMaxPasswordLength(_)
            | ErrorMessage::VerificationTokenExpired
            | ErrorMessage::AlreadyVerified => StatusCode::BAD_REQUEST,
            ErrorMessage::GoogleUnavailable => StatusCode::SERVICE_UNAVAILABLE,
            ErrorMessage::TooManyRequests => StatusCode::TOO_MANY_REQUESTS,
            ErrorMessage::HashingError
            | ErrorMessage::InvalidHashFormat
            | ErrorMessage::ServerError => StatusCode::INTERNAL_SERVER_ERROR,
        };

        HttpError::new(String::from(value), status)
    }
}

/// Единая конверсия ошибок БД. Раньше на каждом вызове стоял ручной
/// `.map_err(|e| HttpError::server_error(e.to_string()))`, который отдавал клиенту
/// внутренние детали Postgres. Теперь наружу уходит generic-сообщение,
/// а подробности попадают в лог.
impl From<sqlx::Error> for HttpError {
    fn from(error: sqlx::Error) -> Self {
        if let Some(db_error) = error.as_database_error() {
            if db_error.is_unique_violation() {
                return HttpError::unique_constraint_violation(ErrorMessage::EmailExist);
            }
        }

        tracing::error!(error = %error, "database query failed");
        HttpError::server_error(ErrorMessage::ServerError)
    }
}

impl From<jsonwebtoken::errors::Error> for HttpError {
    fn from(error: jsonwebtoken::errors::Error) -> Self {
        tracing::error!(error = %error, "failed to issue session token");
        HttpError::server_error(ErrorMessage::ServerError)
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HttpError: message: {}, status: {}",
            self.message, self.status
        )
    }
}

impl std::error::Error for HttpError {}

impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        self.into_http_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameterized_message_keeps_its_argument() {
        assert_eq!(
            String::from(ErrorMessage::ExceededMaxPasswordLength(64)),
            "Password must not be more than 64 characters"
        );
    }

    #[test]
    fn domain_errors_map_to_expected_statuses() {
        assert_eq!(
            HttpError::from(ErrorMessage::EmailNotVerified).status,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            HttpError::from(ErrorMessage::EmailExist).status,
            StatusCode::CONFLICT
        );
        assert_eq!(
            HttpError::from(ErrorMessage::GoogleUnavailable).status,
            StatusCode::SERVICE_UNAVAILABLE
        );
    }
}
