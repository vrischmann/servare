use crate::domain::UserId;
use crate::html::{fetch_document, find_link_in_document, FindLinkCriteria};
use anyhow::Context;
use feed_rs::model::Feed as RawFeed;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::fmt;
use tracing::{event, Level};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct FeedId(pub Uuid);

impl From<Uuid> for FeedId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl Default for FeedId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl AsRef<[u8]> for FeedId {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl fmt::Display for FeedId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug)]
pub struct Feed {
    pub id: FeedId,
    pub url: Url,
    pub title: String,
    pub site_link: String, // TODO(vincent): should this be a Url ?
    pub description: String,
    pub site_favicon: Option<Vec<u8>>,
    pub added_at: time::OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

impl Feed {
    pub fn parse(url: &Url, data: &[u8]) -> Result<Self, ParseError> {
        let raw_feed = feed_rs::parser::parse(data).map_err(Into::<anyhow::Error>::into)?;

        Ok(Self::from_raw_feed(url, raw_feed))
    }

    pub fn from_raw_feed(url: &Url, feed: RawFeed) -> Self {
        // TODO(vincent): this is broken
        let site_link = &feed.links[0].href;

        Feed {
            id: FeedId::default(),
            url: url.clone(),
            title: feed.title.map(|v| v.content).unwrap_or_default(),
            site_link: site_link.clone(),
            description: feed.description.map(|v| v.content).unwrap_or_default(),
            site_favicon: None,
            added_at: time::OffsetDateTime::now_utc(),
        }
    }

    pub fn site_link_as_url(&self) -> Option<Url> {
        Url::parse(&self.site_link).ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FindError {
    #[error("No feed")]
    NoFeed,
    #[error(transparent)]
    URLInvalid(#[from] url::ParseError),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum FoundFeed {
    Url(Url),
    Raw(RawFeed),
}

/// Find the feed at [`url`].
/// TODO(vincent): return all detected feeds
///
/// # Errors
///
/// This function will return an error if .
#[tracing::instrument(name = "Find feed", skip(url, data))]
pub fn find_feed(url: &Url, data: &[u8]) -> Result<FoundFeed, FindError> {
    // Try to parse as a feed
    if let Ok(feed) = feed_rs::parser::parse(data) {
        event!(Level::INFO, "found a raw feed");
        return Ok(FoundFeed::Raw(feed));
    }

    // If not a valid feed, try to parse as a HTML document to find a link
    match select::document::Document::from_read(data) {
        Ok(document) => {
            event!(Level::INFO, "found a HTML document, need parsing");

            let criteria = &[
                FindLinkCriteria::Type("application/rss+xml"),
                FindLinkCriteria::Type("application/atom+xml"),
            ];

            if let Some(url) = find_link_in_document(url, &document, criteria) {
                return Ok(FoundFeed::Url(url));
            }
        }
        Err(err) => {
            event!(Level::ERROR, %err, "failed to parse HTML document");
        }
    }

    // Otherwise there is no feed

    event!(Level::INFO, url = %url, "found no feed");

    Err(FindError::NoFeed)
}

/// Create a new feed in the database for this `user_id` with the URL `url`.
#[tracing::instrument(
    name = "Insert feed",
    skip(pool, feed),
    fields(
        url = tracing::field::Empty,
    )
)]
pub async fn insert_feed(pool: &PgPool, user_id: &UserId, feed: &Feed) -> Result<(), sqlx::Error> {
    // TODO(vincent): use a proper custom error type ?

    sqlx::query!(
        r#"
        INSERT INTO feeds(id, user_id, url, title, site_link, description, added_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        &feed.id.0,
        &user_id.0,
        feed.url.to_string(),
        &feed.title,
        &feed.site_link,
        &feed.description,
        time::OffsetDateTime::now_utc(),
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[tracing::instrument(name = "Get all feeds", skip(pool))]
pub async fn get_all_feeds(pool: &PgPool, user_id: &UserId) -> Result<Vec<Feed>, anyhow::Error> {
    let records = sqlx::query!(
        r#"
        SELECT
            f.id, f.url, f.title, f.site_link, f.description,
            f.site_favicon, f.has_favicon,
            f.added_at
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1
        ORDER BY f.added_at DESC
        "#,
        &user_id.0,
    )
    .fetch_all(pool)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch all feeds")?;

    let mut feeds = Vec::new();
    for record in records {
        let url = Url::parse(&record.url)
            .map_err(Into::<anyhow::Error>::into)
            .context("stored feed URL is invalid")?;

        feeds.push(Feed {
            id: FeedId(record.id),
            url,
            title: record.title,
            site_link: record.site_link,
            description: record.description,
            site_favicon: record.site_favicon,
            added_at: record.added_at,
        });
    }

    Ok(feeds)
}

#[tracing::instrument(
    name = "Get feed favicon",
    skip(pool),
    fields(
        user_id = %user_id,
        feed_id = %feed_id,
    ),
)]
pub async fn get_feed_favicon(
    pool: &PgPool,
    user_id: &UserId,
    feed_id: &FeedId,
) -> Result<Option<Vec<u8>>, anyhow::Error> {
    let result = sqlx::query!(
        r#"
        SELECT f.site_favicon
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.id = $2
        "#,
        &user_id.0,
        &feed_id.0,
    )
    .fetch_optional(pool)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch the feed favicon")?;

    if let Some(record) = result {
        Ok(record.site_favicon)
    } else {
        Ok(None)
    }
}

/// Given a website at [`url`], try to find its favicon URL.
///
/// Returns ['None'] if no favicon is found.
#[tracing::instrument(name = "Find favicon", skip(client, url))]
pub async fn find_favicon(client: &reqwest::Client, url: &Url) -> Option<Url> {
    // 1) First try to find the favicon in the HTML document

    match fetch_document(client, url).await {
        Ok(document) => {
            event!(Level::DEBUG, "found a HTML document");

            let criterias = &[
                FindLinkCriteria::Type("image/x-icon"),
                FindLinkCriteria::Type("image/icon"),
                FindLinkCriteria::Rel("icon"),
            ];
            find_link_in_document(url, &document, criterias)
        }
        Err(err) => {
            event!(Level::ERROR, %err, "failed to parse URL as an HTML document");
            None
        }
    }
}
