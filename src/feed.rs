use crate::domain::UserId;
use anyhow::Context;
use feed_rs::model::Feed as RawFeed;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{event, Level};
use url::Url;
use uuid::Uuid;

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct FeedId(pub Uuid);

impl Default for FeedId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

#[derive(Debug)]
pub struct Feed {
    pub id: FeedId,
    pub url: Url,
    pub title: String,
    pub site_link: String, // TODO(vincent): should this be a Url ?
    pub description: String,
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

            for link in document.find(select::predicate::Name("link")) {
                let link_href = link.attr("href").unwrap_or_default();
                // The href might be absolute
                let feed_url = if !link_href.starts_with("http") {
                    url.join(link_href)
                } else {
                    Url::parse(link_href)
                }?;

                let link_type = link.attr("type").unwrap_or_default();
                if link_type == "application/rss+xml" || link_type == "application/atom+xml" {
                    return Ok(FoundFeed::Url(feed_url));
                }
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
pub async fn insert_feed(pool: &PgPool, user_id: &UserId, feed: Feed) -> Result<(), sqlx::Error> {
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
            f.added_at
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1
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
            added_at: record.added_at,
        });
    }

    Ok(feeds)
}
