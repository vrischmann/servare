use uuid::Uuid;

#[derive(sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserId(Uuid);

#[derive(sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserEmail(String);

impl AsRef<str> for UserEmail {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

pub struct User {
    pub id: UserId,
    pub email: UserEmail,
}

impl User {}
