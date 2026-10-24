use sqlx::{Pool, Postgres};
use uuid::Uuid;

mod auth_repository;
mod intake_repository;
mod medication_repository;
mod person_repository;
mod refresh_token_repository;

pub use auth_repository::{AuthentificationTrait, UserProfileUpdate, UserQueryFilters};
pub use intake_repository::{IntakeRepositoryTrait, IntakeUpsert, IntakeUpsertOutcome};
pub use medication_repository::{
    LinkPersonsOutcome, MedicationRepositoryTrait, MedicationUpsert, MedicationUpsertOutcome,
};
pub use person_repository::{PersonRepositoryTrait, PersonUpsert, PersonUpsertOutcome};
pub use refresh_token_repository::{RefreshTokenRepositoryTrait, RefreshTokenRotation};

#[derive(Debug, Clone)]
pub struct DbClient {
    pool: Pool<Postgres>,
}

impl DbClient {
    pub fn new(pool: Pool<Postgres>) -> Self {
        DbClient { pool }
    }
}


#[derive(Debug, PartialEq, Eq)]
enum UpsertRoute {
    Insert,
    UpdateIfVersionMatches,
    OwnedByAnotherUser,
}

fn decide_upsert_route(existing_owner: Option<Uuid>, requesting_user: Uuid) -> UpsertRoute {
    match existing_owner {
        None => UpsertRoute::Insert,
        Some(owner) if owner == requesting_user => UpsertRoute::UpdateIfVersionMatches,
        Some(_) => UpsertRoute::OwnedByAnotherUser,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_upsert_route_inserts_when_no_existing_owner() {
        assert_eq!(
            decide_upsert_route(None, Uuid::new_v4()),
            UpsertRoute::Insert
        );
    }

    #[test]
    fn decide_upsert_route_checks_version_when_owner_matches() {
        let user = Uuid::new_v4();
        assert_eq!(
            decide_upsert_route(Some(user), user),
            UpsertRoute::UpdateIfVersionMatches
        );
    }

    #[test]
    fn decide_upsert_route_rejects_a_different_owner() {
        assert_eq!(
            decide_upsert_route(Some(Uuid::new_v4()), Uuid::new_v4()),
            UpsertRoute::OwnedByAnotherUser
        );
    }

}
