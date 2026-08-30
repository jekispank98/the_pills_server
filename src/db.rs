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
    weight_kg, color_tag, notes, is_active, photo_url, created_at, updated_at";

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

/// Итог апсерта: успешно, либо чужой `id` (конфликт владения).
pub enum PersonUpsertOutcome {
    Ok(Person),
    OwnedByAnotherUser,
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

        if let Some(owner) = existing_owner {
            if owner != user_account_id {
                tx.rollback().await?;
                return Ok(PersonUpsertOutcome::OwnedByAnotherUser);
            }
        }

        let query = format!(
            r#"
    INSERT INTO persons(id, user_account_id, name, relation, is_user, birth_date, gender,
                         weight_kg, color_tag, notes, is_active, photo_url)
    VALUES ($1, $2, $3, $4, false, $5, $6, $7, $8, $9, $10, $11)
    ON CONFLICT (id) DO UPDATE SET
        name = EXCLUDED.name,
        relation = EXCLUDED.relation,
        birth_date = EXCLUDED.birth_date,
        gender = EXCLUDED.gender,
        weight_kg = EXCLUDED.weight_kg,
        color_tag = EXCLUDED.color_tag,
        notes = EXCLUDED.notes,
        is_active = EXCLUDED.is_active,
        photo_url = EXCLUDED.photo_url,
        updated_at = NOW()
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
}
