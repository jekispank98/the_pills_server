use crate::db::{
    LinkPersonsOutcome, MedicationRepositoryTrait, MedicationUpsert, MedicationUpsertOutcome,
};
use crate::dtos::{LinkPersonsRequest, MedicationDto, UpsertMedicationRequest};
use crate::error::HttpError;
use crate::extractors::{CurrentUser, JsonBody};
use crate::state::AppState;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{Router, get};
use axum::{Extension, Json};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

pub(crate) enum UpsertMedicationResponse {
    Ok(MedicationDto),
    Conflict(MedicationDto),
}

impl IntoResponse for UpsertMedicationResponse {
    fn into_response(self) -> Response {
        match self {
            UpsertMedicationResponse::Ok(dto) => (StatusCode::OK, Json(dto)).into_response(),
            UpsertMedicationResponse::Conflict(dto) => {
                (StatusCode::CONFLICT, Json(dto)).into_response()
            }
        }
    }
}

pub fn medications_router() -> Router {
    Router::new()
        .route("/", get(list_medications))
        .route(
            "/{id}",
            get(get_medication_by_id)
                .put(upsert_medication)
                .delete(delete_medication),
        )
        .route(
            "/{id}/persons",
            get(get_persons_for_medication).put(link_persons_to_medication),
        )
}

pub async fn list_medications(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<MedicationDto>>, HttpError> {
    let medications = app_state.db_client.list_medications(user_id).await?;
    Ok(Json(
        medications
            .into_iter()
            .map(|(medication, person_ids)| MedicationDto::with_person_ids(medication, person_ids))
            .collect(),
    ))
}

pub async fn get_medication_by_id(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(medication_id): Path<Uuid>,
) -> Result<Json<MedicationDto>, HttpError> {
    let medication = app_state
        .db_client
        .get_medication(medication_id)
        .await?
        .filter(|medication| medication.user_account_id == user_id)
        .ok_or_else(|| HttpError::new("Medication not found", StatusCode::NOT_FOUND))?;

    Ok(Json(MedicationDto::from(medication)))
}

pub async fn upsert_medication(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(medication_id): Path<Uuid>,
    JsonBody(body): JsonBody<UpsertMedicationRequest>,
) -> Result<UpsertMedicationResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let upsert = MedicationUpsert {
        id: medication_id,
        name: body.name.unwrap(),
        dosage: body.dosage.unwrap(),
        form: body.form,
        instruction: body.instruction,
        color_tag: body.color_tag.unwrap(),
        base_version: body.base_version,
    };

    match app_state
        .db_client
        .upsert_medication(user_id, upsert)
        .await?
    {
        MedicationUpsertOutcome::Ok(medication) => Ok(UpsertMedicationResponse::Ok(
            MedicationDto::from(medication),
        )),
        MedicationUpsertOutcome::VersionConflict(current) => Ok(
            UpsertMedicationResponse::Conflict(MedicationDto::from(current)),
        ),
        MedicationUpsertOutcome::OwnedByAnotherUser => Err(HttpError::new(
            "This medication id belongs to another account",
            StatusCode::FORBIDDEN,
        )),
    }
}

pub async fn delete_medication(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(medication_id): Path<Uuid>,
) -> Result<StatusCode, HttpError> {
    let deleted = app_state
        .db_client
        .delete_medication(medication_id, user_id)
        .await?;

    if !deleted {
        return Err(HttpError::new(
            "Medication not found",
            StatusCode::NOT_FOUND,
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn link_persons_to_medication(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(medication_id): Path<Uuid>,
    JsonBody(body): JsonBody<LinkPersonsRequest>,
) -> Result<StatusCode, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    match app_state
        .db_client
        .link_persons_to_medication(medication_id, user_id, &body.person_ids)
        .await?
    {
        LinkPersonsOutcome::Ok => Ok(StatusCode::NO_CONTENT),
        LinkPersonsOutcome::MedicationNotOwned => Err(HttpError::new(
            "Medication not found",
            StatusCode::NOT_FOUND,
        )),
        LinkPersonsOutcome::SomePersonsNotOwned => Err(HttpError::new(
            "One or more persons do not belong to your account",
            StatusCode::FORBIDDEN,
        )),
    }
}

pub async fn get_persons_for_medication(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(medication_id): Path<Uuid>,
) -> Result<Json<Vec<crate::dtos::PersonDto>>, HttpError> {
    app_state
        .db_client
        .get_medication(medication_id)
        .await?
        .filter(|medication| medication.user_account_id == user_id)
        .ok_or_else(|| HttpError::new("Medication not found", StatusCode::NOT_FOUND))?;

    let persons = app_state
        .db_client
        .get_persons_for_medication(medication_id)
        .await?;

    Ok(Json(
        persons
            .into_iter()
            .map(crate::dtos::PersonDto::from)
            .collect(),
    ))
}

pub async fn get_medications_for_person(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(person_id): Path<Uuid>,
) -> Result<Json<Vec<MedicationDto>>, HttpError> {
    let medications = app_state
        .db_client
        .get_medications_for_person(person_id, user_id)
        .await?;

    Ok(Json(
        medications.into_iter().map(MedicationDto::from).collect(),
    ))
}
