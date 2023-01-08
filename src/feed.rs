use crate::domain::UserId;
use anyhow::Context;
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

// TODO(vincent): should have specific types to differentiate between a not-fetched feed and a fetched feed.
//
// A not-fetched feed only has the URL
// A fetched feed has the other stuff as well
#[derive(Debug)]
pub struct Feed {
    pub id: FeedId,
    pub url: Url,
    pub title: String,
    pub site_link: String, // TODO(vincent): should this be a Url ?
    pub description: String,
    pub added_at: time::OffsetDateTime,
}

impl Feed {
    pub fn from_rss(channel: rss::Channel, url: &Url) -> Self {
        Feed {
            id: FeedId::default(),
            url: url.clone(),
            title: channel.title,
            site_link: channel.link,
            description: channel.description,
            added_at: time::OffsetDateTime::now_utc(),
        }
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

#[derive(Debug)]
pub enum FoundFeed {
    Url(Url),
    Rss(rss::Channel),
}

/// Find the feed at [`url`].
/// TODO(vincent): return all detected feeds
///
/// # Errors
///
/// This function will return an error if .
#[tracing::instrument(name = "Find feed", skip(url, data))]
pub fn find_feed(url: &Url, data: &[u8]) -> Result<FoundFeed, FindError> {
    // Try to parse as a RSS feed
    if let Ok(feed) = rss::Channel::read_from(data) {
        return Ok(FoundFeed::Rss(feed));
    }

    // If not an RSS feed, try to parse as a HTML document to find a link
    match select::document::Document::from_read(data) {
        Ok(document) => {
            for link in document.find(select::predicate::Name("link")) {
                let link_type = link.attr("type").unwrap_or_default();
                // We're looking for a link of type application/rss+xml.
                if link_type != "application/rss+xml" {
                    continue;
                }

                let link_href = link.attr("href").unwrap_or_default();

                // The href might be absolute
                let feed_url = if link_href.starts_with('/') {
                    url.join(link_href)
                } else {
                    Url::parse(link_href)
                }?;

                return Ok(FoundFeed::Url(feed_url));
            }
        }
        Err(err) => {
            event!(Level::ERROR, %err, "failed to parse HTML document");
        }
    }

    // Otherwise there is no feed

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
