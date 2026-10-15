use crate::db::{IntakeRepositoryTrait, IntakeUpsert, IntakeUpsertOutcome};
use crate::dtos::{IntakeDto, IntakeStatusRequest, UpsertIntakeRequest};
use crate::error::HttpError;
use crate::extractors::{CurrentUser, JsonBody};
use crate::models::intake::IntakeStatus;
use crate::state::AppState;
use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, Router};
use axum::{Extension, Json};
use std::sync::Arc;
use uuid::Uuid;
use validator::Validate;

pub(crate) enum UpsertIntakeResponse {
    Ok(IntakeDto),
    Conflict(IntakeDto),
}

impl IntoResponse for UpsertIntakeResponse {
    fn into_response(self) -> Response {
        match self {
            UpsertIntakeResponse::Ok(dto) => (StatusCode::OK, Json(dto)).into_response(),
            UpsertIntakeResponse::Conflict(dto) => (StatusCode::CONFLICT, Json(dto)).into_response(),
        }
    }
}

pub fn intakes_router() -> Router {
    Router::new()
        .route("/", get(list_intakes))
        .route(
            "/{id}",
            get(get_intake_by_id).put(upsert_intake).delete(delete_intake),
        )
        .route("/{id}/status", axum::routing::patch(set_intake_status))
}

pub async fn list_intakes(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
) -> Result<Json<Vec<IntakeDto>>, HttpError> {
    let intakes = app_state.db_client.list_intakes(user_id).await?;
    Ok(Json(
        intakes
            .into_iter()
            .map(|(intake, person_ids)| IntakeDto::from_intake(intake, person_ids))
            .collect(),
    ))
}

pub async fn get_intake_by_id(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(intake_id): Path<Uuid>,
) -> Result<Json<IntakeDto>, HttpError> {
    let (intake, person_ids) = app_state
        .db_client
        .get_intake(intake_id)
        .await?
        .filter(|(intake, _)| intake.user_account_id == user_id)
        .ok_or_else(|| HttpError::new("Intake not found", StatusCode::NOT_FOUND))?;

    Ok(Json(IntakeDto::from_intake(intake, person_ids)))
}

pub async fn upsert_intake(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(intake_id): Path<Uuid>,
    JsonBody(body): JsonBody<UpsertIntakeRequest>,
) -> Result<UpsertIntakeResponse, HttpError> {
    body.validate()
        .map_err(|e| HttpError::bad_request(e.to_string()))?;

    let upsert = IntakeUpsert {
        id: intake_id,
        medication_id: body.medication_id.unwrap(),
        time: body.time.unwrap(),
        pre_time: body.pre_time,
        is_completed: body.is_completed,
        is_passed: body.is_passed,
        is_canceled: body.is_canceled,
        person_ids: body.person_ids,
        base_version: body.base_version,
    };

    match app_state.db_client.upsert_intake_with_persons(user_id, upsert).await? {
        IntakeUpsertOutcome::Ok(intake, person_ids) => {
            Ok(UpsertIntakeResponse::Ok(IntakeDto::from_intake(intake, person_ids)))
        }
        IntakeUpsertOutcome::VersionConflict(intake, person_ids) => Ok(
            UpsertIntakeResponse::Conflict(IntakeDto::from_intake(intake, person_ids)),
        ),
        IntakeUpsertOutcome::OwnedByAnotherUser => Err(HttpError::new(
            "This intake id belongs to another account",
            StatusCode::FORBIDDEN,
        )),
        IntakeUpsertOutcome::MedicationNotOwned => Err(HttpError::new(
            "Medication not found",
            StatusCode::NOT_FOUND,
        )),
        IntakeUpsertOutcome::SomePersonsNotOwned => Err(HttpError::new(
            "One or more persons do not belong to your account",
            StatusCode::FORBIDDEN,
        )),
    }
}

pub async fn delete_intake(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(intake_id): Path<Uuid>,
) -> Result<StatusCode, HttpError> {
    let deleted = app_state.db_client.delete_intake(intake_id, user_id).await?;

    if !deleted {
        return Err(HttpError::new("Intake not found", StatusCode::NOT_FOUND));
    }

    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_intake_status(
    CurrentUser(user_id): CurrentUser,
    Extension(app_state): Extension<Arc<AppState>>,
    Path(intake_id): Path<Uuid>,
    JsonBody(body): JsonBody<IntakeStatusRequest>,
) -> Result<Json<IntakeDto>, HttpError> {
    let status = match body {
        IntakeStatusRequest::Passed => IntakeStatus::Passed,
        IntakeStatusRequest::Canceled => IntakeStatus::Canceled,
        IntakeStatusRequest::Completed => IntakeStatus::Completed,
        IntakeStatusRequest::Uncompleted => IntakeStatus::Uncompleted,
    };

    let (intake, person_ids) = app_state
        .db_client
        .set_intake_status(intake_id, user_id, status)
        .await?
        .ok_or_else(|| HttpError::new("Intake not found", StatusCode::NOT_FOUND))?;

    Ok(Json(IntakeDto::from_intake(intake, person_ids)))
}
