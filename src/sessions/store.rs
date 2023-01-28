use actix_session::storage::{LoadError, SaveError, UpdateError};
use actix_session::storage::{SessionKey, SessionStore};
use actix_web::cookie::time::Duration;
use anyhow::anyhow;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct PgSessionStore {
    pool: PgPool,
}

#[derive(Debug)]
pub struct CleanupConfig {
    enabled: bool,
    interval: time::Duration,
}

impl CleanupConfig {
    pub fn new(enabled: bool, interval: time::Duration) -> Self {
        Self { enabled, interval }
    }
}

impl Default for CleanupConfig {
    fn default() -> Self {
        Self::new(false, time::Duration::seconds(30))
    }
}

impl PgSessionStore {
    pub fn new(pool: PgPool, cleanup_config: CleanupConfig) -> Self {
        // Launch a background cleanup task if necessary
        if cleanup_config.enabled {
            let cleanup_pool = pool.clone();
            tokio::spawn(async move {
                clean_sessions(cleanup_pool, cleanup_config.interval).await;
            });
        }

        Self { pool }
    }
}

async fn clean_sessions(pool: PgPool, clean_interval: time::Duration) {
    let mut interval = tokio::time::interval(clean_interval.unsigned_abs());
    loop {
        let _ = interval.tick().await;
        let now = time::OffsetDateTime::now_utc();

        let result = sqlx::query!("DELETE FROM sessions WHERE expires_at <= $1", now)
            .execute(&pool)
            .await;
        match result {
            Ok(result) => {
                tracing::debug!(cleaned = %result.rows_affected(), "sessions cleanup done");
            }
            Err(err) => match err {
                sqlx::Error::PoolClosed => {
                    tracing::debug!("pool is closed");
                    return;
                }
                _ => tracing::error!(?err, "unable to cleanup sessions"),
            },
        }
    }
}

type SessionState = HashMap<String, String>;

#[async_trait::async_trait(?Send)]
impl SessionStore for PgSessionStore {
    async fn load(&self, session_key: &SessionKey) -> Result<Option<SessionState>, LoadError> {
        let session_id = session_key_to_uuid(session_key).map_err(LoadError::Other)?;

        // Fetch the state
        let row = sqlx::query!(
            "SELECT state, expires_at FROM sessions WHERE id = $1",
            session_id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .map_err(LoadError::Other)?;

        let (session_state_data, expires_at) = match row {
            None => return Ok(None),
            Some(row) => (row.state, row.expires_at),
        };

        // Check the expiry date
        let now = time::OffsetDateTime::now_utc();
        if expires_at < now {
            return Ok(None);
        }

        tracing::trace!(now = %now, expires_at = %expires_at, session_id = %session_id, "loaded state");

        let state = serde_json::from_value(session_state_data)
            .map_err(Into::<anyhow::Error>::into)
            .map_err(LoadError::Deserialization)?;

        Ok(state)
    }

    async fn save(
        &self,
        session_state: SessionState,
        ttl: &Duration,
    ) -> Result<SessionKey, SaveError> {
        // Setup

        let session_id = Uuid::new_v4();
        let state = serde_json::to_value(&session_state)
            .map_err(Into::into)
            .map_err(SaveError::Serialization)?;

        let created_at = time::OffsetDateTime::now_utc();
        let expires_at = created_at
            .checked_add(*ttl)
            .ok_or_else(|| SaveError::Other(anyhow!("unable to compute expiry timestamp")))?;

        // Save data

        sqlx::query!(
            "INSERT INTO sessions(id, state, created_at, expires_at) VALUES($1, $2, $3, $4)",
            session_id,
            state,
            created_at,
            expires_at,
        )
        .execute(&self.pool)
        .await
        .map_err(Into::<anyhow::Error>::into)
        .map_err(SaveError::Other)?;

        // Return the session key

        let session_key = uuid_to_session_key(session_id).map_err(SaveError::Other)?;

        Ok(session_key)
    }

    async fn update(
        &self,
        session_key: SessionKey,
        session_state: SessionState,
        ttl: &Duration,
    ) -> Result<SessionKey, UpdateError> {
        // Setup

        let session_id = session_key_to_uuid(&session_key).map_err(UpdateError::Other)?;
        let state = serde_json::to_value(&session_state)
            .map_err(Into::into)
            .map_err(UpdateError::Serialization)?;
        let expires_at = time::OffsetDateTime::now_utc()
            .checked_add(*ttl)
            .ok_or_else(|| UpdateError::Other(anyhow!("unable to compute expiry timestamp")))?;

        // Check if the session exists
        let row = sqlx::query!("SELECT 1 AS n FROM sessions WHERE id = $1", session_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(Into::into)
            .map_err(UpdateError::Other)?;

        match row {
            Some(_) => {
                // The session exists, update it

                sqlx::query!(
                    "UPDATE sessions SET state = $1, expires_at = $2 WHERE id = $3",
                    state,
                    expires_at,
                    session_id,
                )
                .execute(&self.pool)
                .await
                .map_err(Into::into)
                .map_err(UpdateError::Other)?;

                Ok(session_key)
            }
            None => {
                // If the session doesn't exist fall back to calling save

                self.save(session_state, ttl)
                    .await
                    .map_err(|err| match err {
                        SaveError::Serialization(err) => UpdateError::Serialization(err),
                        SaveError::Other(err) => UpdateError::Other(err),
                    })
            }
        }
    }

    async fn delete(&self, session_key: &SessionKey) -> Result<(), anyhow::Error> {
        let session_id = session_key_to_uuid(session_key)?;

        sqlx::query!("DELETE FROM sessions WHERE id = $1", session_id)
            .execute(&self.pool)
            .await
            .map_err(Into::into)
            .map_err(UpdateError::Other)?;

        Ok(())
    }
}

fn uuid_to_session_key(id: Uuid) -> Result<SessionKey, anyhow::Error> {
    let session_key_string = id.to_string();

    let res: Result<SessionKey, _> = session_key_string.try_into();
    let session_key = res.map_err(Into::<anyhow::Error>::into)?;

    Ok(session_key)
}

fn session_key_to_uuid(session_key: &SessionKey) -> Result<Uuid, anyhow::Error> {
    Uuid::try_parse(session_key.as_ref()).map_err(Into::<anyhow::Error>::into)
}

#[cfg(test)]
mod tests {
    use super::{uuid_to_session_key, CleanupConfig, PgSessionStore};
    use actix_session::storage::SessionStore;
    use actix_web::cookie::time::Duration;
    use sqlx::PgPool;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_state() -> HashMap<String, String> {
        HashMap::from([("foo".into(), "bar".into()), ("bar".into(), "baz".into())])
    }

    #[sqlx::test]
    async fn loading_a_missing_session_returns_none(pool: PgPool) {
        let store = PgSessionStore::new(pool, CleanupConfig::default());

        let session_key = uuid_to_session_key(Uuid::new_v4()).unwrap();

        let result = store
            .load(&session_key)
            .await
            .expect("Unable to load the session");
        assert!(result.is_none());
    }

    #[sqlx::test]
    async fn loading_an_existing_session_returns_its_state(pool: PgPool) {
        let store = PgSessionStore::new(pool, CleanupConfig::default());
        let state = make_state();

        let session_key = store
            .save(state.clone(), &Duration::seconds(10))
            .await
            .expect("Unable to save the session");
        assert!(session_key.as_ref().len() == 36);

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session")
            .unwrap();

        assert_eq!(state, loaded_state);
    }

    #[sqlx::test]
    async fn updating_then_loading_an_existing_session_returns_its_updated_state(pool: PgPool) {
        let store = PgSessionStore::new(pool, CleanupConfig::default());
        let mut state = make_state();

        let session_key = store
            .save(state.clone(), &Duration::seconds(10))
            .await
            .expect("Unable to save the session");

        state.insert("name".to_string(), "vincent".to_string());
        let session_key = store
            .update(session_key, state.clone(), &Duration::seconds(10))
            .await
            .expect("Unable to update the session");

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session")
            .unwrap();

        assert_eq!(state, loaded_state);
    }

    #[sqlx::test]
    async fn loading_a_session_saved_with_a_negative_ttl_returns_none(pool: PgPool) {
        let store = PgSessionStore::new(pool, CleanupConfig::default());
        let state = make_state();

        let session_key = store
            .save(state.clone(), &Duration::seconds(-10))
            .await
            .expect("Unable to save the session");

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session");

        assert!(loaded_state.is_none());
    }

    #[sqlx::test]
    async fn loading_a_deleted_session_returns_none(pool: PgPool) {
        let store = PgSessionStore::new(pool, CleanupConfig::default());
        let state = make_state();

        let session_key = store
            .save(state.clone(), &Duration::seconds(10))
            .await
            .expect("Unable to save the session");

        store
            .delete(&session_key)
            .await
            .expect("Unable to delete the sesssion");

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session");

        assert!(loaded_state.is_none());
    }

    #[sqlx::test]
    async fn loading_an_expired_session_returns_none(pool: PgPool) {
        let store =
            PgSessionStore::new(pool, CleanupConfig::new(true, Duration::milliseconds(100)));
        let state = make_state();

        let session_key = store
            .save(state.clone(), &Duration::milliseconds(50))
            .await
            .expect("Unable to save the session");

        tokio::time::sleep(Duration::milliseconds(200).unsigned_abs()).await;

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session");

        assert!(loaded_state.is_none(), "found state for {session_key:?}");
    }

    #[sqlx::test]
    async fn updating_a_non_existing_session_creates_it(pool: PgPool) {
        let store =
            PgSessionStore::new(pool, CleanupConfig::new(true, Duration::milliseconds(100)));

        let session_key = uuid_to_session_key(Uuid::new_v4()).unwrap();
        let state = make_state();

        let session_key = store
            .update(session_key, state.clone(), &Duration::milliseconds(1000))
            .await
            .expect("Unable to save the session");

        tokio::time::sleep(Duration::milliseconds(200).unsigned_abs()).await;

        let loaded_state = store
            .load(&session_key)
            .await
            .expect("Unable to load the session");

        assert!(loaded_state.is_some(), "found no state for {session_key:?}");
    }
}
