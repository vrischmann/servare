use crate::domain::{User, UserId};
use anyhow::Context;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand;
use secrecy::{ExposeSecret, Secret};
use std::fmt;

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials(#[source] anyhow::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[tracing::instrument(name = "Authenticate")]
pub async fn authenticate() -> Result<User, AuthError> {
    todo!()
}

pub fn compute_password_hash(password: Secret<String>) -> Result<Secret<String>, anyhow::Error> {
    let salt = SaltString::generate(&mut rand::thread_rng());
    let hasher = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(15000, 2, 1, None).unwrap(),
    );

    let password_hash = hasher.hash_password(password.expose_secret().as_bytes(), &salt)?;
    let password_hash_string = password_hash.to_string();

    Ok(Secret::from(password_hash_string))
}

#[tracing::instrument(
    name = "Verify password hash",
    skip(expected_password_hash, password_candidate)
)]
fn verify_password_hash(
    expected_password_hash: Secret<String>,
    password_candidate: Secret<String>,
) -> Result<(), AuthError> {
    let expected_password_hash = PasswordHash::new(expected_password_hash.expose_secret())
        .context("Failed to parse hash in PHC string format")
        .map_err(AuthError::Unexpected)?;

    Argon2::default()
        .verify_password(
            password_candidate.expose_secret().as_bytes(),
            &expected_password_hash,
        )
        .context("failed to verify password")
        .map_err(AuthError::InvalidCredentials)
}

#[tracing::instrument(name = "Get stored credentials", skip(pool))]
async fn get_stored_credentials<T>(
    pool: &sqlx::PgPool,
    email: T,
) -> Result<Option<(UserId, Secret<String>)>, anyhow::Error>
where
    T: fmt::Debug + AsRef<str>,
{
    let row = sqlx::query!(
        r#"
        SELECT id, password_hash
        FROM users
        WHERE email = $1
        "#,
        email.as_ref(),
    )
    .fetch_optional(pool)
    .await
    .context("Failed to perform a query to retrieve stored credentials")?;

    match row {
        Some(row) => {
            let user_id = UserId(row.id);
            let password_hash = Secret::new(row.password_hash);

            let result = (user_id, password_hash);

            Ok(Some(result))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::get_configuration;
    use crate::domain::UserEmail;
    use crate::startup::get_connection_pool;
    use fake::faker::internet::en::SafeEmail as FakerSafeEmail;
    use fake::Fake;

    async fn get_pool() -> sqlx::PgPool {
        let config = get_configuration().unwrap();
        get_connection_pool(&config.database).await.unwrap()
    }

    #[tokio::test]
    async fn get_stored_credentials_for_non_existing_user_should_return_none() {
        let pool = get_pool().await;

        let email = UserEmail::parse(FakerSafeEmail().fake()).unwrap();

        let credentials = get_stored_credentials(&pool, email.as_ref()).await.unwrap();
        assert!(credentials.is_none());
    }

    #[tokio::test]
    async fn get_stored_credentials_for_existing_user_should_return_its_id() {
        let pool = get_pool().await;

        let user_id = UserId::default();
        let email = UserEmail::parse(FakerSafeEmail().fake()).unwrap();

        // This is a quick hack to set the login methods for this user
        {
            sqlx::query!(
                r#"
                INSERT INTO users(id, email, password_hash)
                VALUES($1, $2, $3)
                "#,
                &user_id.0,
                &email.0,
                "foobar",
            )
            .execute(&pool)
            .await
            .unwrap();
        }

        let credentials = get_stored_credentials(&pool, email.as_ref()).await.unwrap();
        assert!(credentials.is_some());

        let credentials = credentials.unwrap();
        assert_eq!(user_id, credentials.0);
        assert_eq!("foobar", credentials.1.expose_secret());
    }
}
