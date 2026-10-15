use chrono::{Duration, NaiveDateTime, Utc};
use pills_server_test::db::{DbClient, RefreshTokenRepositoryTrait, RefreshTokenRotation};
use pills_server_test::utils::refresh_token;
use sqlx::PgPool;
use tokio::sync::OnceCell;
use uuid::Uuid;

static SCHEMA_READY: OnceCell<()> = OnceCell::const_new();

async fn test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://admin:admin@localhost:5432/users".to_string());
    let pool = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to local dev Postgres — is it running? see .env DATABASE_URL");
    SCHEMA_READY
        .get_or_init(|| async {
            sqlx::raw_sql(include_str!("../migrations/202610151200_add_refresh_tokens.sql"))
                .execute(&pool)
                .await
                .expect("failed to ensure refresh_tokens table exists");
        })
        .await;
    pool
}

async fn create_test_user(pool: &PgPool) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users(name, email, password, verification_token_expires_at, subscribed, verification_token) \
         VALUES ($1, $2, 'x', NOW(), false, NULL) RETURNING id",
    )
    .bind(format!("Test User {}", Uuid::new_v4()))
    .bind(format!("{}@example.test", Uuid::new_v4()))
    .fetch_one(pool)
    .await
    .expect("failed to insert test user")
}

fn in_days(days: i64) -> NaiveDateTime {
    (Utc::now() + Duration::days(days)).naive_utc()
}

async fn issue(client: &DbClient, user_id: Uuid, expires_at: NaiveDateTime) -> String {
    let token = refresh_token::generate();
    client
        .create_refresh_token(user_id, &refresh_token::hash(&token), expires_at)
        .await
        .unwrap();
    token
}

async fn rotate(client: &DbClient, token: &str, grace: Duration) -> (RefreshTokenRotation, String) {
    let next = refresh_token::generate();
    let outcome = client
        .rotate_refresh_token(
            &refresh_token::hash(token),
            &refresh_token::hash(&next),
            in_days(60),
            grace,
        )
        .await
        .unwrap();
    (outcome, next)
}

#[tokio::test]
async fn valid_token_rotates_into_a_working_one() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let first = issue(&client, user_id, in_days(60)).await;

    let (outcome, second) = rotate(&client, &first, Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Rotated { user_id });

    let (outcome, _) = rotate(&client, &second, Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Rotated { user_id });
}

#[tokio::test]
async fn unknown_token_is_invalid() {
    let pool = test_pool().await;
    let client = DbClient::new(pool);

    let (outcome, _) = rotate(&client, &refresh_token::generate(), Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Invalid);
}

#[tokio::test]
async fn expired_token_is_invalid() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let expired = issue(&client, user_id, in_days(-1)).await;

    let (outcome, _) = rotate(&client, &expired, Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Invalid);
}

#[tokio::test]
async fn reusing_a_rotated_token_within_grace_still_rotates() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let first = issue(&client, user_id, in_days(60)).await;

    rotate(&client, &first, Duration::seconds(120)).await;
    let (outcome, _) = rotate(&client, &first, Duration::seconds(120)).await;

    assert_eq!(outcome, RefreshTokenRotation::Rotated { user_id });
}

#[tokio::test]
async fn reusing_a_rotated_token_after_grace_revokes_the_whole_family() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let first = issue(&client, user_id, in_days(60)).await;

    let (_, second) = rotate(&client, &first, Duration::zero()).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let (outcome, _) = rotate(&client, &first, Duration::zero()).await;
    assert_eq!(outcome, RefreshTokenRotation::ReuseDetected);

    let (outcome, _) = rotate(&client, &second, Duration::zero()).await;
    assert_eq!(outcome, RefreshTokenRotation::ReuseDetected);
}

#[tokio::test]
async fn revoked_session_cannot_be_refreshed_even_within_grace() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let first = issue(&client, user_id, in_days(60)).await;
    let (_, second) = rotate(&client, &first, Duration::seconds(120)).await;

    client
        .revoke_refresh_token(&refresh_token::hash(&second))
        .await
        .unwrap();

    let (outcome, _) = rotate(&client, &second, Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Invalid);
    let (outcome, _) = rotate(&client, &first, Duration::seconds(120)).await;
    assert_eq!(outcome, RefreshTokenRotation::Invalid);
}

#[tokio::test]
async fn other_users_families_are_untouched_by_reuse_detection() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let alice = create_test_user(&pool).await;
    let bob = create_test_user(&pool).await;
    let alice_first = issue(&client, alice, in_days(60)).await;
    let bob_token = issue(&client, bob, in_days(60)).await;

    rotate(&client, &alice_first, Duration::zero()).await;
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    rotate(&client, &alice_first, Duration::zero()).await;

    let (outcome, _) = rotate(&client, &bob_token, Duration::zero()).await;
    assert_eq!(outcome, RefreshTokenRotation::Rotated { user_id: bob });
}
