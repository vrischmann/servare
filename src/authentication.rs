use crate::domain::{UserEmail, UserId};
use crate::telemetry::spawn_blocking_with_tracing;
use anyhow::anyhow;
use anyhow::Context;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use rand;
use secrecy::{ExposeSecret, Secret};

/// This error is returned when there is a problem authenticating.
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid credentials")]
    InvalidCredentials(#[source] anyhow::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Represents the credentials used for authentication.
pub struct Credentials {
    pub email: UserEmail,
    pub password: Secret<String>,
}

#[tracing::instrument(name = "Authenticate", skip(pool, credentials))]
pub async fn authenticate(
    pool: &sqlx::PgPool,
    credentials: Credentials,
) -> Result<UserId, AuthError> {
    let mut user_id = None;
    let mut expected_password_hash = Secret::new(
        "$argon2id$v=19$m=15000,t=2,p=1$\
gZiV/M1gPc22ElAH/Jh1Hw$\
CWOrkoo7oJBQ/iyh7uJ0LO2aLEfrHwTWllSAxT0zRno"
            .into(),
    );

    let stored_credentials = get_stored_credentials(pool, &credentials.email)
        .await
        .map_err(AuthError::Unexpected)?;

    if let Some(stored_credentials) = stored_credentials {
        user_id = Some(stored_credentials.0);
        expected_password_hash = stored_credentials.1;
    }

    //

    let verify_result = spawn_blocking_with_tracing(move || {
        verify_password_hash(expected_password_hash, credentials.password)
    })
    .await
    .context("Failed to spawn blocking task")
    .map_err(AuthError::Unexpected)?;

    verify_result?;

    //

    user_id
        .ok_or_else(|| anyhow!("Unknown username"))
        .map_err(AuthError::InvalidCredentials)
}

#[tracing::instrument(name = "Change password", skip(pool, password))]
pub async fn change_password(
    pool: &sqlx::PgPool,
    user_id: UserId,
    password: Secret<String>,
) -> Result<(), anyhow::Error> {
    // Compute the new hash
    let password_hash_result = spawn_blocking_with_tracing(move || compute_password_hash(password))
        .await
        .context("Failed to spawn blocking task")
        .map_err(Into::<anyhow::Error>::into)?;
    let password_hash = password_hash_result?;

    // Store it
    sqlx::query!(
        r#"
        UPDATE users
        SET password_hash = $1
        WHERE id = $2
        "#,
        password_hash.expose_secret(),
        &user_id.0,
    )
    .execute(pool)
    .await
    .context("Failed to update the users password")?;

    Ok(())
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

/// Get the stored credentials for a user email.
///
/// Returns a tuple of (user id, password hash) if the user exists.
/// Returns None otherwise.
#[tracing::instrument(name = "Get stored credentials", skip(pool))]
async fn get_stored_credentials(
    pool: &sqlx::PgPool,
    email: &UserEmail,
) -> Result<Option<(UserId, Secret<String>)>, anyhow::Error> {
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
