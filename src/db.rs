use crate::models::intake::{Intake, IntakeStatus};
use crate::models::medication::{Medication, MedicationForm};
use crate::models::person::{Gender, Person, PersonRelation};
use crate::models::user::User;
use chrono::NaiveDateTime;
use sqlx::{query_as, Error, Pool, Postgres};
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

// Общий список колонок `persons` — переиспользуется и в SELECT, и в RETURNING
// у INSERT/UPDATE, чтобы не дублировать (и не рассинхронизировать) список полей.
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

/// Поля профиля, которые можно поменять через `PATCH /users/me`.
/// `None` — «не трогать это поле» (частичное обновление), а не «очистить».
#[derive(Debug, Clone, Default)]
pub struct UserProfileUpdate {
    pub name: Option<String>,
    pub photo_url: Option<String>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
}

/// Поля, которые клиент присылает при создании/обновлении `Person`.
/// `is_user` туда намеренно не входит — SELF-профиль создаётся только при
/// регистрации (см. `register_user_with_self_person`), апсерт его не трогает.
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
    /// Последняя версия, известная клиенту — 0 для новой персоны. См.
    /// `PersonRepositoryTrait::upsert_person`/`decide_upsert_route`.
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

    /// Создаёт пользователя и его SELF-профиль (`Person`) одной транзакцией —
    /// либо оба ряда появляются, либо ни одного. Раньше персона не создавалась
    /// вовсе (таблицы не было); теперь бэкенд ведёт себя как Android
    /// `UserRepositoryImpl.signUp`, который тоже создаёт SELF-персону при регистрации.
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

    /// Продлевает срок жизни токена подтверждения email (например, при повторной
    /// отправке письма). К сессионной аутентификации отношения не имеет —
    /// сессия живёт только в JWT.
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

/// Итог апсерта: успешно, чужой `id` (конфликт владения), либо устаревшая
/// `base_version` (конфликт версии — `VersionConflict` несёт актуальную запись,
/// чтобы клиент мог сделать rebase-and-retry-once).
pub enum PersonUpsertOutcome {
    Ok(Person),
    OwnedByAnotherUser,
    VersionConflict(Person),
}

/// Куда должен пойти upsert по итогам ownership pre-check — единственная часть
/// решения, которая не требует атомарности с самой записью в БД (в отличие от
/// version-guard, который обязан быть частью одного UPDATE ... WHERE, иначе
/// теряется гарантия атомарности), поэтому вынесена в чистую функцию и
/// unit-тестируется без БД.
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

/// Поля, которые клиент присылает при создании/обновлении `Medication`.
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

/// Тот же трёхвариантный итог, что `PersonUpsertOutcome` — тот же
/// `decide_upsert_route`/OCC-паттерн, другая таблица.
pub enum MedicationUpsertOutcome {
    Ok(Medication),
    OwnedByAnotherUser,
    VersionConflict(Medication),
}

/// Итог привязки persons к лекарству — отдельные варианты для "чужое
/// лекарство" и "один из person_ids не ваш", чтобы хендлер мог вернуть точный
/// статус вместо общего 403.
pub enum LinkPersonsOutcome {
    Ok,
    MedicationNotOwned,
    SomePersonsNotOwned,
}

/// Поля, которые клиент присылает при создании/обновлении `Intake`, включая
/// полный список привязанных persons (full-replace при апдейте — см.
/// `IntakeRepositoryTrait::upsert_intake_with_persons`).
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

/// Расширяет `MedicationUpsertOutcome`-паттерн двумя дополнительными
/// отказами, специфичными для Intake: ссылка на чужое/несуществующее
/// лекарство и попытка привязать не принадлежащего вам person. `Ok`/
/// `VersionConflict` несут уже актуальный список `person_ids` вместе со
/// строкой, чтобы хендлеру не нужен был отдельный запрос за ним.
pub enum IntakeUpsertOutcome {
    Ok(Intake, Vec<Uuid>),
    OwnedByAnotherUser,
    VersionConflict(Intake, Vec<Uuid>),
    MedicationNotOwned,
    SomePersonsNotOwned,
}

/// Чистая функция — какие флаги трогает каждый переход статуса (`None` —
/// «не менять»). Зеркалит четыре DAO-метода Android `IntakeDao`
/// (markAsPassed/markAsCanceled/markAsCompleted/markAsUncompleted) один в
/// один, включая их асимметрию: `Passed` трогает только `is_passed`, три
/// остальных явно переустанавливают все три флага. Вынесена отдельно по
/// той же причине, что `decide_upsert_route` — тестируется без БД, а
/// `set_intake_status` строит по ней SQL тем же приёмом, что
/// `build_filter_query` выше.
fn intake_status_flags(status: IntakeStatus) -> (Option<bool>, Option<bool>, Option<bool>) {
    // (is_completed, is_passed, is_canceled)
    match status {
        IntakeStatus::Passed => (None, Some(true), None),
        IntakeStatus::Canceled => (Some(false), Some(true), Some(true)),
        IntakeStatus::Completed => (Some(true), Some(true), Some(false)),
        IntakeStatus::Uncompleted => (Some(false), Some(false), Some(false)),
    }
}

#[async_trait::async_trait]
pub trait MedicationRepositoryTrait {
    async fn list_medications(&self, user_account_id: Uuid) -> Result<Vec<Medication>, Error>;

    async fn get_medication(&self, id: Uuid) -> Result<Option<Medication>, Error>;

    /// Идемпотентный upsert по client-generated `id`, тот же паттерн, что
    /// `PersonRepositoryTrait::upsert_person`.
    async fn upsert_medication(
        &self,
        user_account_id: Uuid,
        medication: MedicationUpsert,
    ) -> Result<MedicationUpsertOutcome, Error>;

    /// `ON DELETE CASCADE` на `medication_person_cross_ref`/`intakes` сам
    /// подчищает связанные записи — отдельная очистка не нужна.
    async fn delete_medication(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error>;

    /// Аддитивная линковка (как Android `linkPersonsToMedication` —
    /// `OnConflictStrategy.IGNORE`): никогда не отвязывает, только добавляет.
    async fn link_persons_to_medication(
        &self,
        medication_id: Uuid,
        user_account_id: Uuid,
        person_ids: &[Uuid],
    ) -> Result<LinkPersonsOutcome, Error>;

    /// Параллель Android `getPersonsByMedicationId` (медикамент -> люди).
    async fn get_persons_for_medication(&self, medication_id: Uuid) -> Result<Vec<Person>, Error>;

    /// Обратное направление (человек -> медикаменты) — на Android такой
    /// ручки пока нет вообще, заводим с нуля.
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

    /// Апдейт сразу и строки, и привязанных persons (full-replace) — одна
    /// транзакция, зеркалит Android `IntakeDao.upsertIntakeWithPersons`.
    async fn upsert_intake_with_persons(
        &self,
        user_account_id: Uuid,
        intake: IntakeUpsert,
    ) -> Result<IntakeUpsertOutcome, Error>;

    async fn delete_intake(&self, id: Uuid, user_account_id: Uuid) -> Result<bool, Error>;

    /// Точечный переход статуса без апдейта остальных полей — параллель
    /// четырём `markAs*` методам Android `IntakeDao`.
    async fn set_intake_status(
        &self,
        id: Uuid,
        user_account_id: Uuid,
        status: IntakeStatus,
    ) -> Result<bool, Error>;
}

#[async_trait::async_trait]
pub trait PersonRepositoryTrait {
    async fn list_persons(&self, user_account_id: Uuid) -> Result<Vec<Person>, Error>;

    async fn get_person(&self, id: Uuid) -> Result<Option<Person>, Error>;

    /// Идемпотентный upsert по client-generated `id`. Если строка с таким `id`
    /// уже существует и принадлежит другому аккаунту — не перезаписывает её.
    async fn upsert_person(
        &self,
        user_account_id: Uuid,
        person: PersonUpsert,
    ) -> Result<PersonUpsertOutcome, Error>;

    /// Удаляет только если персона принадлежит `user_account_id` и не является
    /// SELF-профилем (`is_user = false`) — тот же запрет, что уже есть в
    /// Android `ProfileViewModel` (нельзя удалить самого себя), продублирован
    /// на сервере, а не только на клиенте.
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
        // COALESCE($n, column) — присланное значение перезаписывает, а
        // отсутствующее (NULL-бинд) оставляет колонку как есть: PATCH
        // семантика "не трогай, если не прислано", а не "обнули".
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
                // Новая строка — version стартует с DEFAULT 0 колонки, base_version
                // клиента здесь не участвует (это не апдейт, конфликтовать не с чем).
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
                // Не INSERT ... ON CONFLICT ... WHERE: если WHERE исключает строку,
                // Postgres всё равно считает conflict-target удовлетворённым и молча
                // не делает апдейт — нет способа отличить "версия не совпала" от
                // "всё ок, апдейт применился". Поэтому это отдельный UPDATE ...
                // WHERE version = $base_version RETURNING ...: fetch_optional
                // вернёт None ровно тогда, когда ни одна строка не подошла под
                // версию — однозначный сигнал конфликта.
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
    async fn list_medications(&self, user_account_id: Uuid) -> Result<Vec<Medication>, Error> {
        let query =
            format!("SELECT {MEDICATION_COLUMNS} FROM medications WHERE user_account_id = $1 ORDER BY name");

        query_as::<_, Medication>(&query)
            .bind(user_account_id)
            .fetch_all(&self.pool)
            .await
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
                // Тот же приём, что у Person: отдельный UPDATE ... WHERE
                // version = $base_version RETURNING ..., а не
                // INSERT ... ON CONFLICT ... WHERE (см. комментарий в
                // upsert_person выше — тот же аргумент применим здесь).
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
        // Фильтр по user_account_id — дополнительная защита от утечки между
        // аккаунтами сверх проверки на уровне хендлера (что person_id — ваш).
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

        // Лекарство должно существовать и принадлежать тому же аккаунту —
        // иначе можно было бы привязать intake к чужому medication_id.
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

        // Full replace — в отличие от аддитивной medication-линковки выше,
        // зеркалит Android IntakeDao.upsertIntakeWithPersons (delete+reinsert).
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
    ) -> Result<bool, Error> {
        let (is_completed, is_passed, is_canceled) = intake_status_flags(status);

        // Тот же динамический SET-билдер, что build_filter_query выше строит
        // WHERE — только для полей, которые этот переход статуса реально трогает.
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
            "UPDATE intakes SET {} WHERE id = ${} AND user_account_id = ${}",
            sets.join(", "),
            binds.len() + 1,
            binds.len() + 2
        );

        let mut db_query = sqlx::query(&query);
        for bind in binds {
            db_query = db_query.bind(bind);
        }
        db_query = db_query.bind(id).bind(user_account_id);

        let result = db_query.execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
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
        // Раньше `user_id: Option<Uuid>` биндился через `.to_string()` в текстовый
        // параметр против INTEGER-колонки `id` — запрос падал бы в рантайме. Теперь
        // `id` сама стала UUID, так что биндим typed `Uuid` напрямую.
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
        // Регрессия на баг: get_user(Some(id), None, None, Some(token)) раньше
        // строил "id = $1 OR verification_token = $2" и мог вернуть чужого
        // пользователя, у которого совпал только один из фильтров.
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
