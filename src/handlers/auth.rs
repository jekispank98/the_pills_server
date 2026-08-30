use crate::db::AuthentificationTrait;
use crate::dtos::{
    GoogleLoginDto, LoginResponseDto, LoginUserDto, RegisterUserDto, Response, VerifyEmailQueryDto,
};
use crate::error::ErrorMessage;
use crate::mail::mails::{send_verification_email, send_welcome_email};
use crate::state::AppState;
use crate::utils::{password, token};
use crate::{error::HttpError, root};
use axum::extract::Query;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post, Router};
use axum::{Extension, Json};
use axum_extra::extract::cookie::Cookie;
use chrono::{Duration, NaiveDateTime, Utc};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use moka::future::Cache;
use std::fs;
use std::sync::Arc;
use validator::Validate;

use crate::models::google_auth::{GoogleJwks, GoogleTokenPayload};

pub fn auth_router() -> Router {
    Router::new()
        .route("/login", get(login))
        .route("/google_login", get(google_login))
        .route("/register", post(register))
        .route("/verify", get(verify_email))
        .route("/google_register", post(root))
        .route("/refresh_token", post(root))
}

pub async fn register(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<RegisterUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let email = body.email.unwrap();
    let name = body.name.unwrap();
    let password_hash = password::hash_password(body.password.unwrap()).await?;

    let token = generate_token();
    let token_expires_at = calculate_expiration();

    // `register_user_with_self_person` возвращает sqlx::Error, который
    // конвертируется в HttpError через `From<sqlx::Error>` (см. error.rs) — там
    // уже учтён unique_violation → 409 EmailExist, и наружу не утекают детали
    // Postgres. User и его SELF-`Person` создаются одной транзакцией — как в
    // Android `UserRepositoryImpl.signUp`.
    let (user, _self_person) = app_state
        .db_client
        .register_user_with_self_person(name, email, password_hash, token_expires_at, token.clone())
        .await?;

    let verification_link = app_state.config.verification_link(&token);
    if let Err(e) =
        send_verification_email(&app_state.config.smtp, &verification_link, &user.email, &user.name).await
    {
        tracing::error!(error = %e, email = %user.email, "failed to send verification email");
    }

    let response = Response {
        status: "success",
        message: "Registration successful! Please check your email to verify your account."
            .to_string(),
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<LoginUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let email = body.email.unwrap();
    let password = body.password.unwrap();

    let user = app_state.db_client.check_is_user_exist(&email).await?;

    let user = match user {
        Some(user) => user,
        None => {
            // Прогоняем фиктивную проверку, чтобы «пользователь не найден» и
            // «пароль неверный» занимали одинаковое время и не позволяли
            // перебором email узнавать, какие аккаунты существуют.
            password::verify_dummy_password(password).await;
            return Err(HttpError::from(ErrorMessage::WrongCredentials));
        }
    };

    let Some(password_hash) = user.password.clone() else {
        // Аккаунт создан через Google — пароля нет вовсе.
        return Err(HttpError::from(ErrorMessage::PasswordLoginUnavailable));
    };

    if !password::verify_password(password, password_hash).await? {
        return Err(HttpError::from(ErrorMessage::WrongCredentials));
    }

    if !user.verified {
        return Err(HttpError::from(ErrorMessage::EmailNotVerified));
    }

    let token = token::create_token(
        &user.id.to_string(),
        app_state.config.jwt_secret.as_bytes(),
        app_state.config.jwt_max_age,
    )?;

    let headers = session_cookie_headers(&app_state, &token);

    let response = LoginResponseDto {
        id: user.id,
        email: user.email,
        name: user.name,
        token,
        subscribed: user.subscribed,
    };

    Ok((headers, Json(response)))
}

pub async fn google_login(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<GoogleLoginDto>,
) -> Result<StatusCode, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let id_token = body.id_token.as_ref().unwrap();

    let claims = verify_google_token(
        id_token,
        &app_state.config.google_web_client_id,
        &app_state.google_keys_cache,
    )
    .await?;

    tracing::debug!(
        email = %claims.email,
        name = ?claims.name,
        "google token verified, but account provisioning is not implemented yet"
    );

    // Поиск/создание пользователя по `google_sub` и выпуск сессии — следующий шаг;
    // намеренно не притворяемся, что вход уже работает (раньше хендлер возвращал
    // `Ok(())`, то есть 200 с пустым телом, ничего на самом деле не сделав).
    Err(HttpError::new(
        "Google sign-in verification succeeded, but account provisioning is not implemented yet",
        StatusCode::NOT_IMPLEMENTED,
    ))
}

pub async fn verify_email(
    Query(query_params): Query<VerifyEmailQueryDto>,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<impl IntoResponse, HttpError> {
    query_params
        .validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let user = app_state
        .db_client
        .get_user(None, None, None, Some(&query_params.token))
        .await?
        .ok_or_else(|| HttpError::from(ErrorMessage::InvalidToken))?;

    let expires_at = user
        .verification_token_expires_at
        .ok_or_else(|| HttpError::from(ErrorMessage::InvalidToken))?;

    if Utc::now().naive_utc() > expires_at {
        return Err(HttpError::from(ErrorMessage::VerificationTokenExpired));
    }

    app_state
        .db_client
        .verified_token(&query_params.token)
        .await?;

    if let Err(e) = send_welcome_email(&app_state.config.smtp, &user.email, &user.name).await {
        tracing::error!(error = %e, email = %user.email, "failed to send welcome email");
    }

    let token = token::create_token(
        &user.id.to_string(),
        app_state.config.jwt_secret.as_bytes(),
        app_state.config.jwt_max_age,
    )?;

    let headers = session_cookie_headers(&app_state, &token);

    let success_html = "src/mail/templates/VerificationSuccess.html";
    let mut html_content = fs::read_to_string(success_html)
        .unwrap_or_else(|_| "<h1>Аккаунт подтвержден!</h1>".to_string());
    html_content = html_content.replace("{{username}}", &user.name);

    Ok((headers, Html(html_content)))
}

/// Строит `Set-Cookie` для сессионного JWT. `Secure` берётся из `Config::cookie_secure`
/// (production-режим) — раньше `Config` уже умел это вычислять, но ни один
/// хендлер не читал это поле, и cookie никогда не помечалась `Secure`.
fn session_cookie_headers(app_state: &AppState, token: &str) -> HeaderMap {
    let cookie_duration = time::Duration::seconds(app_state.config.jwt_max_age.as_secs() as i64);
    let cookie = Cookie::build(("token", token.to_string()))
        .path("/")
        .max_age(cookie_duration)
        .http_only(true)
        .secure(app_state.config.cookie_secure())
        .build();

    let mut headers = HeaderMap::new();
    headers.append(header::SET_COOKIE, cookie.to_string().parse().unwrap());
    headers
}

pub async fn verify_google_token(
    token: &str,
    web_client_id: &str,
    cache: &Cache<String, String>,
) -> Result<GoogleTokenPayload, ErrorMessage> {
    // 1. Декодируем заголовок, чтобы получить kid (Key ID)
    let header = decode_header(token).map_err(|_| ErrorMessage::GoogleTokenRejected)?;
    let kid = header.kid.ok_or(ErrorMessage::GoogleTokenRejected)?;

    // 2. Получаем публичные ключи Google (из кеша или запросом)
    let jwks_body = if let Some(cached) = cache.get("google_jwks").await {
        cached
    } else {
        let res = reqwest::get("https://www.googleapis.com/oauth2/v3/certs")
            .await
            .map_err(|_| ErrorMessage::GoogleUnavailable)?
            .text()
            .await
            .map_err(|_| ErrorMessage::GoogleUnavailable)?;
        cache.insert("google_jwks".to_string(), res.clone()).await;
        res
    };

    let jwks: GoogleJwks =
        serde_json::from_str(&jwks_body).map_err(|_| ErrorMessage::GoogleUnavailable)?;

    // 3. Ищем нужный ключ по kid
    let key_data = jwks.find(&kid).ok_or(ErrorMessage::GoogleTokenRejected)?;

    // 4. Настраиваем валидацию
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[web_client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

    // 5. Декодируем и проверяем подпись
    let decoding_key = DecodingKey::from_rsa_components(&key_data.n, &key_data.e)
        .map_err(|_| ErrorMessage::GoogleTokenRejected)?;

    let token_data = decode::<GoogleTokenPayload>(token, &decoding_key, &validation)
        .map_err(|_| ErrorMessage::GoogleTokenRejected)?;

    Ok(token_data.claims)
}

pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn calculate_expiration() -> NaiveDateTime {
    let datetime_utc = Utc::now() + Duration::days(2);
    datetime_utc.naive_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn malformed_google_token_is_rejected_without_network_call() {
        let cache: Cache<String, String> = Cache::new(1);
        let result = verify_google_token("not-a-jwt", "client-id", &cache).await;
        assert_eq!(result.unwrap_err(), ErrorMessage::GoogleTokenRejected);
    }

    #[test]
    fn calculate_expiration_is_two_days_out() {
        let expires = calculate_expiration();
        let now = Utc::now().naive_utc();
        let delta = expires - now;
        assert!(delta.num_hours() >= 47 && delta.num_hours() <= 48);
    }

    #[test]
    fn generate_token_is_unique_per_call() {
        assert_ne!(generate_token(), generate_token());
    }
}
