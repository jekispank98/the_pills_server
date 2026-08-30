use crate::db::{AuthentificationTrait, UserProfileUpdate};
use crate::dtos::{UpdateUserRequest, UserDto};
use crate::error::{ErrorMessage, HttpError};
use crate::extractors::CurrentUser;
use crate::state::AppState;
use axum::http::StatusCode;
use axum::routing::{get, Router};
use axum::{Extension, Json};
use std::sync::Arc;
use validator::Validate;

pub fn users_router() -> Router {
    Router::new().route("/me", get(get_me).patch(update_me).delete(delete_me))
}

pub async fn get_me(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<Json<UserDto>, HttpError> {
    let user = app_state
        .db_client
        .get_user(Some(user_id), None, None, None)
        .await?
        .ok_or_else(|| HttpError::from(ErrorMessage::UserNoLongerExist))?;

    Ok(Json(UserDto::from(user)))
}

pub async fn update_me(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Json(body): Json<UpdateUserRequest>,
) -> Result<Json<UserDto>, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let update = UserProfileUpdate {
        name: body.name,
        photo_url: body.photo_url,
        time_zone: body.time_zone,
        locale: body.locale,
    };

    let user = app_state
        .db_client
        .update_user_profile(user_id, update)
        .await?
        .ok_or_else(|| HttpError::from(ErrorMessage::UserNoLongerExist))?;

    Ok(Json(UserDto::from(user)))
}

/// Каскадно удаляет и `persons` этого аккаунта — `ON DELETE CASCADE` на
/// `persons.user_account_id` (см. миграцию).
pub async fn delete_me(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<StatusCode, HttpError> {
    let deleted = app_state.db_client.delete_user_by_id(user_id).await?;
    if !deleted {
        return Err(HttpError::from(ErrorMessage::UserNoLongerExist));
    }
    Ok(StatusCode::NO_CONTENT)
}
