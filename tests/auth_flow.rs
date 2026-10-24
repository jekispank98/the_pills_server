use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use chrono::{Duration, Utc};
use pills_server_test::config::{Config, Environment, SmtpConfig};
use pills_server_test::db::{DbClient, RefreshTokenRepositoryTrait};
use pills_server_test::migrations;
use pills_server_test::routes::create_routes;
use pills_server_test::state::AppState;
use pills_server_test::utils::refresh_token;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tower::util::ServiceExt;
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
            migrations::run(&pool, None)
                .await
                .expect("failed to run migrations for auth integration tests");
        })
        .await;

    pool
}

fn test_app(pool: PgPool) -> axum::Router {
    let config = Config {
        database_url: "postgres://localhost/test".into(),
        database_max_connections: 10,
        migrations_baseline: None,
        jwt_secret: "test-secret-that-is-long-enough-32b".into(),
        jwt_max_age: std::time::Duration::from_secs(3600),
        refresh_token_max_age: std::time::Duration::from_secs(60 * 24 * 3600),
        auth_rate_limit_per_minute: 10,
        listen_addr: "127.0.0.1:3000".parse().unwrap(),
        public_base_url: "http://localhost:3000".into(),
        google_web_client_id: "test-client-id".into(),
        smtp: SmtpConfig {
            server: "localhost".into(),
            port: 587,
            username: "test@example.com".into(),
            password: "test".into(),
            from_address: "test@example.com".into(),
        },
        env: Environment::Development,
    };
    create_routes(Arc::new(AppState::new(pool, config)))
}

async fn create_user(
    pool: &PgPool,
    verified: bool,
    verification_token: Option<&str>,
    expires_in_days: i64,
) -> Uuid {
    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users(name, email, password, verification_token_expires_at, subscribed, verified, verification_token) \
         VALUES ($1, $2, 'x', $3, false, $4, $5) RETURNING id",
    )
    .bind(format!("Auth User {}", Uuid::new_v4()))
    .bind(format!("{}@example.test", Uuid::new_v4()))
    .bind((Utc::now() + Duration::days(expires_in_days)).naive_utc())
    .bind(verified)
    .bind(verification_token)
    .fetch_one(pool)
    .await
    .expect("failed to insert auth test user")
}

async fn issue_refresh_token(client: &DbClient, user_id: Uuid) -> String {
    let token = refresh_token::generate();
    client
        .create_refresh_token(
            user_id,
            &refresh_token::hash(&token),
            (Utc::now() + Duration::days(60)).naive_utc(),
        )
        .await
        .expect("failed to issue refresh token");
    token
}

fn post_json(uri: &str, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

#[tokio::test]
async fn refresh_endpoint_rotates_token_and_returns_new_pair() {
    let pool = test_pool().await;
    let db_client = DbClient::new(pool.clone());
    let user_id = create_user(&pool, true, None, 1).await;
    let current_refresh = issue_refresh_token(&db_client, user_id).await;

    let app = test_app(pool);
    let response = app
        .clone()
        .oneshot(post_json(
            "/auth/refresh_token",
            json!({ "refresh_token": current_refresh }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let new_access = json.get("token").and_then(Value::as_str).unwrap();
    let new_refresh = json.get("refresh_token").and_then(Value::as_str).unwrap();
    assert!(!new_access.is_empty());
    assert!(!new_refresh.is_empty());

    let response = app
        .oneshot(post_json(
            "/auth/refresh_token",
            json!({ "refresh_token": new_refresh }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn logout_endpoint_revokes_refresh_token() {
    let pool = test_pool().await;
    let db_client = DbClient::new(pool.clone());
    let user_id = create_user(&pool, true, None, 1).await;
    let refresh = issue_refresh_token(&db_client, user_id).await;

    let app = test_app(pool);
    let logout = app
        .clone()
        .oneshot(post_json(
            "/auth/logout",
            json!({ "refresh_token": refresh.clone() }),
        ))
        .await
        .unwrap();
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);

    let refresh_after_logout = app
        .oneshot(post_json(
            "/auth/refresh_token",
            json!({ "refresh_token": refresh }),
        ))
        .await
        .unwrap();
    assert_eq!(refresh_after_logout.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn verify_endpoint_marks_user_verified_and_token_cannot_be_reused() {
    let pool = test_pool().await;
    let token = format!("verify-{}", Uuid::new_v4());
    let user_id = create_user(&pool, false, Some(&token), 1).await;
    let app = test_app(pool.clone());

    let verify = Request::builder()
        .method("GET")
        .uri(format!("/auth/verify?token={token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(verify).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let (verified, stored_token): (bool, Option<String>) =
        sqlx::query_as("SELECT verified, verification_token FROM users WHERE id = $1")
            .bind(user_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(verified);
    assert!(stored_token.is_none());

    let verify_again = Request::builder()
        .method("GET")
        .uri(format!("/auth/verify?token={token}"))
        .body(Body::empty())
        .unwrap();
    let response = app.oneshot(verify_again).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn verify_endpoint_rejects_expired_token() {
    let pool = test_pool().await;
    let token = format!("expired-{}", Uuid::new_v4());
    create_user(&pool, false, Some(&token), -1).await;
    let app = test_app(pool);

    let request = Request::builder()
        .method("GET")
        .uri(format!("/auth/verify?token={token}"))
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
