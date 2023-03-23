use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;
use validator::validate_email;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct UserId(pub Uuid);

impl Default for UserId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
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

impl fmt::Display for UserEmail {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub struct User {
    pub id: UserId,
    pub email: UserEmail,
}

impl User {}

#[cfg(test)]
mod tests {}
