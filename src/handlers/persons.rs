use crate::db::{PersonRepositoryTrait, PersonUpsert, PersonUpsertOutcome};
use crate::dtos::{PersonDto, UpsertPersonRequest};
use crate::error::HttpError;
use crate::extractors::{CurrentUser, JsonBody};
use crate::handlers::medications::get_medications_for_person;
use crate::state::AppState;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, put, Router};
use axum::{Extension, Json};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

pub(crate) enum UpsertPersonResponse {
    Ok(PersonDto),
    Conflict(PersonDto),
}

impl IntoResponse for UpsertPersonResponse {
    fn into_response(self) -> Response {
        match self {
            UpsertPersonResponse::Ok(dto) => (StatusCode::OK, Json(dto)).into_response(),
            UpsertPersonResponse::Conflict(dto) => {
                (StatusCode::CONFLICT, Json(dto)).into_response()
            }
        }
    }
}

pub fn persons_router() -> Router {
    Router::new()
        .route("/", get(list_persons))
        .route(
            "/{id}",
            get(get_person_by_id).put(upsert_person).delete(delete_person),
        )
        .route("/{id}/medications", get(get_medications_for_person))
}

pub async fn list_persons(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<PersonDto>>, HttpError> {
    let persons = app_state.db_client.list_persons(user_id).await?;
    Ok(Json(persons.into_iter().map(PersonDto::from).collect()))
}

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

pub async fn upsert_person(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(person_id): Path<Uuid>,
    JsonBody(body): JsonBody<UpsertPersonRequest>,
) -> Result<UpsertPersonResponse, HttpError> {
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
        base_version: body.base_version,
    };

    match app_state.db_client.upsert_person(user_id, upsert).await? {
        PersonUpsertOutcome::Ok(person) => Ok(UpsertPersonResponse::Ok(PersonDto::from(person))),
        PersonUpsertOutcome::VersionConflict(current) => {
            Ok(UpsertPersonResponse::Conflict(PersonDto::from(current)))
        }
        PersonUpsertOutcome::OwnedByAnotherUser => Err(HttpError::new(
            "This person id belongs to another account",
            StatusCode::FORBIDDEN,
        )),
    }
}

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
