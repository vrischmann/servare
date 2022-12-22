use anyhow::anyhow;
use uuid::Uuid;
use validator::validate_email;

#[derive(Clone, Debug, sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserId(pub Uuid);

impl Default for UserId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Clone, Debug, sqlx::Type, serde::Deserialize, serde::Serialize)]
#[sqlx(transparent)]
pub struct UserEmail(pub String);

impl UserEmail {
    pub fn parse(s: String) -> anyhow::Result<Self> {
        if validate_email(&s) {
            Ok(Self(s))
        } else {
            Err(anyhow!("{} is not a valid email", s))
        }
    }
}

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
mod tests {}
