use crate::domain::UserId;
use crate::html::{fetch_document, find_link_in_document, FindLinkCriteria};
use crate::impl_typed_id;
use anyhow::Context;
use feed_rs::model::Feed as RawFeed;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{event, Level};
use url::Url;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct FeedId(pub i64);
impl_typed_id!(FeedId);

#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
pub struct FeedEntryId(pub i64);
impl_typed_id!(FeedEntryId);

/// Represents a feed entry.
#[derive(Debug)]
pub struct FeedEntry {
    pub id: FeedEntryId,
    pub url: Option<Url>,
    pub title: String,
    pub summary: String,
    pub created_at: time::OffsetDateTime,
    pub authors: Vec<String>,
}

impl FeedEntry {}

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

impl Feed {
    pub fn site_link_as_url(&self) -> Option<Url> {
        Url::parse(&self.site_link).ok()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

/// Holds feed data parsed from a [`feed_rs::model::Feed`].
///
/// This means this struct should _not_ be used to represent data from the database.
pub struct ParsedFeed {
    pub url: Url,
    pub title: String,
    pub site_link: String, // TODO(vincent): should this be a Url ?
    pub description: String,
}

impl ParsedFeed {
    pub fn parse(url: &Url, data: &[u8]) -> Result<Self, ParseError> {
        let raw_feed = feed_rs::parser::parse(data).map_err(Into::<anyhow::Error>::into)?;

        Ok(Self::from_raw_feed(url, raw_feed))
    }

    pub fn from_raw_feed(url: &Url, feed: RawFeed) -> Self {
        let site_link = feed
            .links
            .into_iter()
            .filter(|link| link.rel.is_none())
            .map(|link| link.href)
            .collect::<Vec<String>>()
            .remove(0);

        ParsedFeed {
            url: url.clone(),
            title: feed.title.map(|v| v.content).unwrap_or_default(),
            site_link,
            description: feed.description.map(|v| v.content).unwrap_or_default(),
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
pub async fn insert_feed(
    pool: &PgPool,
    user_id: &UserId,
    feed: &ParsedFeed,
) -> Result<FeedId, sqlx::Error> {
    // TODO(vincent): use a proper custom error type ?

    let result = sqlx::query!(
        r#"
        INSERT INTO feeds(user_id, url, title, site_link, description, added_at)
        VALUES ($1, $2, $3, $4, $5, $6)
        RETURNING id
        "#,
        &user_id.0,
        feed.url.to_string(),
        &feed.title,
        &feed.site_link,
        &feed.description,
        time::OffsetDateTime::now_utc(),
    )
    .fetch_one(pool)
    .await?;

    let feed_id = FeedId(result.id);

    Ok(feed_id)
}

#[tracing::instrument(name = "Get all feeds", skip(executor))]
pub async fn get_all_feeds<'e, E>(executor: E, user_id: &UserId) -> Result<Vec<Feed>, anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
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
    .fetch_all(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch all feeds")?;

    let mut feeds = Vec::with_capacity(records.len());
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

#[tracing::instrument(name = "Get feed", skip(executor))]
pub async fn get_feed<'e, E>(
    executor: E,
    user_id: &UserId,
    feed_id: &FeedId,
) -> Result<Option<Feed>, anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let record = sqlx::query!(
        r#"
        SELECT
            f.id, f.url, f.title, f.site_link, f.description,
            f.site_favicon, f.has_favicon,
            f.added_at
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.id = $2

        "#,
        &user_id.0,
        &feed_id.0,
    )
    .fetch_optional(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch feed")?;

    if let Some(record) = record {
        let url = Url::parse(&record.url)
            .map_err(Into::<anyhow::Error>::into)
            .context("unable to parse the stored feed URL")?;

        let feed = Feed {
            id: FeedId(record.id),
            url,
            title: record.title,
            site_link: record.site_link,
            description: record.description,
            site_favicon: record.site_favicon,
            added_at: record.added_at,
        };

        Ok(Some(feed))
    } else {
        Ok(None)
    }
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

/// Get all entries for the feed `feed_id`.
///
/// # Errors
///
/// This function will return an error if:
/// * a SQL error occurred
/// * the stored feed entry URL is invalid somehow
#[tracing::instrument(
    name = "Get feed entries",
    skip(executor),
    fields(
        user_id = %user_id,
        feed_id = %feed_id,
    ),
)]
pub async fn get_feed_entries<'e, E>(
    executor: E,
    user_id: &UserId,
    feed_id: &FeedId,
) -> Result<Vec<FeedEntry>, anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let records = sqlx::query!(
        r#"
        SELECT
          fe.id, fe.title, fe.url, fe.summary, fe.created_at, fe.authors
        FROM feeds f
        INNER JOIN feed_entries fe ON fe.feed_id = f.id
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.id = $2
        "#,
        &user_id.0,
        &feed_id.0,
    )
    .fetch_all(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch the feed entries")?;

    let mut entries = Vec::with_capacity(records.len());
    for record in records {
        entries.push(FeedEntry {
            id: FeedEntryId(record.id),
            url: parse_url_from_record(record.url)?,
            title: record.title,
            summary: record.summary,
            created_at: record.created_at,
            authors: record.authors.unwrap_or_default(),
        })
    }

    Ok(entries)
}

/// Get the entry `entry_id` for the feed `feed_id`.
///
/// # Errors
///
/// This function will return an error if:
/// * a SQL error occurred
/// * the stored feed entry URL is invalid somehow
#[tracing::instrument(
    name = "Get feed entry",
    skip(executor),
    fields(
        user_id = %user_id,
        feed_id = %feed_id,
        entry_id = %entry_id,
    ),
)]
pub async fn get_feed_entry<'e, E>(
    executor: E,
    user_id: &UserId,
    feed_id: &FeedId,
    entry_id: &FeedEntryId,
) -> Result<Option<FeedEntry>, anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let record = sqlx::query!(
        r#"
        SELECT
          fe.id, fe.title, fe.url, fe.summary, fe.created_at, fe.authors
        FROM feeds f
        INNER JOIN feed_entries fe ON fe.feed_id = f.id
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.id = $2 AND fe.id = $3
        "#,
        &user_id.0,
        &feed_id.0,
        &entry_id.0,
    )
    .fetch_optional(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch the feed entry")?;

    let result = if let Some(record) = record {
        Some(FeedEntry {
            id: FeedEntryId(record.id),
            url: parse_url_from_record(record.url)?,
            title: record.title,
            summary: record.summary,
            created_at: record.created_at,
            authors: record.authors.unwrap_or_default(),
        })
    } else {
        None
    };

    Ok(result)
}

#[tracing::instrument(
    name = "Mark a feed entry as read",
    skip(executor),
    fields(
        user_id = %user_id,
        feed_id = %feed_id,
        entry_id = %entry_id,
    ),
)]
pub async fn mark_feed_entry_as_read<'e, E>(
    executor: E,
    user_id: &UserId,
    feed_id: &FeedId,
    entry_id: &FeedEntryId,
) -> Result<(), anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query!(
        r#"
        UPDATE feed_entries
        SET read_at = now()
        FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.id = $2 AND feed_entries.id = $3
        "#,
        &user_id.0,
        &feed_id.0,
        &entry_id.0,
    )
    .execute(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to fetch the feed entry")?;

    Ok(())
}

/// Check if a feed with the given `url` already exists.
///
/// # Errors
///
/// This function will return an error if there's a SQL error.
pub async fn feed_with_url_exists<'e, E>(
    executor: E,
    user_id: &UserId,
    url: &Url,
) -> Result<bool, anyhow::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let record = sqlx::query!(
        r#"
        SELECT f.id FROM feeds f
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND f.url = $2
        "#,
        &user_id.0,
        url.to_string(),
    )
    .fetch_optional(executor)
    .await
    .map_err(Into::<anyhow::Error>::into)
    .context("unable to find the feed")?;

    Ok(record.is_some())
}

/// Parse a URL as it is stored in a record generated by sqlx.
///
/// # Errors
///
/// This function will return an error if the URL is invalid somehow.
fn parse_url_from_record(s: Option<String>) -> Result<Option<Url>, url::ParseError> {
    // This ugly thing goes from:
    // Option<String> to Option<&str> to Result<Option<Url>, _>

    let url_str: Option<&str> = s.as_deref();
    url_str.map(Url::parse).transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::fetch;
    use wiremock::matchers::any;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(rust_embed::RustEmbed)]
    #[folder = "testdata/"]
    struct TestData;

    #[test]
    fn feed_parse_should_work() {
        const DATA: &str = r#"
<rss xmlns:atom="http://www.w3.org/2005/Atom" version="2.0">
<channel>
<title>Foo</title>
<link>https://example.com/blog/</link>
<description>Foo</description>
<atom:link href="https://example.com/blog/index.xml" rel="self" type="application/rss+xml"/>
</channel>
</rss>"#;

        let url = Url::parse("https://example.com/blog/").unwrap();

        let feed = ParsedFeed::parse(&url, DATA.as_bytes()).unwrap();
        assert_eq!(feed.title, "Foo");
        assert_eq!(feed.site_link, "https://example.com/blog/");
        assert_eq!(feed.description, "Foo");
    }

    #[test]
    fn feed_parse_should_work_even_with_links_not_in_order() {
        // Move the relevant site link _after_ the "self" link.
        // We expect to ignore the self link.

        const DATA: &str = r#"
<rss xmlns:atom="http://www.w3.org/2005/Atom" version="2.0">
<channel>
<atom:link href="https://example.com/blog/index.xml" rel="self" type="application/rss+xml"/>
<title>Foo</title>
<description>Foo</description>
<link>https://example.com/blog/</link>
</channel>
</rss>"#;

        let url = Url::parse("https://example.com/blog/").unwrap();

        let feed = ParsedFeed::parse(&url, DATA.as_bytes()).unwrap();
        assert_eq!(feed.title, "Foo");
        assert_eq!(feed.site_link, "https://example.com/blog/");
        assert_eq!(feed.description, "Foo");
    }

    #[tokio::test]
    async fn find_feed_should_work() {
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        let mock_url = Url::parse(&mock_uri).unwrap();

        Mock::given(any())
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                TestData::get("tailscale_rss_feed.xml").unwrap().data,
                "application/xml",
            ))
            .expect(1)
            .mount(&mock_server)
            .await;

        let data = fetch(&mock_url).await;
        let found_feed = find_feed(&mock_url, &data[..]).unwrap();

        let feed = match found_feed {
            FoundFeed::Raw(raw_feed) => ParsedFeed::from_raw_feed(&mock_url, raw_feed),
            FoundFeed::Url(_) => panic!("expected a FoundFeed::Raw"),
        };

        assert_eq!("Blog on Tailscale", feed.title);
        assert_eq!("https://tailscale.com/blog/", feed.site_link);
        assert_eq!("Recent content in Blog on Tailscale", feed.description);
    }
}
