use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;
use uuid::Uuid;
use validator::validate_email;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct FeedId(pub Uuid);

impl Default for FeedId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

// TODO(vincent): should have specific types to differentiate between a not-fetched feed and a fetched feed.
//
// A not-fetched feed only has the URL
// A fetched feed has the other stuff as well
pub struct Feed {
    pub id: FeedId,
    pub url: Url,
    pub title: Option<String>,
    pub site_link: Option<String>, // TODO(vincent): should this be a Url ?
    pub description: Option<String>,
    pub created_at: time::OffsetDateTime,
    pub last_checked_at: Option<time::OffsetDateTime>,
}

#[cfg(test)]
mod tests {}
