use pills_server_test::db::{DbClient, PersonRepositoryTrait, PersonUpsert, PersonUpsertOutcome};
use pills_server_test::models::person::{Gender, PersonRelation};
use sqlx::PgPool;
use uuid::Uuid;

async fn test_pool() -> PgPool {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://admin:admin@localhost:5432/users".to_string());
    let pool = PgPool::connect(&database_url)
        .await
        .expect("failed to connect to local dev Postgres — is it running? see .env DATABASE_URL");
    sqlx::query("ALTER TABLE persons ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await
        .expect("failed to ensure persons.version column exists");
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

fn upsert(id: Uuid, name: &str, base_version: i32) -> PersonUpsert {
    PersonUpsert {
        id,
        name: name.to_string(),
        relation: PersonRelation::Other,
        birth_date: None,
        gender: Gender::Unspecified,
        weight_kg: None,
        color_tag: None,
        notes: None,
        is_active: true,
        photo_url: None,
        base_version,
    }
}

#[tokio::test]
async fn matching_base_version_succeeds_and_increments_version() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let person_id = Uuid::new_v4();

    client
        .upsert_person(user_id, upsert(person_id, "First", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_person(user_id, upsert(person_id, "Second", 0))
        .await
        .unwrap();

    match outcome {
        PersonUpsertOutcome::Ok(person) => {
            assert_eq!(person.name, "Second");
            assert_eq!(person.version, 1);
        }
        _ => panic!("expected Ok, got a different outcome"),
    }
}

#[tokio::test]
async fn stale_base_version_returns_version_conflict_with_current_row() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let person_id = Uuid::new_v4();

    client
        .upsert_person(user_id, upsert(person_id, "First", 0))
        .await
        .unwrap();
    client
        .upsert_person(user_id, upsert(person_id, "Second", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_person(user_id, upsert(person_id, "Stale write", 0))
        .await
        .unwrap();

    match outcome {
        PersonUpsertOutcome::VersionConflict(current) => {
            assert_eq!(current.name, "Second");
            assert_eq!(current.version, 1);
        }
        _ => panic!("expected VersionConflict, got a different outcome"),
    }
}

#[tokio::test]
async fn brand_new_person_ignores_base_version_and_starts_at_zero() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let person_id = Uuid::new_v4();

    let outcome = client
        .upsert_person(user_id, upsert(person_id, "Brand new", 99))
        .await
        .unwrap();

    match outcome {
        PersonUpsertOutcome::Ok(person) => assert_eq!(person.version, 0),
        _ => panic!("expected Ok, got a different outcome"),
    }
}

#[tokio::test]
async fn upsert_by_a_different_owner_is_rejected_without_changing_the_row() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let owner = create_test_user(&pool).await;
    let intruder = create_test_user(&pool).await;
    let person_id = Uuid::new_v4();

    client
        .upsert_person(owner, upsert(person_id, "Owner's person", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_person(intruder, upsert(person_id, "Hijacked", 0))
        .await
        .unwrap();

    assert!(matches!(outcome, PersonUpsertOutcome::OwnedByAnotherUser));

    let unchanged = client.get_person(person_id).await.unwrap().unwrap();
    assert_eq!(unchanged.name, "Owner's person");
    assert_eq!(unchanged.version, 0);
}

#[tokio::test]
async fn two_sequential_upserts_reusing_the_returned_version_both_succeed() {
    let pool = test_pool().await;
    let client = DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let person_id = Uuid::new_v4();

    client
        .upsert_person(user_id, upsert(person_id, "v0", 0))
        .await
        .unwrap();

    let first = client
        .upsert_person(user_id, upsert(person_id, "v1", 0))
        .await
        .unwrap();
    let first_version = match first {
        PersonUpsertOutcome::Ok(person) => person.version,
        _ => panic!("expected Ok"),
    };

    let second = client
        .upsert_person(user_id, upsert(person_id, "v2", first_version))
        .await
        .unwrap();

    match second {
        PersonUpsertOutcome::Ok(person) => {
            assert_eq!(person.name, "v2");
            assert_eq!(person.version, first_version + 1);
        }
        _ => panic!("expected Ok, got a different outcome"),
    }
}
