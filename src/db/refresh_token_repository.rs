use super::DbClient;
use chrono::NaiveDateTime;
use sqlx::Error;
use uuid::Uuid;

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
