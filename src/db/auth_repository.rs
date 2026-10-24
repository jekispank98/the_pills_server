use super::person_repository::PERSON_COLUMNS;
use super::DbClient;
use crate::models::person::{Person, PersonRelation};
use crate::models::user::User;
use chrono::NaiveDateTime;
use sqlx::{query_as, Error};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Default)]
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
        verification_token_expires_at,
        subscribed,
        verified,
        created_at,
        updated_at,
        verification_token,
        google_sub,
        photo_url,
        time_zone,
        locale
    FROM users
"#;

#[derive(Debug, Clone, Copy, PartialEq)]
enum FilterBind<'a> {
    Uuid(Uuid),
    Text(&'a str),
}

fn build_filter_query<'a>(filters: &UserQueryFilters<'a>) -> Option<(String, Vec<FilterBind<'a>>)> {
    let mut conditions = Vec::new();
    let mut binds: Vec<FilterBind<'a>> = Vec::new();

    if let Some(user_id) = filters.user_id {
        binds.push(FilterBind::Uuid(user_id));
        conditions.push(format!("id = ${}", binds.len()));
    }
    if let Some(name) = filters.name {
        binds.push(FilterBind::Text(name));
        conditions.push(format!("name = ${}", binds.len()));
    }
    if let Some(email) = filters.email {
        binds.push(FilterBind::Text(email));
        conditions.push(format!("email = ${}", binds.len()));
    }
    if let Some(token) = filters.token {
        binds.push(FilterBind::Text(token));
        conditions.push(format!("verification_token = ${}", binds.len()));
    }

    if conditions.is_empty() {
        return None;
    }

    Some((conditions.join(" AND "), binds))
}

#[derive(Debug, Clone, Default)]
pub struct UserProfileUpdate {
    pub name: Option<String>,
    pub photo_url: Option<String>,
    pub time_zone: Option<String>,
    pub locale: Option<String>,
}

#[async_trait::async_trait]
pub trait AuthentificationTrait {
    async fn get_user(
        &self,
        user_id: Option<Uuid>,
        name: Option<&str>,
        email: Option<&str>,
        token: Option<&str>,
    ) -> Result<Option<User>, Error>;

    async fn register_user_with_self_person(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<(User, Person), Error>;

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error>;

    async fn delete_user_by_id(&self, user_id: Uuid) -> Result<bool, Error>;

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error>;

    async fn update_verification_token_expires_at(
        &self,
        expires_at: NaiveDateTime,
        id: Uuid,
    ) -> Result<bool, Error>;

    async fn update_user_profile(
        &self,
        id: Uuid,
        update: UserProfileUpdate,
    ) -> Result<Option<User>, Error>;

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

    async fn register_user_with_self_person(
        &self,
        name: String,
        email: String,
        password: String,
        verification_token_expires_at: NaiveDateTime,
        verification_token: String,
    ) -> Result<(User, Person), Error> {
        let mut tx = self.pool.begin().await?;

        let user_query = r#"
    INSERT INTO users(name, email, password, verification_token_expires_at, subscribed, verification_token)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING id, name, email, password, verification_token_expires_at, subscribed, verified,
              created_at, updated_at, verification_token, google_sub, photo_url, time_zone, locale"#;

        let user = query_as::<_, User>(user_query)
            .bind(&name)
            .bind(email)
            .bind(password)
            .bind(verification_token_expires_at)
            .bind(false)
            .bind(verification_token)
            .fetch_one(&mut *tx)
            .await?;

        let person_query = format!(
            r#"
    INSERT INTO persons(id, user_account_id, name, relation, is_user)
    VALUES ($1, $2, $3, $4, true)
    RETURNING {PERSON_COLUMNS}
"#
        );

        let person = query_as::<_, Person>(&person_query)
            .bind(Uuid::new_v4())
            .bind(user.id)
            .bind(&name)
            .bind(PersonRelation::Myself)
            .fetch_one(&mut *tx)
            .await?;

        tx.commit().await?;

        Ok((user, person))
    }

    async fn check_is_user_exist(&self, email: &str) -> Result<Option<User>, Error> {
        let query = format!("{USER_SELECT_QUERY} WHERE email = $1");

        query_as::<_, User>(&query)
            .bind(email)
            .fetch_optional(&self.pool)
            .await
    }

    async fn delete_user_by_id(&self, user_id: Uuid) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE id = $1";

        let result = sqlx::query(query).bind(user_id).execute(&self.pool).await?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_user_by_email(&self, email: &str) -> Result<bool, Error> {
        let query = "DELETE FROM users WHERE email = $1";

        let result = sqlx::query(query).bind(email).execute(&self.pool).await?;

        Ok(result.rows_affected() > 0)
    }

    async fn update_verification_token_expires_at(
        &self,
        expires_at: NaiveDateTime,
        id: Uuid,
    ) -> Result<bool, Error> {
        let query = "UPDATE users SET verification_token_expires_at = $1 WHERE id = $2";

        let result = sqlx::query(query)
            .bind(expires_at)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn update_user_profile(
        &self,
        id: Uuid,
        update: UserProfileUpdate,
    ) -> Result<Option<User>, Error> {
        let query = r#"
            UPDATE users
            SET
                name = COALESCE($1, name),
                photo_url = COALESCE($2, photo_url),
                time_zone = COALESCE($3, time_zone),
                locale = COALESCE($4, locale),
                updated_at = NOW()
            WHERE id = $5
            RETURNING id, name, email, password, verification_token_expires_at, subscribed, verified,
                      created_at, updated_at, verification_token, google_sub, photo_url, time_zone, locale
            "#;

        query_as::<_, User>(query)
            .bind(update.name)
            .bind(update.photo_url)
            .bind(update.time_zone)
            .bind(update.locale)
            .bind(id)
            .fetch_optional(&self.pool)
            .await
    }

    async fn find_user_with_filters(
        &self,
        filters: UserQueryFilters<'_>,
    ) -> Result<Option<User>, Error> {
        let Some((where_clause, binds)) = build_filter_query(&filters) else {
            return Ok(None);
        };

        let query = format!("{USER_SELECT_QUERY} WHERE {where_clause}");
        let mut db_query = query_as::<_, User>(&query);
        for bind in binds {
            db_query = match bind {
                FilterBind::Uuid(v) => db_query.bind(v),
                FilterBind::Text(v) => db_query.bind(v),
            };
        }

        db_query.fetch_optional(&self.pool).await
    }

    async fn verified_token(&self, token: &str) -> Result<(), Error> {
        sqlx::query(
            r#"
            UPDATE users
            SET
                verified = true,
                updated_at = NOW(),
                verification_token = NULL,
                verification_token_expires_at = NULL
            WHERE verification_token = $1
            "#,
        )
        .bind(token)
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_filters_means_no_query() {
        assert_eq!(build_filter_query(&UserQueryFilters::default()), None);
    }

    #[test]
    fn single_filter_uses_first_placeholder() {
        let filters = UserQueryFilters {
            token: Some("tok"),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "verification_token = $1");
        assert_eq!(binds, vec![FilterBind::Text("tok")]);
    }

    #[test]
    fn user_id_is_bound_as_uuid_not_text() {
        let id = Uuid::new_v4();
        let filters = UserQueryFilters {
            user_id: Some(id),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1");
        assert_eq!(binds, vec![FilterBind::Uuid(id)]);
    }

    #[test]
    fn multiple_filters_are_combined_with_and_not_or() {
        let id = Uuid::new_v4();
        let filters = UserQueryFilters {
            user_id: Some(id),
            token: Some("tok"),
            ..Default::default()
        };
        let (clause, binds) = build_filter_query(&filters).unwrap();
        assert_eq!(clause, "id = $1 AND verification_token = $2");
        assert_eq!(binds, vec![FilterBind::Uuid(id), FilterBind::Text("tok")]);
    }

    #[test]
    fn all_filters_get_sequential_placeholders() {
        let filters = UserQueryFilters {
            user_id: Some(Uuid::new_v4()),
            name: Some("n"),
            email: Some("e"),
            token: Some("t"),
        };
        let (clause, _) = build_filter_query(&filters).unwrap();
        assert_eq!(
            clause,
            "id = $1 AND name = $2 AND email = $3 AND verification_token = $4"
        );
    }
}