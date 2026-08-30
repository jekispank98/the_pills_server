use crate::error::{ErrorMessage, HttpError};
use crate::state::AppState;
use crate::utils::token;
use axum::extract::FromRequestParts;
use axum::http::header;
use axum::http::request::Parts;
use axum::http::HeaderMap;
use axum_extra::extract::cookie::CookieJar;
use std::sync::Arc;
use uuid::Uuid;

/// Id текущего пользователя, извлечённый и проверенный из сессионного JWT.
///
/// До этого экстрактора в проекте не было ни одного защищённого эндпоинта:
/// `token::decode_token` существовал, но нигде не вызывался. Источник токена —
/// cookie `token` (её выставляет `session_cookie_headers` при login/verify),
/// либо заголовок `Authorization: Bearer <token>` — мобильный клиент не гоняет
/// cookies автоматически.
pub struct CurrentUser(pub Uuid);

/// Достаёт сырой токен из cookie или заголовка. Отдельная чистая функция —
/// чтобы протестировать логику поиска токена без поднятия `AppState`/БД,
/// которых требует `FromRequestParts`.
fn extract_raw_token(headers: &HeaderMap) -> Option<String> {
    CookieJar::from_headers(headers)
        .get("token")
        .map(|cookie| cookie.value().to_string())
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.strip_prefix("Bearer "))
                .map(str::to_string)
        })
}

impl<S> FromRequestParts<S> for CurrentUser
where
    S: Send + Sync,
{
    type Rejection = HttpError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let app_state = parts
            .extensions
            .get::<Arc<AppState>>()
            .cloned()
            .ok_or_else(|| HttpError::server_error(ErrorMessage::ServerError))?;

        let raw_token = extract_raw_token(&parts.headers)
            .ok_or_else(|| HttpError::from(ErrorMessage::TokenNotProvided))?;

        let subject = token::decode_token(raw_token, app_state.config.jwt_secret.as_bytes())?;
        let user_id =
            Uuid::parse_str(&subject).map_err(|_| HttpError::from(ErrorMessage::InvalidToken))?;

        Ok(CurrentUser(user_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn no_cookie_and_no_header_means_no_token() {
        assert_eq!(extract_raw_token(&HeaderMap::new()), None);
    }

    #[test]
    fn reads_token_from_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("token=abc.def.ghi; other=1"),
        );
        assert_eq!(extract_raw_token(&headers), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn falls_back_to_bearer_header_for_clients_without_cookies() {
        // Мобильный клиент не гоняет cookies автоматически — ему нужен этот путь.
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc.def.ghi"),
        );
        assert_eq!(extract_raw_token(&headers), Some("abc.def.ghi".to_string()));
    }

    #[test]
    fn cookie_takes_priority_over_header_when_both_present() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, HeaderValue::from_static("token=from-cookie"));
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer from-header"),
        );
        assert_eq!(extract_raw_token(&headers), Some("from-cookie".to_string()));
    }

    #[test]
    fn non_bearer_authorization_header_is_ignored() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Basic dXNlcjpwYXNz"),
        );
        assert_eq!(extract_raw_token(&headers), None);
    }
}
