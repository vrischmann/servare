use crate::domain::User;

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
