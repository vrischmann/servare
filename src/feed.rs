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
pub struct Feed {
    pub id: FeedId,
    pub url: Url,
    pub title: String,
    pub site_link: String, // TODO(vincent): should this be a Url ?
    pub description: String,
    pub added_at: time::OffsetDateTime,
}

#[derive(Debug, thiserror::Error)]
pub enum FetchError {
    #[error("Invalid URL")]
    InvalidURL(#[source] url::ParseError),
    #[error("RSS error")]
    RSS(#[source] rss::Error),
    #[error("HTTP request error")]
    Reqwest(#[source] reqwest::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[tracing::instrument(name = "Fetch feed", skip(client))]
pub async fn fetch_feed(client: &reqwest::Client, url: &Url) -> Result<Feed, FetchError> {
    // Fetch the feed data
    let response_result = client.get(url.to_string()).send().await;

    let response = response_result.map_err(FetchError::Reqwest)?;
    let response_bytes = response.bytes().await.map_err(FetchError::Reqwest)?;

    // Parse the feed
    let rss_channel = rss::Channel::read_from(&response_bytes[..]).map_err(FetchError::RSS)?;

    let feed = Feed {
        id: FeedId::default(),
        url: url.clone(),
        title: rss_channel.title,
        site_link: rss_channel.link,
        description: rss_channel.description,
        added_at: time::OffsetDateTime::now_utc(),
    };

    event!(Level::INFO,
        title = %feed.title,
        site_link = %feed.site_link,
        "Fetched feed",
    );

    Ok(feed)
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
        INSERT INTO feeds(id, user_id, url, added_at)
        VALUES ($1, $2, $3, $4)
        "#,
        &feed.id.0,
        &user_id.0,
        feed.url.to_string(),
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
