use crate::models::user::User;
use chrono::NaiveDateTime;
use sqlx::{query_as, Error, Pool, Postgres};

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
    pub user_id: Option<i32>,
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
        google_sub
    FROM users
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
enum FilterBind<'a> {
    Int(i32),
    Text(&'a str),
}


fn build_filter_query<'a>(filters: &UserQueryFilters<'a>) -> Option<(String, Vec<FilterBind<'a>>)> {
    let mut conditions = Vec::new();
    let mut binds: Vec<FilterBind<'a>> = Vec::new();

    if let Some(user_id) = filters.user_id {
        binds.push(FilterBind::Int(user_id));
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

#[async_trait::async_trait]
pub trait AuthentificationTrait {
    async fn get_user(
        &self,
        user_id: Option<i32>,
        name: Option<&str>,
        email: Option<&str>,
        token: Option<&str>,
    ) -> Result<Option<User>, Error>;

    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<User, Error>;

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error>;

    async fn delete_user_by_id(&self, user_id: i32) -> Result<bool, Error>;

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error>;

    /// Продлевает срок жизни токена подтверждения email (например, при повторной
    /// отправке письма). К сессионной аутентификации отношения не имеет —
    /// сессия живёт только в JWT.
    async fn update_verification_token_expires_at(
        &self,
        expires_at: NaiveDateTime,
        id: i32,
    ) -> Result<bool, Error>;

    async fn find_user_with_filters(
        &self,
        filters: UserQueryFilters<'_>,
    ) -> Result<Option<User>, Error>;

    async fn verified_token(&self, token: &str) -> Result<(), Error>;
}

#[async_trait::async_trait]
impl AuthentificationTrait for DbClient {
    async fn get_user(
        &self,
        user_id: Option<i32>,
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

    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<User, Error> {
        let query = r#"
    INSERT INTO users(name, email, password, verification_token_expires_at, subscribed, verification_token)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id, name, email, password, verification_token_expires_at, subscribed, verified,
              created_at, updated_at, verification_token, google_sub"#;

        query_as::<_, User>(query)
            .bind(name)
            .bind(email)
            .bind(password)
            .bind(verification_token_expires_at)
            .bind(false)
            .bind(verification_token)
            .fetch_one(&self.pool)
            .await
    }

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error> {
        let query = format!("{USER_SELECT_QUERY} WHERE email = $1");

        query_as::<_, User>(&query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }

    async fn delete_user_by_id(&self, user_id: i32) -> Result<bool, Error> {
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
        id: i32,
    ) -> Result<bool, Error> {
        let query = "UPDATE users SET verification_token_expires_at = $1 WHERE id = $2";

        let result = sqlx::query(query)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
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
                FilterBind::Int(v) => db_query.bind(v),
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
    fn user_id_is_bound_as_int_not_text() {
        // Раньше `user_id: Option<Uuid>` биндился через `.to_string()` в текстовый
        // параметр против INTEGER-колонки `id` — запрос падал бы в рантайме.
        let filters = UserQueryFilters {
            user_id: Some(42),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1");
        assert_eq!(binds, vec![FilterBind::Int(42)]);
    }

    #[test]
    fn multiple_filters_are_combined_with_and_not_or() {
        // Регрессия на баг: get_user(Some(id), None, None, Some(token)) раньше
        // строил "id = $1 OR verification_token = $2" и мог вернуть чужого
        // пользователя, у которого совпал только один из фильтров.
        let filters = UserQueryFilters {
            user_id: Some(1),
            token: Some("tok"),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1 AND verification_token = $2");
        assert_eq!(binds, vec![FilterBind::Int(1), FilterBind::Text("tok")]);
    }

    #[test]
    fn all_filters_get_sequential_placeholders() {
        let filters = UserQueryFilters {
            user_id: Some(1),
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
