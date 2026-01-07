use std::fs;
use crate::db::AuthentificationTrait;
use crate::dtos::{GoogleLoginDto, LoginResponseDto, LoginUserDto, RegisterUserDto, Response, VerifyEmailQueryDto};
use crate::error::ErrorMessage;
use crate::mail::mails::{send_verification_email, send_welcome_email};
use crate::state::AppState;
use crate::utils::token;
use crate::{error::HttpError, root};
use axum::extract::Query;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect};
use axum::routing::{get, post, Router};
use axum::{Extension, Json};
use axum_extra::extract::cookie::Cookie;
use chrono::{Duration, NaiveDateTime, Utc};
use std::sync::Arc;
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use moka::future::Cache;
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

    let login = body.email.unwrap();
    let name = body.name.unwrap();

    let password_hash = bcrypt::hash(body.password.unwrap(), bcrypt::DEFAULT_COST)
        .map_err(|_| HttpError::server_error(ErrorMessage::HashingError.to_string()))?;

    let token = generate_token();
    let token_expires_at = Some(calculate_expiration());

    let user = app_state
        .db_client
        .save_user(name.clone(), login.clone(), password_hash, token_expires_at.unwrap(), token.clone())
        .await
        .map_err(|e| {
            if let Some(db_err) = e.as_database_error() {
                if db_err.is_unique_violation() {
                    return HttpError::unique_constraint_violation(
                        ErrorMessage::EmailExist.to_string(),
                    );
                }
            }
            HttpError::server_error(e.to_string())
        })?;

    let send_email_result = send_verification_email(&user.email, &user.name, &token).await;

    if let Err(e) = send_email_result {
        eprintln!("Failed to send verification email: {}", e);
        // Здесь можно принять решение, как реагировать. Например, залогировать и
        // сообщить пользователю, что письмо будет отправлено позже.
        // Или откатить транзакцию, если это возможно.
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
    /* Input data validation */
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    /* Parse value after validation */
    let email = body.email.unwrap(); // safe после валидации
    let password = body.password.unwrap();
    let user = app_state
        .db_client
        .check_is_user_exist(email)
        .await
        .map_err(|_| HttpError::server_error(ErrorMessage::ServerError.to_string()))?
        .ok_or_else(|| HttpError::unauthorized(ErrorMessage::WrongCredentials.to_string()))?;

    if !bcrypt::verify(password, &user.password)
        .map_err(|_| HttpError::server_error(ErrorMessage::HashingError.to_string()))? {
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
        email: user.email,
        name: user.name,
        token,
        token_expires_at,
        subscribed: user.subscribed,
    };

    Ok(Json(response))
}

pub async fn google_login(
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<GoogleLoginDto>,
) -> Result<impl IntoResponse, HttpError> {
    body.validate().map_err(|e| HttpError::bad_request(e.to_string()))?;

    let id_token = body.id_token.as_ref().unwrap();

    // Верификация
    let claims = verify_google_token(
        id_token,
        &app_state.config.google_web_client_id,
        &app_state.google_keys_cache
    ).await.map_err(|e| HttpError::unauthorized(e))?;

    // Далее логика поиска/создания пользователя как в предыдущем примере
    let email = claims.email;
    let name = claims.name.unwrap_or_else(|| "Google User".to_string());

    // ... твой код работы с БД и генерации своего JWT ...

    Ok(())
}
pub async fn verify_email(
    Query(query_params): Query<VerifyEmailQueryDto>,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<impl IntoResponse, HttpError> {
    println!("verify email is started");
    query_params
        .validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;
    println!("Received token: {}", query_params.token);
    let result = app_state
        .db_client
        .get_user(None, None, None, Some(&query_params.token))
        .await
        .map_err(|e| HttpError::server_error(e.to_string()))?;
    println!("DB search result: {:?}", result);
    let user = result.ok_or(HttpError::unauthorized(
        ErrorMessage::InvalidToken.to_string(),
    ))?;

    // Проверяем, что токен не просрочен
    if Utc::now().naive_utc() > user.token_expires_at.expect("Verification token has expired") {
        return Err(HttpError::bad_request(
            "Verification token has expired".to_string(),
        ))?;
    }
    println!("дошли сюда");
    app_state
        .db_client
        .verified_token(&query_params.token)
        .await
        .map_err(|e| HttpError::server_error(e.to_string()))?;

    let send_welcome_email_result = send_welcome_email(&user.email, &user.name).await;

    if let Err(e) = send_welcome_email_result {
        eprintln!("Failed to send welcome email: {}", e);
    }

    let token = token::create_token(
        &user.id.to_string(),
        app_state.config.jwt_secret.as_bytes(),
        app_state.config.jwt_maxage,
    )
        .map_err(|e| HttpError::server_error(e.to_string()))?;

    let cookie_duration = time::Duration::minutes(app_state.config.jwt_maxage * 60);
    let cookie = Cookie::build(("token", token.clone()))
        .path("/")
        .max_age(cookie_duration)
        .http_only(true)
        .build();

    let mut headers = HeaderMap::new();
    headers.append(header::SET_COOKIE, cookie.to_string().parse().unwrap());
    let success_html = "src/mail/templates/VerificationSuccess.html";
    let mut html_content = fs::read_to_string(success_html)
        .unwrap_or_else(|_| "<h1>Аккаунт подтвержден!</h1>".to_string());
    html_content = html_content.replace("{{username}}", &user.name);
    Ok((headers, Html(html_content)))
}

pub async fn verify_google_token(
    token: &str,
    web_client_id: &str,
    cache: &Cache<String, String>, // Кеш для хранения ключей
) -> Result<GoogleTokenPayload, String> {
    // 1. Декодируем заголовок, чтобы получить kid (Key ID)
    let header = decode_header(token).map_err(|_| "Invalid header")?;
    let kid = header.kid.ok_or("Missing kid in header")?;

    // 2. Получаем публичные ключи Google (из кеша или запросом)
    let jwks_body = if let Some(cached) = cache.get("google_jwks").await {
        cached
    } else {
        let res = reqwest::get("https://www.googleapis.com/oauth2/v3/certs")
            .await.map_err(|_| "Failed to fetch Google keys")?
            .text().await.map_err(|_| "Failed to read Google keys")?;
        cache.insert("google_jwks".to_string(), res.clone()).await;
        res
    };

    let jwks: GoogleJwks = serde_json::from_str(&jwks_body).map_err(|_| "Failed to parse JWKS")?;

    // 3. Ищем нужный ключ по kid
    let key_data = (&jwks.keys).into_iter().find(|k| k.kid == kid)
        .ok_or_else(|| format!("Key ID {} not found", kid))?;

    // 4. Настраиваем валидацию
    let mut validation = Validation::new(Algorithm::RS256);
    validation.set_audience(&[web_client_id]);
    validation.set_issuer(&["https://accounts.google.com", "accounts.google.com"]);

    // 5. Декодируем и проверяем подпись
    let decoding_key = DecodingKey::from_rsa_components(&key_data.n, &key_data.e)
        .map_err(|_| "Failed to create decoding key")?;

    let token_data = decode::<GoogleTokenPayload>(token, &decoding_key, &validation)
        .map_err(|e| format!("Token validation failed: {}", e))?;

    Ok(token_data.claims)
}
pub fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub fn calculate_expiration() -> NaiveDateTime {
    let datetime_utc = Utc::now() + Duration::days(2);
    datetime_utc.naive_utc()
}
