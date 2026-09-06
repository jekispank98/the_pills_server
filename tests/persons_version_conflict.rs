//! Первый integration-тест в этом репозитории. Версионный guard в
//! `upsert_person` — атомарное свойство самого SQL (`UPDATE ... WHERE version
//! = $base_version`), для него нет чистой функции-эквивалента (см.
//! `db::tests::decide_upsert_route_*` — те покрывают только ветку владения).
//! Единственный способ реально проверить эту логику — прогнать её против
//! живого Postgres, поэтому тест здесь, а не в `#[cfg(test)]` внутри `db.rs`.
//!
//! Подключается к локальной dev-БД из `.env`/`DATABASE_URL` (в этом репозитории
//! нет CI и нет `sqlx-cli`). Локальная dev-БД была изначально накатана не через
//! sqlx-миграции (в ней нет полной истории `_sqlx_migrations`), поэтому
//! `sqlx::migrate!` тут не годится — тест сам гарантирует нужную колонку через
//! идемпотентный `ADD COLUMN IF NOT EXISTS`, не трогая остальную схему.
//! Каждый тест создаёт свои собственные user/person со случайными UUID, общих
//! фикстур нет — тесты не мешают друг другу и их можно гонять параллельно.

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

/// Заводит независимого user (обходя пароль/почту — эти тесты не про auth) и
/// возвращает его id, готовый как `user_account_id` для persons.
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
    // Кто-то (или та же клиентская сессия дважды) уже применил версию 0 -> 1.
    client
        .upsert_person(user_id, upsert(person_id, "Second", 0))
        .await
        .unwrap();

    // Повторная попытка с устаревшей base_version = 0.
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

    // base_version = 99 на несуществующем id — это Insert-ветка, версия не
    // конфликтует ни с чем.
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
    // Мимикрирует Android rebase-and-retry-once: клиент шлёт base_version,
    // получает актуальную версию с сервера (тут — просто из первого ответа) и
    // повторяет апдейт с этой версией.
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
