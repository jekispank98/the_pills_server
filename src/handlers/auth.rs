use crate::db::AuthentificationTrait;
use crate::dtos::{LoginResponseDto, LoginUserDto, RegisterUserDto};
use crate::error::ErrorMessage;
use crate::state::AppState;
use crate::{error::HttpError, root};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{Router, get, post};
use axum::{Extension, Json};
use chrono::{Duration, NaiveDateTime, Utc};
use std::sync::Arc;
use validator::Validate;

pub fn auth_router() -> Router {
    Router::new()
        .route("/login", get(login))
        .route("/google_login", get(root))
        .route("/register", post(register))
        .route("/google_register", post(root))
        .route("/refresh_token", post(root))
}

pub async fn register(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<RegisterUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    /* Input data validation */
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    /* Parse value after validation */
    let login = body.login.unwrap();
    let password = body.password.unwrap();
    let name = body.name.unwrap();

    /* Check if user already exists */
    let existing_user = app_state
        .db_client
        .check_is_user_exist(login.clone())
        .await
        .map_err(|_| HttpError::server_error(ErrorMessage::ServerError.to_string()))?;
    if existing_user.is_some() {
        Err(HttpError::unique_constraint_violation(
            ErrorMessage::EmailExist.to_string(),
        ))
    } else {
        println!("User does not exist - creating");
        let password_hash = bcrypt::hash(&password, bcrypt::DEFAULT_COST)
            .map_err(|_| HttpError::server_error(ErrorMessage::HashingError.to_string()))?;

        /* Generate user ID and token */
        let user_id = uuid::Uuid::new_v4().to_string();
        let token = generate_token();
        let token_expires_at = calculate_expiration();

        /* Save user to database */
        let user = app_state
            .db_client
            .save_user(
                name,
                login.clone(),
                password_hash,
                user_id,
                token_expires_at,
            )
            .await
            .map_err(|_| HttpError::server_error(ErrorMessage::ServerError.to_string()))?;

        /* Build DTO Response */
        let response = LoginResponseDto {
            id: user.id,
            login: user.login,
            name: user.name,
            token,
            token_expires_at,
            subscribed: user.subscribed,
        };

        Ok((StatusCode::CREATED, Json(response)))
    }
}
pub async fn login(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<LoginUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    /* Input data validation */
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    /* Parse value after validation */
    let login = body.login.unwrap(); // safe после валидации
    let password = body.password.unwrap();

    let user = app_state
        .db_client
        .check_is_user_exist(login)
        .await
        .map_err(|_| HttpError::server_error(ErrorMessage::ServerError.to_string()))?
        .ok_or_else(|| HttpError::unauthorized(ErrorMessage::WrongCredentials.to_string()))?;

    // Проверка пароля (предполагая, что password - хэш)
    // В реальности нужно использовать bcrypt или аналоги
    if user.password != password {
        return Err(HttpError::unauthorized(
            ErrorMessage::WrongCredentials.to_string(),
        ));
    }

    let token = generate_token();
    let token_expires_at = calculate_expiration();
    let id = user.id.clone();
    app_state
        .db_client
        .update_token_expires_at(token_expires_at, id)
        .await
        .map_err(|_| HttpError::server_error(ErrorMessage::ServerError.to_string()))?;

    /* Build DTO Response */
    let response = LoginResponseDto {
        id: user.id,
        login: user.login,
        name: user.name,
        token,
        token_expires_at,
        subscribed: user.subscribed,
    };

    Ok(Json(response))
}

pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn calculate_expiration() -> NaiveDateTime {
    let datetime_utc = Utc::now() + Duration::hours(24);
    datetime_utc.naive_utc() // Преобразуем в NaiveDateTime
}
