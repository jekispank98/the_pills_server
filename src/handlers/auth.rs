use crate::db::{AuthentificationTrait, RefreshTokenRepositoryTrait, RefreshTokenRotation};
use crate::dtos::{
    GoogleLoginDto, LoginResponseDto, LoginUserDto, RefreshTokenRequestDto,
    RefreshTokenResponseDto, RegisterUserDto, Response, VerifyEmailQueryDto,
};
use crate::error::ErrorMessage;
use crate::error::HttpError;
use crate::extractors::JsonBody;
use crate::mail::mails::{send_verification_email, send_welcome_email};
use crate::rate_limit::{RateLimiter, limit_by_ip};
use crate::state::AppState;
use crate::utils::refresh_token;
use crate::utils::{password, token};
use axum::extract::Query;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse};
use axum::routing::{Router, get, post};
use axum::{Extension, Json, middleware};
use axum_extra::extract::cookie::Cookie;
use chrono::{Duration, NaiveDateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use moka::future::Cache;
use std::fs;
use std::sync::Arc;
use validator::Validate;

use crate::models::google_auth::{GoogleJwks, GoogleTokenPayload};

pub fn auth_router(limiter: RateLimiter) -> Router {
    let rate_limited = Router::new()
        .route("/login", post(login))
        .route("/google_login", post(google_login))
        .route("/register", post(register))
        .route("/verify", get(verify_email))
        .route("/google_register", post(google_login))
        .route_layer(middleware::from_fn_with_state(limiter, limit_by_ip));

    Router::new()
        .merge(rate_limited)
        .route("/refresh_token", post(refresh_session))
        .route("/logout", post(logout))
}

const REFRESH_TOKEN_REUSE_GRACE_SECONDS: i64 = 120;

fn refresh_token_expires_at(app_state: &AppState) -> Result<NaiveDateTime, HttpError> {
    let ttl = Duration::from_std(app_state.config.refresh_token_max_age)
        .map_err(|_| HttpError::server_error(ErrorMessage::ServerError))?;
    Ok((Utc::now() + ttl).naive_utc())
}

async fn issue_refresh_token(
    app_state: &AppState,
    user_id: uuid::Uuid,
) -> Result<String, HttpError> {
    let token = refresh_token::generate();
    app_state
        .db_client
        .create_refresh_token(
            user_id,
            &refresh_token::hash(&token),
            refresh_token_expires_at(app_state)?,
        )
        .await?;
    Ok(token)
}

pub async fn refresh_session(
    Extension(app_state): Extension<Arc<AppState>>,
    JsonBody(body): JsonBody<RefreshTokenRequestDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let presented = body.refresh_token.unwrap();
    let new_refresh_token = refresh_token::generate();

    let rotation = app_state
        .db_client
        .rotate_refresh_token(
            &refresh_token::hash(&presented),
            &refresh_token::hash(&new_refresh_token),
            refresh_token_expires_at(&app_state)?,
            Duration::seconds(REFRESH_TOKEN_REUSE_GRACE_SECONDS),
        )
        .await?;

    let user_id = match rotation {
        RefreshTokenRotation::Rotated { user_id } => user_id,
        RefreshTokenRotation::ReuseDetected => {
            tracing::warn!("refresh token reuse detected, token family revoked");
            return Err(HttpError::from(ErrorMessage::InvalidToken));
        }
        RefreshTokenRotation::Invalid => return Err(HttpError::from(ErrorMessage::InvalidToken)),
    };

    let token = token::create_token(
        &user_id.to_string(),
        app_state.config.jwt_secret.as_bytes(),
        app_state.config.jwt_max_age,
    )?;

    let headers = session_cookie_headers(&app_state, &token);

    Ok((
        headers,
        Json(RefreshTokenResponseDto {
            token,
            refresh_token: new_refresh_token,
        }),
    ))
}

pub async fn logout(
    Extension(app_state): Extension<Arc<AppState>>,
    JsonBody(body): JsonBody<RefreshTokenRequestDto>,
) -> Result<StatusCode, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    app_state
        .db_client
        .revoke_refresh_token(&refresh_token::hash(&body.refresh_token.unwrap()))
        .await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn register(
    Extension(app_state): Extension<Arc<AppState>>,
    JsonBody(body): JsonBody<RegisterUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let email = body.email.unwrap();
    let name = body.name.unwrap();
    let password_hash = password::hash_password(body.password.unwrap()).await?;

    let token = generate_token();
    let token_expires_at = calculate_expiration();

    let (user, _self_person) = app_state
        .db_client
        .register_user_with_self_person(name, email, password_hash, token_expires_at, token.clone())
        .await?;

    let verification_link = app_state.config.verification_link(&token);
    let smtp = app_state.config.smtp.clone();
    let (to_email, username) = (user.email.clone(), user.name.clone());
    tokio::spawn(async move {
        if let Err(e) =
            send_verification_email(&smtp, &verification_link, &to_email, &username).await
        {
            tracing::error!(error = %e, email = %to_email, "failed to send verification email");
        }
    });

    let response = Response {
        status: "success",
        message: "Registration successful! Please check your email to verify your account."
            .to_string(),
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn login(
    Extension(app_state): Extension<Arc<AppState>>,
    JsonBody(body): JsonBody<LoginUserDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let email = body.email.unwrap();
    let password = body.password.unwrap();

    let user = app_state.db_client.check_is_user_exist(&email).await?;

    let user = match user {
        Some(user) => user,
        None => {
            password::verify_dummy_password(password).await;
            return Err(HttpError::from(ErrorMessage::WrongCredentials));
        }
    };

    let Some(password_hash) = user.password.clone() else {
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
    let refresh_token = issue_refresh_token(&app_state, user.id).await?;

    let response = LoginResponseDto {
        id: user.id,
        email: user.email,
        name: user.name,
        token,
        refresh_token,
        subscribed: user.subscribed,
    };

    Ok((headers, Json(response)))
}

pub async fn google_login(
    Extension(app_state): Extension<Arc<AppState>>,
    JsonBody(body): JsonBody<GoogleLoginDto>,
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

    let smtp = app_state.config.smtp.clone();
    let (to_email, username) = (user.email.clone(), user.name.clone());
    tokio::spawn(async move {
        if let Err(e) = send_welcome_email(&smtp, &to_email, &username).await {
            tracing::error!(error = %e, email = %to_email, "failed to send welcome email");
        }
    });

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
    let header = decode_header(token).map_err(|_| ErrorMessage::GoogleTokenRejected)?;
    let kid = header.kid.ok_or(ErrorMessage::GoogleTokenRejected)?;

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

    let key_data = jwks.find(&kid).ok_or(ErrorMessage::GoogleTokenRejected)?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[web_client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

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
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::util::ServiceExt;

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

    fn test_limiter() -> RateLimiter {
        RateLimiter::new(100, std::time::Duration::from_secs(60))
    }

    #[tokio::test]
    async fn google_register_route_exists_for_post() {
        let app = auth_router(test_limiter());
        let request = Request::builder()
            .method(Method::POST)
            .uri("/google_register")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"id_token":"x"}"#))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn google_register_route_rejects_get_method() {
        let app = auth_router(test_limiter());
        let request = Request::builder()
            .method(Method::GET)
            .uri("/google_register")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }
}
