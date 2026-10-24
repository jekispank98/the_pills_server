use super::{DbClient, UpsertRoute};
use crate::models::person::{Gender, Person, PersonRelation};
use chrono::NaiveDateTime;
use sqlx::{query_as, Error};
use uuid::Uuid;

pub(super) const PERSON_COLUMNS: &str = "id, user_account_id, name, relation, is_user, birth_date, gender, \
    weight_kg, color_tag, notes, is_active, photo_url, created_at, updated_at, version";

#[derive(Debug, Clone)]
pub struct PersonUpsert {
    pub id: Uuid,
    pub name: String,
    pub relation: PersonRelation,
    pub birth_date: Option<NaiveDateTime>,
    pub gender: Gender,
    pub weight_kg: Option<f64>,
    pub color_tag: Option<i64>,
    pub notes: Option<String>,
    pub is_active: bool,
    pub photo_url: Option<String>,
    pub base_version: i32,
}

pub enum PersonUpsertOutcome {
    Ok(Person),
    OwnedByAnotherUser,
    VersionConflict(Person),
}

#[async_trait::async_trait]
pub trait PersonRepositoryTrait {
    async fn list_persons(&self, user_account_id: Uuid) -> Result<Vec<Person>, Error>;

    async fn get_person(&self, id: Uuid) -> Result<Option<Person>, Error>;

    async fn upsert_person(
        &self,
        user_account_id: Uuid,
        person: PersonUpsert,
    ) -> Result<PersonUpsertOutcome, Error>;

    async fn delete_person(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error>;
}

#[async_trait::async_trait]
impl PersonRepositoryTrait for DbClient {
    async fn list_persons(&self, user_account_id: Uuid) -> Result<Vec<Person>, Error> {
        let query =
            format!("SELECT {PERSON_COLUMNS} FROM persons WHERE user_account_id = $1 ORDER BY created_at");

        query_as::<_, Person>(&query)
            .bind(user_account_id)
            .fetch_all(&self.pool)
            .await
    }

    async fn get_person(&self, id: Uuid) -> Result<Option<Person>, Error> {
        let query = format!("SELECT {PERSON_COLUMNS} FROM persons WHERE id = $1");

        query_as::<_, Person>(&query)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    async fn upsert_person(
        &self,
        user_account_id: Uuid,
        person: PersonUpsert,
    ) -> Result<PersonUpsertOutcome, Error> {
        let mut tx = self.pool.begin().await?;

        let existing_owner: Option<Uuid> =
            sqlx::query_scalar("SELECT user_account_id FROM persons WHERE id = $1")
                .bind(person.id)
                .fetch_optional(&mut *tx)
                .await?;

        match super::decide_upsert_route(existing_owner, user_account_id) {
            UpsertRoute::OwnedByAnotherUser => {
                tx.rollback().await?;
                Ok(PersonUpsertOutcome::OwnedByAnotherUser)
            }
            UpsertRoute::Insert => {
                let query = format!(
                    r#"
    INSERT INTO persons(id, user_account_id, name, relation, is_user, birth_date, gender,
                         weight_kg, color_tag, notes, is_active, photo_url)
    VALUES ($1, $2, $3, $4, false, $5, $6, $7, $8, $9, $10, $11)
    RETURNING {PERSON_COLUMNS}
"#
                );

                let saved = query_as::<_, Person>(&query)
                    .bind(person.id)
                    .bind(user_account_id)
                    .bind(person.name)
                    .bind(person.relation)
                    .bind(person.birth_date)
                    .bind(person.gender)
                    .bind(person.weight_kg)
                    .bind(person.color_tag)
                    .bind(person.notes)
                    .bind(person.is_active)
                    .bind(person.photo_url)
                    .fetch_one(&mut *tx)
                    .await?;

                tx.commit().await?;
                Ok(PersonUpsertOutcome::Ok(saved))
            }
            UpsertRoute::UpdateIfVersionMatches => {
                let update_query = format!(
                    r#"
    UPDATE persons SET
        name = $2, relation = $3, birth_date = $4, gender = $5, weight_kg = $6,
        color_tag = $7, notes = $8, is_active = $9, photo_url = $10,
        version = version + 1, updated_at = NOW()
    WHERE id = $1 AND version = $11
    RETURNING {PERSON_COLUMNS}
"#
                );

                let updated = query_as::<_, Person>(&update_query)
                    .bind(person.id)
                    .bind(&person.name)
                    .bind(person.relation)
                    .bind(person.birth_date)
                    .bind(person.gender)
                    .bind(person.weight_kg)
                    .bind(person.color_tag)
                    .bind(&person.notes)
                    .bind(person.is_active)
                    .bind(&person.photo_url)
                    .bind(person.base_version)
                    .fetch_optional(&mut *tx)
                    .await?;

                match updated {
                    Some(saved) => {
                        tx.commit().await?;
                        Ok(PersonUpsertOutcome::Ok(saved))
                    }
                    None => {
                        let current_query = format!("SELECT {PERSON_COLUMNS} FROM persons WHERE id = $1");
                        let current = query_as::<_, Person>(&current_query)
                            .bind(person.id)
                            .fetch_one(&mut *tx)
                            .await?;
                        tx.rollback().await?;
                        Ok(PersonUpsertOutcome::VersionConflict(current))
                    }
                }
            }
        }
    }

    async fn delete_person(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error> {
        let query = "DELETE FROM persons WHERE id = $1 AND user_account_id = $2 AND is_user = false";

        let result = sqlx::query(query)
            .bind(id)
            .bind(user_account_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}