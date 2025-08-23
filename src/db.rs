use crate::User;
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

#[async_trait::async_trait]
pub trait AuthentificationTrait {
    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        user_id: String,
        token_expires_at: NaiveDateTime,
    ) -> Result<User, Error>;

    async fn check_is_user_exist(&self, email: String) -> Result<Option<User>, Error>;

    async fn delete_user_by_id(&self, user_id: &str) -> Result<bool, Error>;

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error>;

    async fn update_token_expires_at(
        &self,
        token_expires: NaiveDateTime,
        id: String,
    ) -> Result<bool, Error>;
}

#[async_trait::async_trait]
impl AuthentificationTrait for DbClient {
    async fn save_user(
        &self,
        name: String,
        email: String,
        password: String,
        user_id: String,
        token_expires_at: NaiveDateTime,
    ) -> Result<User, Error> {
        let query = r#"
        INSERT INTO users(name, login, password, id, token_expires_at, subscribed)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING name, login, password, id, token_expires_at, false"#;
        let res = query_as::<_, User>(query)
            .bind(name)
            .bind(email)
            .bind(password)
            .bind(user_id)
            .bind(token_expires_at)
            .bind(false)
            .fetch_one(&self.pool)
            .await;
        
        println!("result: {:?}", res);
        res
    }

    async fn check_is_user_exist(&self, email: String) -> Result<Option<User>, Error> {
        let query = r#"
        SELECT name, login, password, id, token_expires_at, subscribed
        FROM users
        WHERE login = $1
        "#;

        let user = query_as::<_, User>(query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await;
        println!("User from DB: {:?}", user);
        user
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
        id: String,
    ) -> Result<bool, Error> {
        let query = "UPDATE users SET token_expires_at = $1 WHERE id = $2";

        let result = sqlx::query(query)
            .bind(token_expires)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }
}
