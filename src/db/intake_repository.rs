use super::{DbClient, UpsertRoute};
use crate::models::intake::{Intake, IntakeStatus};
use chrono::NaiveDateTime;
use sqlx::{Error, query_as};
use uuid::Uuid;

const INTAKE_COLUMNS: &str = "id, user_account_id, medication_id, time, pre_time, \
    is_completed, is_passed, is_canceled, created_at, updated_at, version";

#[derive(Debug, Clone)]
pub struct IntakeUpsert {
    pub id: Uuid,
    pub medication_id: Uuid,
    pub time: NaiveDateTime,
    pub pre_time: Option<NaiveDateTime>,
    pub is_completed: bool,
    pub is_passed: bool,
    pub is_canceled: bool,
    pub person_ids: Vec<Uuid>,
    pub base_version: i32,
}

pub enum IntakeUpsertOutcome {
    Ok(Intake, Vec<Uuid>),
    OwnedByAnotherUser,
    VersionConflict(Intake, Vec<Uuid>),
    MedicationNotOwned,
    SomePersonsNotOwned,
}

fn intake_status_flags(status: IntakeStatus) -> (Option<bool>, Option<bool>, Option<bool>) {
    match status {
        IntakeStatus::Passed => (None, Some(true), None),
        IntakeStatus::Canceled => (Some(false), Some(true), Some(true)),
        IntakeStatus::Completed => (Some(true), Some(true), Some(false)),
        IntakeStatus::Uncompleted => (Some(false), Some(false), Some(false)),
    }
}

#[async_trait::async_trait]
pub trait IntakeRepositoryTrait {
    async fn list_intakes(&self, user_account_id: Uuid) -> Result<Vec<(Intake, Vec<Uuid>)>, Error>;

    async fn get_intake(&self, id: Uuid) -> Result<Option<(Intake, Vec<Uuid>)>, Error>;

    async fn upsert_intake_with_persons(
        &self,
        user_account_id: Uuid,
        intake: IntakeUpsert,
    ) -> Result<IntakeUpsertOutcome, Error>;

    async fn delete_intake(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error>;

    async fn set_intake_status(
        &self,
        id: Uuid,
        user_account_id: Uuid,
        status: IntakeStatus,
    ) -> Result<Option<(Intake, Vec<Uuid>)>, Error>;
}

impl DbClient {
    async fn get_person_ids_for_intake(&self, intake_id: Uuid) -> Result<Vec<Uuid>, Error> {
        sqlx::query_scalar("SELECT person_id FROM intake_person_cross_ref WHERE intake_id = $1")
            .bind(intake_id)
            .fetch_all(&self.pool)
            .await
    }
}

#[async_trait::async_trait]
impl IntakeRepositoryTrait for DbClient {
    async fn list_intakes(&self, user_account_id: Uuid) -> Result<Vec<(Intake, Vec<Uuid>)>, Error> {
        let query = format!(
            "SELECT {INTAKE_COLUMNS} FROM intakes WHERE user_account_id = $1 ORDER BY time"
        );

        let intakes = query_as::<_, Intake>(&query)
            .bind(user_account_id)
            .fetch_all(&self.pool)
            .await?;

        let mut result = Vec::with_capacity(intakes.len());
        for intake in intakes {
            let person_ids = self.get_person_ids_for_intake(intake.id).await?;
            result.push((intake, person_ids));
        }
        Ok(result)
    }

    async fn get_intake(&self, id: Uuid) -> Result<Option<(Intake, Vec<Uuid>)>, Error> {
        let query = format!("SELECT {INTAKE_COLUMNS} FROM intakes WHERE id = $1");

        let intake = query_as::<_, Intake>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?;

        match intake {
            Some(intake) => {
                let person_ids = self.get_person_ids_for_intake(intake.id).await?;
                Ok(Some((intake, person_ids)))
            }
            None => Ok(None),
        }
    }

    async fn upsert_intake_with_persons(
        &self,
        user_account_id: Uuid,
        intake: IntakeUpsert,
    ) -> Result<IntakeUpsertOutcome, Error> {
        let mut tx = self.pool.begin().await?;

        let medication_owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_account_id FROM medications WHERE id = $1")
                .bind(intake.medication_id)
                .fetch_optional(&mut *tx)
                .await?;
        if medication_owner != Some(user_account_id) {
            tx.rollback().await?;
            return Ok(IntakeUpsertOutcome::MedicationNotOwned);
        }

        let owned_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persons WHERE id = ANY($1) AND user_account_id = $2",
        )
        .bind(&intake.person_ids)
        .bind(user_account_id)
        .fetch_one(&mut *tx)
        .await?;
        if owned_count as usize != intake.person_ids.len() {
            tx.rollback().await?;
            return Ok(IntakeUpsertOutcome::SomePersonsNotOwned);
        }

        let existing_owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_account_id FROM intakes WHERE id = $1")
                .bind(intake.id)
                .fetch_optional(&mut *tx)
                .await?;

        let saved = match super::decide_upsert_route(existing_owner, user_account_id) {
            UpsertRoute::OwnedByAnotherUser => {
                tx.rollback().await?;
                return Ok(IntakeUpsertOutcome::OwnedByAnotherUser);
            }
            UpsertRoute::Insert => {
                let query = format!(
                    r#"
    INSERT INTO intakes(id, user_account_id, medication_id, time, pre_time,
                         is_completed, is_passed, is_canceled)
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    RETURNING {INTAKE_COLUMNS}
"#
                );

                query_as::<_, Intake>(&query)
                    .bind(intake.id)
                    .bind(user_account_id)
                    .bind(intake.medication_id)
                    .bind(intake.time)
                    .bind(intake.pre_time)
                    .bind(intake.is_completed)
                    .bind(intake.is_passed)
                    .bind(intake.is_canceled)
                    .fetch_one(&mut *tx)
                    .await?
            }
            UpsertRoute::UpdateIfVersionMatches => {
                let update_query = format!(
                    r#"
    UPDATE intakes SET
        medication_id = $2, time = $3, pre_time = $4,
        is_completed = $5, is_passed = $6, is_canceled = $7,
        version = version + 1, updated_at = NOW()
    WHERE id = $1 AND version = $8
    RETURNING {INTAKE_COLUMNS}
"#
                );

                let updated = query_as::<_, Intake>(&update_query)
                    .bind(intake.id)
                    .bind(intake.medication_id)
                    .bind(intake.time)
                    .bind(intake.pre_time)
                    .bind(intake.is_completed)
                    .bind(intake.is_passed)
                    .bind(intake.is_canceled)
                    .bind(intake.base_version)
                    .fetch_optional(&mut *tx)
                    .await?;

                match updated {
                    Some(saved) => saved,
                    None => {
                        let current_query =
                            format!("SELECT {INTAKE_COLUMNS} FROM intakes WHERE id = $1");
                        let current = query_as::<_, Intake>(&current_query)
                            .bind(intake.id)
                            .fetch_one(&mut *tx)
                            .await?;
                        let current_persons: Vec<Uuid> = sqlx::query_scalar(
                            "SELECT person_id FROM intake_person_cross_ref WHERE intake_id = $1",
                        )
                        .bind(intake.id)
                        .fetch_all(&mut *tx)
                        .await?;
                        tx.rollback().await?;
                        return Ok(IntakeUpsertOutcome::VersionConflict(
                            current,
                            current_persons,
                        ));
                    }
                }
            }
        };

        sqlx::query("DELETE FROM intake_person_cross_ref WHERE intake_id = $1")
            .bind(saved.id)
            .execute(&mut *tx)
            .await?;
        for person_id in &intake.person_ids {
            sqlx::query(
                "INSERT INTO intake_person_cross_ref (intake_id, person_id) VALUES ($1, $2)",
            )
            .bind(saved.id)
            .bind(person_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(IntakeUpsertOutcome::Ok(saved, intake.person_ids))
    }

    async fn delete_intake(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM intakes WHERE id = $1 AND user_account_id = $2")
            .bind(id)
            .bind(user_account_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_intake_status(
        &self,
        id: Uuid,
        user_account_id: Uuid,
        status: IntakeStatus,
    ) -> Result<Option<(Intake, Vec<Uuid>)>, Error> {
        let (is_completed, is_passed, is_canceled) = intake_status_flags(status);

        let mut sets = Vec::new();
        let mut binds: Vec<bool> = Vec::new();
        for (column, value) in [
            ("is_completed", is_completed),
            ("is_passed", is_passed),
            ("is_canceled", is_canceled),
        ] {
            if let Some(v) = value {
                binds.push(v);
                sets.push(format!("{column} = ${}", binds.len()));
            }
        }
        sets.push("version = version + 1".to_string());
        sets.push("updated_at = NOW()".to_string());

        let query = format!(
            "UPDATE intakes SET {} WHERE id = ${} AND user_account_id = ${} RETURNING {INTAKE_COLUMNS}",
            sets.join(", "),
            binds.len() + 1,
            binds.len() + 2
        );

        let mut db_query = query_as::<_, Intake>(&query);
        for bind in binds {
            db_query = db_query.bind(bind);
        }
        db_query = db_query.bind(id).bind(user_account_id);

        match db_query.fetch_optional(&self.pool).await? {
            Some(intake) => {
                let person_ids = self.get_person_ids_for_intake(intake.id).await?;
                Ok(Some((intake, person_ids)))
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intake_status_flags_passed_only_touches_is_passed() {
        assert_eq!(
            intake_status_flags(IntakeStatus::Passed),
            (None, Some(true), None)
        );
    }

    #[test]
    fn intake_status_flags_canceled_resets_completed() {
        assert_eq!(
            intake_status_flags(IntakeStatus::Canceled),
            (Some(false), Some(true), Some(true))
        );
    }

    #[test]
    fn intake_status_flags_completed_resets_canceled() {
        assert_eq!(
            intake_status_flags(IntakeStatus::Completed),
            (Some(true), Some(true), Some(false))
        );
    }

    #[test]
    fn intake_status_flags_uncompleted_clears_everything() {
        assert_eq!(
            intake_status_flags(IntakeStatus::Uncompleted),
            (Some(false), Some(false), Some(false))
        );
    }
}
