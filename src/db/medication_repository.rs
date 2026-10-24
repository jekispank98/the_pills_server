use super::{DbClient, UpsertRoute};
use crate::models::medication::{Medication, MedicationForm};
use crate::models::person::Person;
use sqlx::{Error, query_as};
use std::collections::HashMap;
use uuid::Uuid;

const MEDICATION_COLUMNS: &str = "id, user_account_id, name, dosage, form, instruction, \
    color_tag, created_at, updated_at, version";

#[derive(Debug, Clone)]
pub struct MedicationUpsert {
    pub id: Uuid,
    pub name: String,
    pub dosage: f64,
    pub form: MedicationForm,
    pub instruction: String,
    pub color_tag: String,
    pub base_version: i32,
}

pub enum MedicationUpsertOutcome {
    Ok(Medication),
    OwnedByAnotherUser,
    VersionConflict(Medication),
}

pub enum LinkPersonsOutcome {
    Ok,
    MedicationNotOwned,
    SomePersonsNotOwned,
}

#[async_trait::async_trait]
pub trait MedicationRepositoryTrait {
    async fn list_medications(
        &self,
        user_account_id: Uuid,
    ) -> Result<Vec<(Medication, Vec<Uuid>)>, Error>;

    async fn get_medication(&self, id: Uuid) -> Result<Option<Medication>, Error>;

    async fn upsert_medication(
        &self,
        user_account_id: Uuid,
        medication: MedicationUpsert,
    ) -> Result<MedicationUpsertOutcome, Error>;

    async fn delete_medication(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error>;

    async fn link_persons_to_medication(
        &self,
        medication_id: Uuid,
        user_account_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<LinkPersonsOutcome, Error>;

    async fn get_persons_for_medication(&self, medication_id: Uuid) -> Result<Vec<Person>, Error>;

    async fn get_medications_for_person(
        &self,
        person_id: Uuid,
        user_account_id: Uuid,
    ) -> Result<Vec<Medication>, Error>;
}

#[async_trait::async_trait]
impl MedicationRepositoryTrait for DbClient {
    async fn list_medications(
        &self,
        user_account_id: Uuid,
    ) -> Result<Vec<(Medication, Vec<Uuid>)>, Error> {
        let query = format!(
            "SELECT {MEDICATION_COLUMNS} FROM medications WHERE user_account_id = $1 ORDER BY name"
        );

        let medications = query_as::<_, Medication>(&query)
            .bind(user_account_id)
            .fetch_all(&self.pool)
            .await?;

        let links: Vec<(Uuid, Uuid)> = query_as(
            "SELECT x.medication_id, x.person_id FROM medication_person_cross_ref x \
             INNER JOIN medications m ON m.id = x.medication_id \
             WHERE m.user_account_id = $1",
        )
        .bind(user_account_id)
        .fetch_all(&self.pool)
        .await?;

        let mut person_ids_by_medication: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for (medication_id, person_id) in links {
            person_ids_by_medication
                .entry(medication_id)
                .or_default()
                .push(person_id);
        }

        Ok(medications
            .into_iter()
            .map(|medication| {
                let person_ids = person_ids_by_medication
                    .remove(&medication.id)
                    .unwrap_or_default();
                (medication, person_ids)
            })
            .collect())
    }

    async fn get_medication(&self, id: Uuid) -> Result<Option<Medication>, Error> {
        let query = format!("SELECT {MEDICATION_COLUMNS} FROM medications WHERE id = $1");

        query_as::<_, Medication>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    async fn upsert_medication(
        &self,
        user_account_id: Uuid,
        medication: MedicationUpsert,
    ) -> Result<MedicationUpsertOutcome, Error> {
        let mut tx = self.pool.begin().await?;

        let existing_owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_account_id FROM medications WHERE id = $1")
                .bind(medication.id)
                .fetch_optional(&mut *tx)
                .await?;

        match super::decide_upsert_route(existing_owner, user_account_id) {
            UpsertRoute::OwnedByAnotherUser => {
                tx.rollback().await?;
                Ok(MedicationUpsertOutcome::OwnedByAnotherUser)
            }
            UpsertRoute::Insert => {
                let query = format!(
                    r#"
    INSERT INTO medications(id, user_account_id, name, dosage, form, instruction, color_tag)
    VALUES ($1, $2, $3, $4, $5, $6, $7)
    RETURNING {MEDICATION_COLUMNS}
"#
                );

                let saved = query_as::<_, Medication>(&query)
                    .bind(medication.id)
                    .bind(user_account_id)
                    .bind(medication.name)
                    .bind(medication.dosage)
                    .bind(medication.form)
                    .bind(medication.instruction)
                    .bind(medication.color_tag)
                    .fetch_one(&mut *tx)
                    .await?;

                tx.commit().await?;
                Ok(MedicationUpsertOutcome::Ok(saved))
            }
            UpsertRoute::UpdateIfVersionMatches => {
                let update_query = format!(
                    r#"
    UPDATE medications SET
        name = $2, dosage = $3, form = $4, instruction = $5, color_tag = $6,
        version = version + 1, updated_at = NOW()
    WHERE id = $1 AND version = $7
    RETURNING {MEDICATION_COLUMNS}
"#
                );

                let updated = query_as::<_, Medication>(&update_query)
                    .bind(medication.id)
                    .bind(&medication.name)
                    .bind(medication.dosage)
                    .bind(medication.form)
                    .bind(&medication.instruction)
                    .bind(&medication.color_tag)
                    .bind(medication.base_version)
                    .fetch_optional(&mut *tx)
                    .await?;

                match updated {
                    Some(saved) => {
                        tx.commit().await?;
                        Ok(MedicationUpsertOutcome::Ok(saved))
                    }
                    None => {
                        let current_query =
                            format!("SELECT {MEDICATION_COLUMNS} FROM medications WHERE id = $1");
                        let current = query_as::<_, Medication>(&current_query)
                            .bind(medication.id)
                            .fetch_one(&mut *tx)
                            .await?;
                        tx.rollback().await?;
                        Ok(MedicationUpsertOutcome::VersionConflict(current))
                    }
                }
            }
        }
    }

    async fn delete_medication(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query("DELETE FROM medications WHERE id = $1 AND user_account_id = $2")
            .bind(id)
            .bind(user_account_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }

    async fn link_persons_to_medication(
        &self,
        medication_id: Uuid,
        user_account_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<LinkPersonsOutcome, Error> {
        let mut tx = self.pool.begin().await?;

        let medication_owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_account_id FROM medications WHERE id = $1")
                .bind(medication_id)
                .fetch_optional(&mut *tx)
                .await?;
        if medication_owner != Some(user_account_id) {
            tx.rollback().await?;
            return Ok(LinkPersonsOutcome::MedicationNotOwned);
        }

        let owned_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM persons WHERE id = ANY($1) AND user_account_id = $2",
        )
        .bind(person_ids)
        .bind(user_account_id)
        .fetch_one(&mut *tx)
        .await?;
        if owned_count as usize != person_ids.len() {
            tx.rollback().await?;
            return Ok(LinkPersonsOutcome::SomePersonsNotOwned);
        }

        for person_id in person_ids {
            sqlx::query(
                "INSERT INTO medication_person_cross_ref (medication_id, person_id) \
                 VALUES ($1, $2) ON CONFLICT (medication_id, person_id) DO NOTHING",
            )
            .bind(medication_id)
            .bind(person_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(LinkPersonsOutcome::Ok)
    }

    async fn get_persons_for_medication(&self, medication_id: Uuid) -> Result<Vec<Person>, Error> {
        let query = "SELECT p.* FROM persons p \
                     INNER JOIN medication_person_cross_ref x ON x.person_id = p.id \
                     WHERE x.medication_id = $1 ORDER BY p.created_at";

        query_as::<_, Person>(query)
            .bind(medication_id)
            .fetch_all(&self.pool)
            .await
    }

    async fn get_medications_for_person(
        &self,
        person_id: Uuid,
        user_account_id: Uuid,
    ) -> Result<Vec<Medication>, Error> {
        let query = "SELECT m.* FROM medications m \
                     INNER JOIN medication_person_cross_ref x ON x.medication_id = m.id \
                     WHERE x.person_id = $1 AND m.user_account_id = $2 ORDER BY m.name";

        query_as::<_, Medication>(query)
            .bind(person_id)
            .bind(user_account_id)
            .fetch_all(&self.pool)
            .await
    }
}
