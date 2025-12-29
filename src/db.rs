use crate::User;
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

#[derive(Debug, Clone, Copy)]
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
        token_expires_at, 
        subscribed,
        verified, 
        created_at, 
        updated_at, 
        verification_token
    FROM users
"#;

#[async_trait::async_trait]
pub trait AuthentificationTrait {
    async fn get_user(
        &self,
        user_id: Option<Uuid>,
        name: Option<&str>,
        email: Option<&str>,
        token: Option<&str>,
    ) -> Result<Option<User>, Error>;

    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<User, Error>;

    async fn check_is_user_exist(&self, email: String) -> Result<Option<User>, Error>;

    async fn delete_user_by_id(&self, user_id: &str) -> Result<bool, Error>;

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error>;

    async fn update_token_expires_at(
        &self,
        token_expires: NaiveDateTime,
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
    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<User, Error> {
        let query = r#"
    INSERT INTO users(name, email, password, token_expires_at, subscribed, verification_token)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id, name, email, password, token_expires_at, subscribed, verified, created_at, updated_at, verification_token"#;

        query_as::<_, User>(query)
            .bind(name)
            .bind(email)
            .bind(password)
            .bind(token_expires_at)
            .bind(false)
            .bind(verification_token)
            .fetch_one(&self.pool)
            .await
    }

    async fn check_is_user_exist(&self, email: String) -> Result<Option<User>, Error> {
        let query = r#"
    SELECT id, name, email, password, token_expires_at, subscribed, verified, created_at, updated_at, verification_token
    FROM users
    WHERE email = $1
    "#;

        let q = query_as::<_, User>(query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await;
        println!("result : {:?}", q);
        q
    }

    async fn delete_user_by_id(&self, user_id: &str) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE id = $1";

        let result = sqlx::query(query).bind(user_id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE email = $1";

        let result = sqlx::query(query).bind(email).execute(&self.pool).await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_token_expires_at(
        &self,
        token_expires: NaiveDateTime,
        id: i32,
    ) -> Result<bool, Error> {
        let query = "UPDATE users SET token_expires_at = $1 WHERE id = $2";

        let result = sqlx::query(query)
            .bind(token_expires)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn find_user_with_filters(
        &self,
        filters: UserQueryFilters<'_>,
    ) -> Result<Option<User>, Error> {
        let mut query = USER_SELECT_QUERY.to_string();
        let mut conditions = Vec::new();
        let mut params: Vec<String> = Vec::new();
        let mut param_count = 1;

        if let Some(user_id) = filters.user_id {
            conditions.push(format!("id = ${}", param_count));
            params.push(user_id.to_string());
            param_count += 1;
        }

        if let Some(name) = filters.name {
            conditions.push(format!("name = ${}", param_count));
            params.push(name.to_string());
            param_count += 1;
        }

        if let Some(email) = filters.email {
            conditions.push(format!("email = ${}", param_count));
            params.push(email.to_string());
            param_count += 1;
        }

        if let Some(token) = filters.token {
            conditions.push(format!("verification_token = ${}", param_count));
            params.push(token.to_string());
        }

        if !conditions.is_empty() {
            query.push_str(" WHERE ");
            query.push_str(&conditions.join(" OR "));
        } else {
            return Ok(None);
        }

        let mut db_query = query_as::<_, User>(&query);

        for param in params {
            db_query = db_query.bind(param);
        }

        db_query.fetch_optional(&self.pool).await
    }

    async fn verified_token(&self, token: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE users
            SET 
                verified = true, 
                updated_at = NOW(),
                verification_token = NULL,
                token_expires_at = NULL
            WHERE verification_token = $1
            "#,
        )
        .bind(token)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
