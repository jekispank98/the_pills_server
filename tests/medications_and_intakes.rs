use pills_server_test::db::{
    IntakeRepositoryTrait, IntakeUpsert, IntakeUpsertOutcome, LinkPersonsOutcome,
    MedicationRepositoryTrait, MedicationUpsert, MedicationUpsertOutcome,
};
use pills_server_test::models::intake::IntakeStatus;
use pills_server_test::models::medication::MedicationForm;
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
        .get_or_init(|| ensure_schema(pool.clone()))
        .await;
    pool
}

async fn ensure_schema(pool: PgPool) {
    sqlx::query("ALTER TABLE persons ADD COLUMN IF NOT EXISTS version INTEGER NOT NULL DEFAULT 0")
        .execute(&pool)
        .await
        .expect("failed to ensure persons.version column exists");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS medications (
            id               UUID PRIMARY KEY,
            user_account_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
            name             TEXT NOT NULL,
            dosage           DOUBLE PRECISION NOT NULL,
            form             TEXT NOT NULL
                CHECK (form IN ('pellets', 'liquid', 'injection', 'powder', 'drops')),
            instruction      TEXT NOT NULL DEFAULT '',
            color_tag        TEXT NOT NULL,
            created_at       TIMESTAMP NOT NULL DEFAULT now(),
            updated_at       TIMESTAMP NOT NULL DEFAULT now(),
            version          INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to ensure medications table exists");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS medication_person_cross_ref (
            medication_id UUID NOT NULL REFERENCES medications (id) ON DELETE CASCADE,
            person_id     UUID NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
            PRIMARY KEY (medication_id, person_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to ensure medication_person_cross_ref table exists");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS intakes (
            id               UUID PRIMARY KEY,
            user_account_id  UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
            medication_id    UUID NOT NULL REFERENCES medications (id) ON DELETE CASCADE,
            time             TIMESTAMP NOT NULL,
            pre_time         TIMESTAMP,
            is_completed     BOOLEAN NOT NULL DEFAULT false,
            is_passed        BOOLEAN NOT NULL DEFAULT false,
            is_canceled      BOOLEAN NOT NULL DEFAULT false,
            created_at       TIMESTAMP NOT NULL DEFAULT now(),
            updated_at       TIMESTAMP NOT NULL DEFAULT now(),
            version          INTEGER NOT NULL DEFAULT 0
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to ensure intakes table exists");

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS intake_person_cross_ref (
            intake_id UUID NOT NULL REFERENCES intakes (id) ON DELETE CASCADE,
            person_id UUID NOT NULL REFERENCES persons (id) ON DELETE CASCADE,
            PRIMARY KEY (intake_id, person_id)
        )
        "#,
    )
    .execute(&pool)
    .await
    .expect("failed to ensure intake_person_cross_ref table exists");
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

async fn create_test_person(pool: &PgPool, user_id: Uuid, name: &str) -> Uuid {
    let person_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO persons(id, user_account_id, name, relation, is_user) \
         VALUES ($1, $2, $3, 'OTHER', false)",
    )
    .bind(person_id)
    .bind(user_id)
    .bind(name)
    .execute(pool)
    .await
    .expect("failed to insert test person");
    person_id
}

fn medication_upsert(id: Uuid, name: &str, base_version: i32) -> MedicationUpsert {
    MedicationUpsert {
        id,
        name: name.to_string(),
        dosage: 100.0,
        form: MedicationForm::Pellets,
        instruction: "Take with food".to_string(),
        color_tag: "#6A94E8".to_string(),
        base_version,
    }
}

#[tokio::test]
async fn medication_upsert_succeeds_and_increments_version() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin 2", 0))
        .await
        .unwrap();

    match outcome {
        MedicationUpsertOutcome::Ok(medication) => {
            assert_eq!(medication.name, "Aspirin 2");
            assert_eq!(medication.version, 1);
            assert_eq!(medication.form, MedicationForm::Pellets);
        }
        _ => panic!("expected Ok, got a different outcome"),
    }
}

#[tokio::test]
async fn medication_stale_version_returns_conflict_with_current_row() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "v0", 0))
        .await
        .unwrap();
    client
        .upsert_medication(user_id, medication_upsert(medication_id, "v1", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_medication(user_id, medication_upsert(medication_id, "stale", 0))
        .await
        .unwrap();

    match outcome {
        MedicationUpsertOutcome::VersionConflict(current) => {
            assert_eq!(current.name, "v1");
            assert_eq!(current.version, 1);
        }
        _ => panic!("expected VersionConflict, got a different outcome"),
    }
}

#[tokio::test]
async fn medication_upsert_by_a_different_owner_is_rejected() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let owner = create_test_user(&pool).await;
    let intruder = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();

    client
        .upsert_medication(owner, medication_upsert(medication_id, "Owner's", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_medication(intruder, medication_upsert(medication_id, "Hijacked", 0))
        .await
        .unwrap();

    assert!(matches!(
        outcome,
        MedicationUpsertOutcome::OwnedByAnotherUser
    ));
}

#[tokio::test]
async fn link_persons_to_medication_is_additive_and_never_unlinks() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let bob = create_test_person(&pool, user_id, "Bob").await;

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    client
        .link_persons_to_medication(medication_id, user_id, &[alice])
        .await
        .unwrap();
    client
        .link_persons_to_medication(medication_id, user_id, &[bob])
        .await
        .unwrap();

    let persons = client
        .get_persons_for_medication(medication_id)
        .await
        .unwrap();
    let mut names: Vec<_> = persons.into_iter().map(|p| p.name).collect();
    names.sort();
    assert_eq!(names, vec!["Alice", "Bob"]);
}

#[tokio::test]
async fn link_persons_to_medication_rejects_a_person_you_do_not_own() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let stranger = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let not_mine = create_test_person(&pool, stranger, "Not Mine").await;

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let outcome = client
        .link_persons_to_medication(medication_id, user_id, &[not_mine])
        .await
        .unwrap();

    assert!(matches!(outcome, LinkPersonsOutcome::SomePersonsNotOwned));
    assert!(
        client
            .get_persons_for_medication(medication_id)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn link_persons_to_medication_rejects_a_medication_you_do_not_own() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let owner = create_test_user(&pool).await;
    let intruder = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let my_person = create_test_person(&pool, intruder, "Mine").await;

    client
        .upsert_medication(owner, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let outcome = client
        .link_persons_to_medication(medication_id, intruder, &[my_person])
        .await
        .unwrap();

    assert!(matches!(outcome, LinkPersonsOutcome::MedicationNotOwned));
}

#[tokio::test]
async fn list_medications_returns_each_medication_with_its_person_ids() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let bob = create_test_person(&pool, user_id, "Bob").await;
    let aspirin_id = Uuid::new_v4();
    let ibuprofen_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(aspirin_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .upsert_medication(user_id, medication_upsert(ibuprofen_id, "Ibuprofen", 0))
        .await
        .unwrap();
    client
        .link_persons_to_medication(aspirin_id, user_id, &[alice, bob])
        .await
        .unwrap();

    let other_user = create_test_user(&pool).await;
    let other_person = create_test_person(&pool, other_user, "Carol").await;
    let other_medication = Uuid::new_v4();
    client
        .upsert_medication(other_user, medication_upsert(other_medication, "Other", 0))
        .await
        .unwrap();
    client
        .link_persons_to_medication(other_medication, other_user, &[other_person])
        .await
        .unwrap();

    let medications = client.list_medications(user_id).await.unwrap();

    assert_eq!(medications.len(), 2);
    let (aspirin, mut aspirin_persons) = medications[0].clone();
    assert_eq!(aspirin.id, aspirin_id);
    aspirin_persons.sort();
    let mut expected = vec![alice, bob];
    expected.sort();
    assert_eq!(aspirin_persons, expected);
    assert_eq!(medications[1].0.id, ibuprofen_id);
    assert!(medications[1].1.is_empty());
}

#[tokio::test]
async fn get_medications_for_person_returns_the_inverse_of_linking() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let aspirin_id = Uuid::new_v4();
    let ibuprofen_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(aspirin_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .upsert_medication(user_id, medication_upsert(ibuprofen_id, "Ibuprofen", 0))
        .await
        .unwrap();
    client
        .link_persons_to_medication(aspirin_id, user_id, &[alice])
        .await
        .unwrap();

    let medications = client
        .get_medications_for_person(alice, user_id)
        .await
        .unwrap();

    assert_eq!(medications.len(), 1);
    assert_eq!(medications[0].name, "Aspirin");
}

#[tokio::test]
async fn delete_medication_cascades_to_cross_refs_and_intakes() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .link_persons_to_medication(medication_id, user_id, &[alice])
        .await
        .unwrap();
    client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![alice], 0),
        )
        .await
        .unwrap();

    let deleted = client
        .delete_medication(medication_id, user_id)
        .await
        .unwrap();

    assert!(deleted);
    assert!(client.get_intake(intake_id).await.unwrap().is_none());
}

fn intake_upsert(
    id: Uuid,
    medication_id: Uuid,
    person_ids: Vec<Uuid>,
    base_version: i32,
) -> IntakeUpsert {
    use chrono::NaiveDateTime;
    IntakeUpsert {
        id,
        medication_id,
        time: "2026-01-01T09:00:00".parse::<NaiveDateTime>().unwrap(),
        pre_time: None,
        is_completed: false,
        is_passed: false,
        is_canceled: false,
        person_ids,
        base_version,
    }
}

#[tokio::test]
async fn intake_upsert_creates_with_persons_and_increments_version_on_update() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let created = client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![alice], 0),
        )
        .await
        .unwrap();
    match created {
        IntakeUpsertOutcome::Ok(intake, person_ids) => {
            assert_eq!(intake.version, 0);
            assert_eq!(person_ids, vec![alice]);
        }
        _ => panic!("expected Ok"),
    }

    let updated = client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![alice], 0),
        )
        .await
        .unwrap();
    match updated {
        IntakeUpsertOutcome::Ok(intake, _) => assert_eq!(intake.version, 1),
        _ => panic!("expected Ok"),
    }
}

#[tokio::test]
async fn intake_upsert_fully_replaces_persons_on_update_not_additive() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let alice = create_test_person(&pool, user_id, "Alice").await;
    let bob = create_test_person(&pool, user_id, "Bob").await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![alice], 0),
        )
        .await
        .unwrap();

    let outcome = client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![bob], 0),
        )
        .await
        .unwrap();

    match outcome {
        IntakeUpsertOutcome::Ok(_, person_ids) => assert_eq!(person_ids, vec![bob]),
        _ => panic!("expected Ok"),
    }
}

#[tokio::test]
async fn intake_upsert_rejects_a_medication_you_do_not_own() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let owner = create_test_user(&pool).await;
    let intruder = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(owner, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_intake_with_persons(intruder, intake_upsert(intake_id, medication_id, vec![], 0))
        .await
        .unwrap();

    assert!(matches!(outcome, IntakeUpsertOutcome::MedicationNotOwned));
}

#[tokio::test]
async fn intake_upsert_rejects_a_person_you_do_not_own() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let stranger = create_test_user(&pool).await;
    let not_mine = create_test_person(&pool, stranger, "Not Mine").await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();

    let outcome = client
        .upsert_intake_with_persons(
            user_id,
            intake_upsert(intake_id, medication_id, vec![not_mine], 0),
        )
        .await
        .unwrap();

    assert!(matches!(outcome, IntakeUpsertOutcome::SomePersonsNotOwned));
}

#[tokio::test]
async fn set_intake_status_passed_does_not_touch_completed_or_canceled() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    let mut initial = intake_upsert(intake_id, medication_id, vec![], 0);
    initial.is_completed = true;
    client
        .upsert_intake_with_persons(user_id, initial)
        .await
        .unwrap();

    let (updated, _) = client
        .set_intake_status(intake_id, user_id, IntakeStatus::Passed)
        .await
        .unwrap()
        .unwrap();
    assert!(updated.is_passed);
    assert_eq!(updated.version, 1);

    let (intake, _) = client.get_intake(intake_id).await.unwrap().unwrap();
    assert!(intake.is_passed);
    assert!(intake.is_completed);
    assert!(!intake.is_canceled);
    assert_eq!(intake.version, 1);
}

#[tokio::test]
async fn set_intake_status_uncompleted_clears_all_three_flags() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    let mut initial = intake_upsert(intake_id, medication_id, vec![], 0);
    initial.is_completed = true;
    initial.is_passed = true;
    client
        .upsert_intake_with_persons(user_id, initial)
        .await
        .unwrap();

    client
        .set_intake_status(intake_id, user_id, IntakeStatus::Uncompleted)
        .await
        .unwrap();

    let (intake, _) = client.get_intake(intake_id).await.unwrap().unwrap();
    assert!(!intake.is_completed);
    assert!(!intake.is_passed);
    assert!(!intake.is_canceled);
}

#[tokio::test]
async fn set_intake_status_returns_the_updated_intake_with_its_persons() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let user_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(user_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .upsert_intake_with_persons(user_id, intake_upsert(intake_id, medication_id, vec![], 0))
        .await
        .unwrap();

    let (first, person_ids) = client
        .set_intake_status(intake_id, user_id, IntakeStatus::Completed)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.id, intake_id);
    assert!(first.is_completed);
    assert_eq!(first.version, 1);
    assert!(person_ids.is_empty());

    let (second, _) = client
        .set_intake_status(intake_id, user_id, IntakeStatus::Uncompleted)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.version, 2);
}

#[tokio::test]
async fn set_intake_status_returns_none_for_another_users_intake() {
    let pool = test_pool().await;
    let client = pills_server_test::db::DbClient::new(pool.clone());
    let owner_id = create_test_user(&pool).await;
    let stranger_id = create_test_user(&pool).await;
    let medication_id = Uuid::new_v4();
    let intake_id = Uuid::new_v4();

    client
        .upsert_medication(owner_id, medication_upsert(medication_id, "Aspirin", 0))
        .await
        .unwrap();
    client
        .upsert_intake_with_persons(owner_id, intake_upsert(intake_id, medication_id, vec![], 0))
        .await
        .unwrap();

    let result = client
        .set_intake_status(intake_id, stranger_id, IntakeStatus::Completed)
        .await
        .unwrap();
    assert!(result.is_none());

    let (intake, _) = client.get_intake(intake_id).await.unwrap().unwrap();
    assert!(!intake.is_completed);
    assert_eq!(intake.version, 0);
}
