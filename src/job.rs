use crate::configuration::JobConfig;
use crate::domain::UserId;
use crate::feed::{find_favicon, FeedId, ParsedFeed};
use crate::fetch_bytes;
use crate::run_group::Shutdown;
use blake2::{Blake2b512, Digest};
use feed_rs::model::Entry as RawFeedEntry;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::fmt;
use std::io::Write;
use tracing::{error, event, info, Level};
use url::Url;
use uuid::Uuid;

#[derive(Clone)]
pub struct JobId(pub Uuid);

impl Default for JobId {
    fn default() -> Self {
        Self(Uuid::new_v4())
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, thiserror::Error)]
enum RunJobError {
    #[error(transparent)]
    SQLx(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// The [`JobRunner`] runs all the background jobs.
///
/// It periodically does two things:
/// * manage jobs
/// * run jobs
///
/// Managing jobs means it will actually _add_ jobs to the queue if necessary; think background
/// refreshes of a feed, retries, etc.
///
/// Running jobs is self explanatory: it will pop jobs from the queue and run them, handling any
/// errors that occur.
pub struct JobRunner {
    http_client: reqwest::Client,
    config: JobConfig,
    pool: PgPool,
}

// Hardcode some limits on the number of jobs to run in one tick.
const MANAGE_JOBS_LIMIT: usize = 1;
const RUN_JOBS_LIMIT: usize = 1;

impl JobRunner {
    pub fn new(config: JobConfig, pool: PgPool) -> anyhow::Result<Self> {
        let http_client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .cookie_store(true)
            .build()?;

        Ok(Self {
            http_client,
            config,
            pool,
        })
    }

    pub async fn run(mut self, mut shutdown: Shutdown) -> anyhow::Result<()> {
        let mut interval = tokio::time::interval(self.config.run_interval());

        'outer_loop: loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    info!("job runner shutting down");
                    break 'outer_loop;
                },
                _ = interval.tick() => {
                    if let Err(err) = self.manage_jobs().await {
                        error!(%err, "failed while managing jobs");
                    }

                    if let Err(err) = self.run_jobs().await {
                        error!(%err, "failed while managing jobs");
                    }
                },
            }
        }

        Ok(())
    }

    #[tracing::instrument(name = "Manage jobs", level = "TRACE", skip(self))]
    async fn manage_jobs(&mut self) -> anyhow::Result<()> {
        let mut remaining = MANAGE_JOBS_LIMIT;

        create_fetch_favicons_jobs(&self.pool, &mut remaining).await?;

        Ok(())
    }

    #[tracing::instrument(name = "Run jobs", level = "TRACE", skip(self))]
    async fn run_jobs(&mut self) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        let records = sqlx::query!(
            r#"
            SELECT id, data, status as "status: String", attempts
            FROM jobs
            WHERE status = 'pending'
            FOR UPDATE
            SKIP LOCKED
            LIMIT $1
            "#,
            RUN_JOBS_LIMIT as i64,
        )
        .fetch_all(&mut tx)
        .await?;

        // TODO(vincent): use an exponential backoff
        const MAX_JOBS_ATTEMPTS: i32 = 5;

        for record in records {
            // 1) Sanity checks
            if record.attempts >= MAX_JOBS_ATTEMPTS {
                sqlx::query!("UPDATE jobs SET status = 'failed' WHERE id = $1", record.id)
                    .execute(&mut tx)
                    .await?;

                continue;
            }

            // 2) The job is valid; run it

            let job: Job = serde_json::from_value(record.data)?;
            let result: anyhow::Result<()> = match job {
                Job::FetchFavicon(data) => {
                    run_fetch_favicon_job(&self.http_client, &self.pool, data).await
                }
                Job::RefreshFeed(data) => {
                    run_refresh_feed_job(&self.http_client, &self.pool, data).await
                }
            };

            // 2) The job was run but it may have failed.
            // Update its status accordingly

            if let Err(err) = result {
                error!(%err, "job failed to run, retrying at a later time");

                sqlx::query!(
                    "UPDATE jobs SET attempts = attempts + 1 WHERE id = $1",
                    record.id
                )
                .execute(&mut tx)
                .await?;
            } else {
                // Job has finished successfully, delete it.

                sqlx::query!("DELETE FROM jobs WHERE id = $1", record.id)
                    .execute(&mut tx)
                    .await?;
            }
        }

        tx.commit().await?;

        Ok(())
    }
}

//
// Define the job types
//

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefreshFeedJobData {
    user_id: UserId,
    feed_id: FeedId,
    feed_url: Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchFaviconJobData {
    user_id: UserId,
    feed_id: FeedId,
    site_link: Url,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
enum Job {
    FetchFavicon(FetchFaviconJobData),
    RefreshFeed(RefreshFeedJobData),
}

impl Job {
    /// Returns the key of this [`Job`].
    ///
    /// The key is a [`Blake2b512`] hash computed on relevant data for each job type.
    ///
    /// This key is used to avoid adding the same job twice in the job queue.
    fn key(&self) -> [u8; 64] {
        let mut hasher = Blake2b512::new();

        match self {
            Job::FetchFavicon(data) => {
                write!(hasher, "fetch_favicon").unwrap();

                let feed_id_bytes: [u8; 8] = data.feed_id.into();
                hasher.update(feed_id_bytes);
            }
            Job::RefreshFeed(data) => {
                write!(hasher, "refresh_feed").unwrap();

                let feed_id_bytes: [u8; 8] = data.feed_id.into();
                hasher.update(feed_id_bytes);
            }
        }

        hasher.finalize().into()
    }
}

//
// Public API
//

#[derive(Debug, thiserror::Error)]
pub enum PostError {
    #[error(transparent)]
    SQLx(#[from] sqlx::Error),
}

type PostResult = Result<JobId, PostError>;

pub async fn post_fetch_favicon_job<'e, E>(
    executor: E,
    user_id: UserId,
    feed_id: FeedId,
    site_link: Url,
) -> PostResult
where
    E: sqlx::PgExecutor<'e>,
{
    post_job(
        executor,
        Job::FetchFavicon(FetchFaviconJobData {
            user_id,
            feed_id,
            site_link,
        }),
    )
    .await
}

pub async fn post_refresh_feed_job<'e, E>(
    executor: E,
    user_id: UserId,
    feed_id: FeedId,
    feed_url: Url,
) -> PostResult
where
    E: sqlx::PgExecutor<'e>,
{
    post_job(
        executor,
        Job::RefreshFeed(RefreshFeedJobData {
            user_id,
            feed_id,
            feed_url,
        }),
    )
    .await
}

/// Add a job to the job queue.
///
/// Each job has a key associated
///
/// # Errors
///
/// This function will return an error if .
#[tracing::instrument(
        name = "Add job",
        skip(executor, job),
        fields(
            id = tracing::field::Empty,
        ),
    )]
async fn post_job<'e, E>(executor: E, job: Job) -> Result<JobId, PostError>
where
    E: sqlx::PgExecutor<'e>,
{
    let job_id = JobId::default();

    tracing::Span::current().record("id", &tracing::field::display(&job_id));

    sqlx::query!(
        r#"
            INSERT INTO jobs(id, key, data) VALUES($1, $2, $3)
            ON CONFLICT DO NOTHING
            "#,
        &job_id.0,
        &job.key(),
        json!(job)
    )
    .execute(executor)
    .await?;

    Ok(job_id)
}

/// Add as many as `remaining` jobs to fetch the favicon of a feed.
///
/// # Errors
///
/// This function will return an error if there was an error adding a job to the queue
#[tracing::instrument(
    name = "Add fetch favicons jobs",
    level = "TRACE",
    skip(pool, remaining)
)]
async fn create_fetch_favicons_jobs(pool: &PgPool, remaining: &mut usize) -> anyhow::Result<()> {
    let records = sqlx::query!(
        r#"
            SELECT user_id, id, site_link
            FROM feeds f
            WHERE has_favicon IS NULL
            LIMIT $1
            "#,
        *remaining as i64,
    )
    .fetch_all(pool)
    .await?;

    let mut tx = pool.begin().await?;

    for record in records {
        let user_id = UserId(record.user_id);
        let feed_id = FeedId(record.id);
        let site_link = Url::parse(&record.site_link)?;

        post_job(
            &mut tx,
            Job::FetchFavicon(FetchFaviconJobData {
                user_id,
                feed_id,
                site_link,
            }),
        )
        .await?;
    }

    tx.commit().await?;

    Ok(())
}

#[tracing::instrument(
    name = "Run refresh feed job",
    skip(http_client, pool, data),
    fields(
        feed_id = %data.feed_id,
        feed_url = %data.feed_url,
    )
)]
async fn run_refresh_feed_job(
    http_client: &reqwest::Client,
    pool: &PgPool,
    data: RefreshFeedJobData,
) -> anyhow::Result<()> {
    let response_bytes = fetch_bytes(http_client, &data.feed_url)
        .await
        .map_err(Into::<anyhow::Error>::into)?;

    // 1) Try to parse as a feed
    let (feed, feed_entries) = {
        let mut raw_feed =
            feed_rs::parser::parse(&response_bytes[..]).map_err(Into::<anyhow::Error>::into)?;
        let raw_entries = std::mem::take(&mut raw_feed.entries);

        (
            ParsedFeed::from_raw_feed(&data.feed_url, raw_feed),
            raw_entries,
        )
    };

    event!(
        Level::INFO,
        title = %feed.title,
        entries = %feed_entries.len(),
        "found a raw feed",
    );

    // 2) Process all entries
    //
    // For every entry we check if it already exists in the database; to do that we use the
    // `external_id` field which maps to the `id` field of the [`feed_rs::model::Entry`] struct.
    // If the entry doesn't exist we insert it.

    let mut tx = pool.begin().await?;

    for entry in feed_entries {
        let entry = ParsedFeedEntry::from_raw_feed_entry(entry);

        if feed_entry_with_external_id_exists(&mut tx, data.user_id, &entry.external_id).await? {
            continue;
        }

        insert_feed_entry(&mut tx, &data.feed_id, entry).await?;
    }

    tx.commit().await?;

    Ok(())
}

#[tracing::instrument(
    name = "Run fetch favicon job",
    skip(http_client, pool, data),
    fields(
        feed_id = %data.feed_id,
        site_link = %data.site_link,
    )
)]
async fn run_fetch_favicon_job(
    http_client: &reqwest::Client,
    pool: &PgPool,
    data: FetchFaviconJobData,
) -> anyhow::Result<()> {
    let FetchFaviconJobData {
        user_id: _,
        feed_id,
        site_link,
    } = data;

    // 1) Find the favicon URL in the site. There might not be any.

    let favicon_url = find_favicon(http_client, &site_link).await;

    if let Some(url) = favicon_url {
        // Found the favicon URL in the document, fetch it and store it.

        let favicon = fetch_bytes(http_client, &url).await?;
        set_favicon(pool, &feed_id, Some(&favicon)).await?;
    } else {
        // No favicon URL in the document: try to fetch the relatively standard one at favicon.ico

        let favicon_url = site_link.join("/favicon.ico")?;
        let response = http_client.get(favicon_url.to_string()).send().await?;

        if response.status().is_success() {
            // Response is a 200, assume it's a valid favicon
            //
            // TODO(vincent): at some point we should try to detect an image in this

            let response_bytes = response.bytes().await?;
            set_favicon(pool, &feed_id, Some(&response_bytes)).await?;
        } else {
            // No favicon for you !

            set_favicon(pool, &feed_id, None).await?;
        }
    }

    Ok(())
}

#[tracing::instrument(
    name = "Set favicon",
    skip(pool, data),
    fields(
        feed_id = %feed_id,
    ),
)]
async fn set_favicon(pool: &PgPool, feed_id: &FeedId, data: Option<&[u8]>) -> anyhow::Result<()> {
    sqlx::query!(
        r#"
        UPDATE feeds
        SET site_favicon = $1, has_favicon = $2 WHERE id = $3
        "#,
        data,
        data.is_some(),
        &feed_id.0,
    )
    .execute(pool)
    .await?;

    Ok(())
}

/// Holds feed entry data parsed from a [`feed_rs::model::Entry`].
///
/// This means this struct should _not_ be used to represent data from the database.
struct ParsedFeedEntry {
    external_id: String,
    url: Option<Url>,
    title: String,
    summary: String,
    authors: Vec<String>,
}

impl ParsedFeedEntry {
    fn from_raw_feed_entry(entry: RawFeedEntry) -> Self {
        let url = None;
        // TODO(vincent): choose the correct one
        // let url = entry
        //     .links
        //     .into_iter()
        //     .map(|v| Url::parse(&v.href))
        //     .last()
        //     .ok();
        let title = entry.title.map(|v| v.content).unwrap_or_default();
        let summary = entry.summary.map(|v| v.content).unwrap_or_default();

        // TODO(vincent): see if there's anything better to do ?
        let authors: Vec<String> = entry
            .authors
            .into_iter()
            .map(|person| {
                if let Some(ref email) = person.email {
                    email.clone()
                } else {
                    person.name
                }
            })
            .collect();

        Self {
            external_id: entry.id,
            url,
            title,
            summary,
            authors,
        }
    }
}

/// Create a new feed entry in the database for this `user_id`.
#[tracing::instrument(
    name = "Insert feed entry",
    skip(executor, entry),
    fields(
        feed_id = %feed_id,
        url = tracing::field::Empty,
    )
)]
async fn insert_feed_entry<'e, E>(
    executor: E,
    feed_id: &FeedId,
    entry: ParsedFeedEntry,
) -> Result<(), sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    sqlx::query!(
        r#"
        INSERT INTO feed_entries(feed_id, external_id, title, url, created_at, authors, summary)
        VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
        &feed_id.0,
        &entry.external_id,
        &entry.title,
        entry.url.as_ref().map(Url::to_string),
        time::OffsetDateTime::now_utc(), // TODO(vincent): use the correct time
        &entry.authors,                  // TODO(vincent): rename creator to author ?
        &entry.summary,
    )
    .execute(executor)
    .await?;

    Ok(())
}

/// Check if a feed entry belonging to `user_id` with the given `external_id` already exists.
///
/// # Errors
///
/// This function will return an error if there's a SQL error.
async fn feed_entry_with_external_id_exists<'e, E>(
    executor: E,
    user_id: UserId,
    external_id: &str,
) -> Result<bool, sqlx::Error>
where
    E: sqlx::PgExecutor<'e>,
{
    let record = sqlx::query!(
        r#"
        SELECT fe.id FROM feed_entries fe
        INNER JOIN feeds f ON f.id = fe.feed_id
        INNER JOIN users u ON f.user_id = u.id
        WHERE u.id = $1 AND fe.external_id = $2
        "#,
        &user_id.0,
        external_id,
    )
    .fetch_optional(executor)
    .await?;

    Ok(record.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::get_feed_favicon;
    use crate::tests::{create_feed, create_user, get_pool};
    use select::document::Document;
    use select::predicate::Name;
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[derive(rust_embed::RustEmbed)]
    #[folder = "testdata/"]
    struct TestData;

    #[tokio::test]
    async fn fetch_favicon_job_should_work_when_link_exists_in_site() {
        let pool = get_pool().await;
        let http_client = reqwest::Client::new();

        // Setup a mock server that:
        // * responds with a basic HTML containing a favicon link
        // * responds with a fake favicon

        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        let mock_url = Url::parse(&mock_uri).unwrap();

        let fake_icon_data: &[u8] = b"\xde\xad\xbe\xef";

        Mock::given(path("/icon.png"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(fake_icon_data))
            .expect(1)
            .mount(&mock_server)
            .await;

        const HTML: &str = r#"
        <head>
        <link type="image/x-icon" href="/icon.png">
        </head>
        "#;

        Mock::given(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(HTML, "text/html"))
            .expect(1)
            .mount(&mock_server)
            .await;

        // Create a test user and feed

        let user_id = create_user(&pool).await;
        let feed_id =
            create_feed(&pool, user_id, &mock_url.join("/feed").unwrap(), &mock_url).await;

        // Run the job

        let data = FetchFaviconJobData {
            user_id,
            feed_id,
            site_link: mock_url,
        };

        run_fetch_favicon_job(&http_client, &pool, data)
            .await
            .unwrap();

        // Check the result

        let favicon = get_feed_favicon(&pool, user_id, &feed_id).await.unwrap();
        assert!(favicon.is_some());
        assert_eq!(fake_icon_data, &favicon.unwrap()[..]);
    }

    #[tokio::test]
    async fn image_links_in_summary_should_be_absolute() {
        let feed_data = TestData::get("tailscale_rss_feed_relative_image.xml")
            .unwrap()
            .data;

        let pool = get_pool().await;
        let http_client = reqwest::Client::new();

        // Setup a mock server that:
        // * responds with a XML feed

        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();
        let mock_url = Url::parse(&mock_uri).unwrap();

        Mock::given(path("/"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(feed_data, "text/html"))
            .expect(1)
            .mount(&mock_server)
            .await;

        // Create a test user and feed

        let user_id = create_user(&pool).await;
        let feed_id =
            create_feed(&pool, user_id, &mock_url.join("/feed").unwrap(), &mock_url).await;

        // Run the job

        let data = RefreshFeedJobData {
            user_id,
            feed_id,
            feed_url: mock_url,
        };

        run_refresh_feed_job(&http_client, &pool, data)
            .await
            .unwrap();

        // Check the result

        let records = sqlx::query!(
            r#"
            SELECT summary FROM feed_entries WHERE feed_id = $1
            "#,
            &feed_id.0,
        )
        .fetch_all(&pool)
        .await
        .expect("unable to get the feed entries");

        assert_eq!(records.len(), 1);

        // Find and check all images in the summary

        let summary = &records[0].summary;

        let document = Document::from(summary.as_str());

        for image in document.find(Name("img")) {
            let image_src = image.attr("src").unwrap_or_default();

            println!("image src: {:?}", image_src);

            assert!(image_src.starts_with("http"));
        }

        // println!("document: {:?}", document);
    }
}
