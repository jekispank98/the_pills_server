use crate::db::{PersonRepositoryTrait, PersonUpsert, PersonUpsertOutcome};
use crate::dtos::{PersonDto, UpsertPersonRequest};
use crate::error::HttpError;
use crate::extractors::CurrentUser;
use crate::state::AppState;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::routing::{get, put, Router};
use axum::{Extension, Json};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

pub fn persons_router() -> Router {
    Router::new()
        .route("/", get(list_persons))
        .route(
            "/{id}",
            get(get_person_by_id).put(upsert_person).delete(delete_person),
        )
}

pub async fn list_persons(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<PersonDto>>, HttpError> {
    let persons = app_state.db_client.list_persons(user_id).await?;
    Ok(Json(persons.into_iter().map(PersonDto::from).collect()))
}

/// 404 и для «не существует», и для «существует, но не ваш» — чтобы нельзя
/// было перебором id узнавать, какие persons есть у чужих аккаунтов.
pub async fn get_person_by_id(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(person_id): Path<Uuid>,
) -> Result<Json<PersonDto>, HttpError> {
    let person = app_state
        .db_client
        .get_person(person_id)
        .await?
        .filter(|person| person.user_account_id == user_id)
        .ok_or_else(|| HttpError::new("Person not found", StatusCode::NOT_FOUND))?;

    Ok(Json(PersonDto::from(person)))
}

/// Идемпотентный upsert по client-generated `id` из пути — offline-first:
/// клиент создаёт `Person` локально и синкает её этим же запросом позже.
pub async fn upsert_person(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(person_id): Path<Uuid>,
    Json(body): Json<UpsertPersonRequest>,
) -> Result<Json<PersonDto>, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let upsert = PersonUpsert {
        id: person_id,
        name: body.name.unwrap(),
        relation: body.relation,
        birth_date: body.birth_date,
        gender: body.gender,
        weight_kg: body.weight_kg,
        color_tag: body.color_tag,
        notes: body.notes,
        is_active: body.is_active,
        photo_url: body.photo_url,
    };

    match app_state.db_client.upsert_person(user_id, upsert).await? {
        PersonUpsertOutcome::Ok(person) => Ok(Json(PersonDto::from(person))),
        // Кто-то другой уже занял этот id (крайне маловероятное совпадение UUID,
        // но лучше явная 403, чем молча перезаписать/подсунуть чужую запись).
        PersonUpsertOutcome::OwnedByAnotherUser => Err(HttpError::new(
            "This person id belongs to another account",
            StatusCode::FORBIDDEN,
        )),
    }
}

/// Как и в Android `ProfileViewModel`, SELF-профиль (`is_user = true`) удалить
/// нельзя — проверка задублирована на сервере, а не только на клиенте.
pub async fn delete_person(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(person_id): Path<Uuid>,
) -> Result<StatusCode, HttpError> {
    let deleted = app_state
        .db_client
        .delete_person(person_id, user_id)
        .await?;

    if !deleted {
        return Err(HttpError::new(
            "Person not found, not yours, or is your own profile",
            StatusCode::NOT_FOUND,
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}
