use uuid::Uuid;

#[derive(sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

impl Default for UserId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserEmail(pub String);

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

#[cfg(test)]
mod tests {

}
