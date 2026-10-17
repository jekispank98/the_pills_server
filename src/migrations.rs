use sqlx::migrate::{Migrate, MigrateError, Migrator};
use sqlx::PgPool;

pub static MIGRATOR: Migrator = sqlx::migrate!();

pub async fn run(pool: &PgPool, baseline: Option<i64>) -> Result<(), MigrateError> {
    if let Some(baseline) = baseline {
        mark_applied_up_to(pool, baseline).await?;
    }
    MIGRATOR.run(pool).await
}

/// Records migrations up to `baseline` as applied without running them, for a
/// database whose schema was created by hand. Does nothing once any migration
/// is recorded.
async fn mark_applied_up_to(pool: &PgPool, baseline: i64) -> Result<(), MigrateError> {
    let mut conn = pool.acquire().await?;
    conn.ensure_migrations_table().await?;
    if !conn.list_applied_migrations().await?.is_empty() {
        return Ok(());
    }

    for migration in MIGRATOR
        .iter()
        .filter(|m| m.version <= baseline && !m.migration_type.is_down_migration())
    {
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
             VALUES ($1, $2, TRUE, $3, 0)",
        )
        .bind(migration.version)
        .bind(&*migration.description)
        .bind(&*migration.checksum)
        .execute(&mut *conn)
        .await?;
        tracing::info!(version = migration.version, "migration marked as applied (baseline)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_start_with_users_table() {
        let versions: Vec<i64> = MIGRATOR.iter().map(|m| m.version).collect();
        assert!(versions.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(&*MIGRATOR.iter().next().unwrap().description, "create users");
    }
}
