use crate::models::intake::{Intake, IntakeStatus};
use crate::models::medication::{Medication, MedicationForm};
use crate::models::person::{Gender, Person, PersonRelation};
use crate::models::user::User;
use chrono::NaiveDateTime;
use sqlx::{query_as, Error, Pool, Postgres};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct DbClient {
    pool: Pool<Postgres>,
}

impl DbClient {
    pub fn new(pool: Pool<Postgres>) -> Self {
        DbClient { pool }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UserQueryFilters<'a> {
    pub user_id: Option<Uuid>,
    pub name: Option<&'a str>,
    pub email: Option<&'a str>,
    pub token: Option<&'a str>,
}

const USER_SELECT_QUERY: &str = r#"
    SELECT
        id,
        email,
        password,
        name,
        verification_token_expires_at,
        subscribed,
        verified,
        created_at,
        updated_at,
        verification_token,
        google_sub,
        photo_url,
        time_zone,
        locale
    FROM users
"#;

const PERSON_COLUMNS: &str = "id, user_account_id, name, relation, is_user, birth_date, gender, \
    weight_kg, color_tag, notes, is_active, photo_url, created_at, updated_at, version";

const MEDICATION_COLUMNS: &str = "id, user_account_id, name, dosage, form, instruction, \
    color_tag, created_at, updated_at, version";

const INTAKE_COLUMNS: &str = "id, user_account_id, medication_id, time, pre_time, \
    is_completed, is_passed, is_canceled, created_at, updated_at, version";

#[derive(Debug, Clone, Copy, PartialEq)]
enum FilterBind<'a> {
    Uuid(Uuid),
    Text(&'a str),
}

fn build_filter_query<'a>(filters: &UserQueryFilters<'a>) -> Option<(String, Vec<FilterBind<'a>>)> {
    let mut conditions = Vec::new();
    let mut binds: Vec<FilterBind<'a>> = Vec::new();

    if let Some(user_id) = filters.user_id {
        binds.push(FilterBind::Uuid(user_id));
        conditions.push(format!("id = ${}", binds.len()));
    }
    if let Some(name) = filters.name {
        binds.push(FilterBind::Text(name));
        conditions.push(format!("name = ${}", binds.len()));
    }
    if let Some(email) = filters.email {
        binds.push(FilterBind::Text(email));
        conditions.push(format!("email = ${}", binds.len()));
    }
    if let Some(token) = filters.token {
        binds.push(FilterBind::Text(token));
        conditions.push(format!("verification_token = ${}", binds.len()));
    }

    if conditions.is_empty() {
        return None;
    }

    Some((conditions.join(" AND "), binds))
}

#[derive(Debug, Clone, Default)]
pub struct UserProfileUpdate {
    pub name: Option<String>,
    pub photo_url: Option<String>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
}

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

#[async_trait::async_trait]
pub trait AuthentificationTrait {
    async fn get_user(
        &self,
        user_id: Option<Uuid>,
        name: Option<&str>,
        email: Option<&str>,
        token: Option<&str>,
    ) -> Result<Option<User>, Error>;

    async fn register_user_with_self_person(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<(User, Person), Error>;

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error>;

    async fn delete_user_by_id(&self, user_id: Uuid) -> Result<bool, Error>;

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error>;

    async fn update_verification_token_expires_at(
        &self,
        expires_at: NaiveDateTime,
        id: Uuid,
    ) -> Result<bool, Error>;

    async fn update_user_profile(
        &self,
        id: Uuid,
        update: UserProfileUpdate,
    ) -> Result<Option<User>, Error>;

    async fn find_user_with_filters(
        &self,
        filters: UserQueryFilters<'_>,
    ) -> Result<Option<User>, Error>;

    async fn verified_token(&self, token: &str) -> Result<(), Error>;
}

pub enum PersonUpsertOutcome {
    Ok(Person),
    OwnedByAnotherUser,
    VersionConflict(Person),
}

#[derive(Debug, PartialEq, Eq)]
enum UpsertRoute {
    Insert,
    UpdateIfVersionMatches,
    OwnedByAnotherUser,
}

fn decide_upsert_route(existing_owner: Option<Uuid>, requesting_user: Uuid) -> UpsertRoute {
    match existing_owner {
        None => UpsertRoute::Insert,
        Some(owner) if owner == requesting_user => UpsertRoute::UpdateIfVersionMatches,
        Some(_) => UpsertRoute::OwnedByAnotherUser,
    }
}

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
pub trait MedicationRepositoryTrait {
    async fn list_medications(&self, user_account_id: Uuid) -> Result<Vec<(Medication, Vec<Uuid>)>, Error>;

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
impl AuthentificationTrait for DbClient {
    async fn get_user(
        &self,
        user_id: Option<Uuid>,
        name: Option<&str>,
        email: Option<&str>,
        token: Option<&str>,
    ) -> Result<Option<User>, Error> {
        let filters = UserQueryFilters {
            user_id,
            name,
            email,
            token,
        };
        self.find_user_with_filters(filters).await
    }

    async fn register_user_with_self_person(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<(User, Person), Error> {
        let mut tx = self.pool.begin().await?;

        let user_query = r#"
    INSERT INTO users(name, email, password, verification_token_expires_at, subscribed, verification_token)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id, name, email, password, verification_token_expires_at, subscribed, verified,
              created_at, updated_at, verification_token, google_sub, photo_url, time_zone, locale"#;

        let user = query_as::<_, User>(user_query)
            .bind(&name)
            .bind(email)
            .bind(password)
            .bind(verification_token_expires_at)
            .bind(false)
            .bind(verification_token)
            .fetch_one(&mut *tx)
            .await?;

        let person_query = format!(
            r#"
    INSERT INTO persons(id, user_account_id, name, relation, is_user)
    VALUES ($1, $2, $3, $4, true)
    RETURNING {PERSON_COLUMNS}
"#
        );

        let person = query_as::<_, Person>(&person_query)
            .bind(Uuid::new_v4())
            .bind(user.id)
            .bind(&name)
            .bind(PersonRelation::Myself)
            .fetch_one(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok((user, person))
    }

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error> {
        let query = format!("{USER_SELECT_QUERY} WHERE email = $1");

        query_as::<_, User>(&query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }

    async fn delete_user_by_id(&self, user_id: Uuid) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE id = $1";

        let result = sqlx::query(query).bind(user_id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE email = $1";

        let result = sqlx::query(query).bind(email).execute(&self.pool).await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_verification_token_expires_at(
        &self,
        expires_at: NaiveDateTime,
        id: Uuid,
    ) -> Result<bool, Error> {
        let query = "UPDATE users SET verification_token_expires_at = $1 WHERE id = $2";

        let result = sqlx::query(query)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn update_user_profile(
        &self,
        id: Uuid,
        update: UserProfileUpdate,
    ) -> Result<Option<User>, Error> {
        let query = r#"
            UPDATE users
            SET
                name = COALESCE($1, name),
                photo_url = COALESCE($2, photo_url),
                time_zone = COALESCE($3, time_zone),
                locale = COALESCE($4, locale),
                updated_at = NOW()
            WHERE id = $5
            RETURNING id, name, email, password, verification_token_expires_at, subscribed, verified,
                      created_at, updated_at, verification_token, google_sub, photo_url, time_zone, locale
            "#;

        query_as::<_, User>(query)
            .bind(update.name)
            .bind(update.photo_url)
            .bind(update.time_zone)
            .bind(update.locale)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    async fn find_user_with_filters(
        &self,
        filters: UserQueryFilters<'_>,
    ) -> Result<Option<User>, Error> {
        let Some((where_clause, binds)) = build_filter_query(&filters) else {
            return Ok(None);
        };

        let query = format!("{USER_SELECT_QUERY} WHERE {where_clause}");
        let mut db_query = query_as::<_, User>(&query);
        for bind in binds {
            db_query = match bind {
                FilterBind::Uuid(v) => db_query.bind(v),
                FilterBind::Text(v) => db_query.bind(v),
            };
        }

        db_query.fetch_optional(&self.pool).await
    }

    async fn verified_token(&self, token: &str) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE users
            SET
                verified = true,
                updated_at = NOW(),
                verification_token = NULL,
                verification_token_expires_at = NULL
            WHERE verification_token = $1
            "#,
        )
        .bind(token)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl PersonRepositoryTrait for DbClient {
    async fn list_persons(&self, user_account_id: Uuid) -> Result<Vec<Person>, Error> {
        let query = format!(
            "SELECT {PERSON_COLUMNS} FROM persons WHERE user_account_id = $1 ORDER BY created_at"
        );

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

        match decide_upsert_route(existing_owner, user_account_id) {
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
                        let current_query =
                            format!("SELECT {PERSON_COLUMNS} FROM persons WHERE id = $1");
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
        let query =
            "DELETE FROM persons WHERE id = $1 AND user_account_id = $2 AND is_user = false";

        let result = sqlx::query(query)
            .bind(id)
            .bind(user_account_id)
            .execute(&self.pool)
            .await?;

        Ok(result.rows_affected() > 0)
    }
}

#[async_trait::async_trait]
impl MedicationRepositoryTrait for DbClient {
    async fn list_medications(&self, user_account_id: Uuid) -> Result<Vec<(Medication, Vec<Uuid>)>, Error> {
        let query =
            format!("SELECT {MEDICATION_COLUMNS} FROM medications WHERE user_account_id = $1 ORDER BY name");

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
            person_ids_by_medication.entry(medication_id).or_default().push(person_id);
        }

        Ok(medications
            .into_iter()
            .map(|medication| {
                let person_ids = person_ids_by_medication.remove(&medication.id).unwrap_or_default();
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

        match decide_upsert_route(existing_owner, user_account_id) {
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
                        let current_query = format!(
                            "SELECT {MEDICATION_COLUMNS} FROM medications WHERE id = $1"
                        );
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

#[async_trait::async_trait]
impl IntakeRepositoryTrait for DbClient {
    async fn list_intakes(&self, user_account_id: Uuid) -> Result<Vec<(Intake, Vec<Uuid>)>, Error> {
        let query =
            format!("SELECT {INTAKE_COLUMNS} FROM intakes WHERE user_account_id = $1 ORDER BY time");

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

        let saved = match decide_upsert_route(existing_owner, user_account_id) {
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

impl DbClient {
    async fn get_person_ids_for_intake(&self, intake_id: Uuid) -> Result<Vec<Uuid>, Error> {
        sqlx::query_scalar("SELECT person_id FROM intake_person_cross_ref WHERE intake_id = $1")
            .bind(intake_id)
            .fetch_all(&self.pool)
            .await
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum RefreshTokenRotation {
    Rotated { user_id: Uuid },
    Invalid,
    ReuseDetected,
}

#[async_trait::async_trait]
pub trait RefreshTokenRepositoryTrait {
    async fn create_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: NaiveDateTime,
    ) -> Result<(), Error>;

    async fn rotate_refresh_token(
        &self,
        token_hash: &str,
        new_token_hash: &str,
        new_expires_at: NaiveDateTime,
        reuse_grace: chrono::Duration,
    ) -> Result<RefreshTokenRotation, Error>;

    async fn revoke_refresh_token(&self, token_hash: &str) -> Result<(), Error>;
}

#[async_trait::async_trait]
impl RefreshTokenRepositoryTrait for DbClient {
    async fn create_refresh_token(
        &self,
        user_id: Uuid,
        token_hash: &str,
        expires_at: NaiveDateTime,
    ) -> Result<(), Error> {
        sqlx::query(
            "INSERT INTO refresh_tokens (id, user_id, family_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(Uuid::new_v4())
        .bind(token_hash)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn rotate_refresh_token(
        &self,
        token_hash: &str,
        new_token_hash: &str,
        new_expires_at: NaiveDateTime,
        reuse_grace: chrono::Duration,
    ) -> Result<RefreshTokenRotation, Error> {
        let mut tx = self.pool.begin().await?;

        let row: Option<(Uuid, Uuid, NaiveDateTime, Option<NaiveDateTime>)> = sqlx::query_as(
            "SELECT user_id, family_id, expires_at, revoked_at FROM refresh_tokens \
             WHERE token_hash = $1 FOR UPDATE",
        )
        .bind(token_hash)
        .fetch_optional(&mut *tx)
        .await?;

        let Some((user_id, family_id, expires_at, revoked_at)) = row else {
            return Ok(RefreshTokenRotation::Invalid);
        };

        let now = chrono::Utc::now().naive_utc();
        if expires_at <= now {
            return Ok(RefreshTokenRotation::Invalid);
        }

        match revoked_at {
            Some(revoked_at) if now - revoked_at > reuse_grace => {
                sqlx::query(
                    "UPDATE refresh_tokens SET revoked_at = $2 \
                     WHERE family_id = $1 AND revoked_at IS NULL",
                )
                .bind(family_id)
                .bind(now)
                .execute(&mut *tx)
                .await?;
                tx.commit().await?;
                return Ok(RefreshTokenRotation::ReuseDetected);
            }
            Some(_) => {}
            None => {
                sqlx::query("UPDATE refresh_tokens SET revoked_at = $2 WHERE token_hash = $1")
                    .bind(token_hash)
                    .bind(now)
                    .execute(&mut *tx)
                    .await?;
            }
        }

        sqlx::query(
            "INSERT INTO refresh_tokens (id, user_id, family_id, token_hash, expires_at) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(Uuid::new_v4())
        .bind(user_id)
        .bind(family_id)
        .bind(new_token_hash)
        .bind(new_expires_at)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(RefreshTokenRotation::Rotated { user_id })
    }

    async fn revoke_refresh_token(&self, token_hash: &str) -> Result<(), Error> {
        sqlx::query(
            "DELETE FROM refresh_tokens WHERE family_id = \
             (SELECT family_id FROM refresh_tokens WHERE token_hash = $1)",
        )
        .bind(token_hash)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filters_means_no_query() {
        assert_eq!(build_filter_query(&UserQueryFilters::default()), None);
    }

    #[test]
    fn single_filter_uses_first_placeholder() {
        let filters = UserQueryFilters {
            token: Some("tok"),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "verification_token = $1");
        assert_eq!(binds, vec![FilterBind::Text("tok")]);
    }

    #[test]
    fn user_id_is_bound_as_uuid_not_text() {
        let id = Uuid::new_v4();
        let filters = UserQueryFilters {
            user_id: Some(id),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1");
        assert_eq!(binds, vec![FilterBind::Uuid(id)]);
    }

    #[test]
    fn multiple_filters_are_combined_with_and_not_or() {
        let id = Uuid::new_v4();
        let filters = UserQueryFilters {
            user_id: Some(id),
            token: Some("tok"),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1 AND verification_token = $2");
        assert_eq!(binds, vec![FilterBind::Uuid(id), FilterBind::Text("tok")]);
    }

    #[test]
    fn all_filters_get_sequential_placeholders() {
        let filters = UserQueryFilters {
            user_id: Some(Uuid::new_v4()),
            name: Some("n"),
            email: Some("e"),
            token: Some("t"),
        };
        let (clause, _) = build_filter_query(&filters).unwrap();
        assert_eq!(
            clause,
            "id = $1 AND name = $2 AND email = $3 AND verification_token = $4"
        );
    }

    #[test]
    fn decide_upsert_route_inserts_when_no_existing_owner() {
        assert_eq!(
            decide_upsert_route(None, Uuid::new_v4()),
            UpsertRoute::Insert
        );
    }

    #[test]
    fn decide_upsert_route_checks_version_when_owner_matches() {
        let user = Uuid::new_v4();
        assert_eq!(
            decide_upsert_route(Some(user), user),
            UpsertRoute::UpdateIfVersionMatches
        );
    }

    #[test]
    fn decide_upsert_route_rejects_a_different_owner() {
        assert_eq!(
            decide_upsert_route(Some(Uuid::new_v4()), Uuid::new_v4()),
            UpsertRoute::OwnedByAnotherUser
        );
    }

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
