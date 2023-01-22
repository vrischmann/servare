use crate::configuration::JobConfig;
use crate::feed::{find_favicon, insert_feed_entry, Feed, FeedEntry, FeedId};
use crate::fetch_bytes;
use crate::shutdown::Shutdown;
use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::fmt;
use tracing::{error, event, info, Level};
use url::Url;
use uuid::Uuid;

struct JobId(pub Uuid);

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
pub enum AddError {
    #[error(transparent)]
    SQLx(#[from] sqlx::Error),
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
async fn add_job<'e, E>(executor: E, job: Job) -> Result<(), AddError>
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

    Ok(())
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

    #[tracing::instrument(name = "Manage jobs", skip(self))]
    async fn manage_jobs(&mut self) -> anyhow::Result<()> {
        let mut remaining = MANAGE_JOBS_LIMIT;

        add_fetch_favicons_jobs(&self.pool, &mut remaining).await?;

        Ok(())
    }

    #[tracing::instrument(name = "Run jobs", skip(self))]
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
                hasher.update(&data.feed_id);
            }
            Job::RefreshFeed(data) => {
                hasher.update(&data.feed_id);
            }
        }

        hasher.finalize().into()
    }
}

//
// Job: refreshing a feed
//

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RefreshFeedJobData {
    feed_id: FeedId,
    feed_url: Url,
}

/// Add a job to refresh a feed.
///
/// # Errors
///
/// This function will return an error if:
/// * `feed_url` is invalid
/// * There was an error adding the job to the queue
#[tracing::instrument(
    name = "Add refresh feed job",
    skip(executor),
    fields(
        feed_id = %feed_id,
        feed_url = %feed_url,
    )
)]
pub async fn add_refresh_feed_job<'e, E>(
    executor: E,
    feed_id: FeedId,
    feed_url: Url,
) -> anyhow::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    add_job(
        executor,
        Job::RefreshFeed(RefreshFeedJobData { feed_id, feed_url }),
    )
    .await?;

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

    // Try to parse as a feed
    let (feed, feed_entries) = {
        let mut raw_feed =
            feed_rs::parser::parse(&response_bytes[..]).map_err(Into::<anyhow::Error>::into)?;
        let raw_entries = std::mem::take(&mut raw_feed.entries);

        (
            Feed::from_raw_feed(&data.feed_url, raw_feed),
            raw_entries
                .into_iter()
                .map(FeedEntry::from_raw_feed_entry)
                .collect::<Vec<FeedEntry>>(),
        )
    };

    event!(
        Level::INFO,
        title = %feed.title,
        entries = %feed_entries.len(),
        "found a raw feed",
    );

    // Insert all items

    let mut tx = pool.begin().await?;

    for entry in feed_entries {
        insert_feed_entry(&mut tx, &data.feed_id, &entry).await?;
    }

    tx.commit().await?;

    Ok(())
}

//
// Job: fetching a favicon
//

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchFaviconJobData {
    feed_id: FeedId,
    site_link: Url,
}

/// Add a job to fetch the favicon of a feed.
///
/// # Errors
///
/// This function will return an error if:
/// * `site_link` is invalid
/// * There was an error adding the job to the queue
#[tracing::instrument(
    name = "Add fetch favicon job",
    skip(executor),
    fields(
        feed_id = %feed_id,
    ),
)]
pub async fn add_fetch_favicon_job<'e, E>(
    executor: E,
    feed_id: FeedId,
    site_link: &str,
) -> anyhow::Result<()>
where
    E: sqlx::PgExecutor<'e>,
{
    let site_link = Url::parse(site_link)?;

    add_job(
        executor,
        Job::FetchFavicon(FetchFaviconJobData { feed_id, site_link }),
    )
    .await?;

    Ok(())
}

/// Add as many as `remaining` jobs to fetch the favicon of a feed.
///
/// # Errors
///
/// This function will return an error if there was an error adding a job to the queue
#[tracing::instrument(name = "Add fetch favicons jobs", skip(pool, remaining))]
async fn add_fetch_favicons_jobs(pool: &PgPool, remaining: &mut usize) -> anyhow::Result<()> {
    let records = sqlx::query!(
        r#"
            SELECT id, site_link
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
        let feed_id = FeedId::from(record.id);

        add_fetch_favicon_job(&mut tx, feed_id, &record.site_link).await?;
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
    let FetchFaviconJobData { feed_id, site_link } = data;

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::get_feed_favicon;
    use crate::tests::{create_feed, create_user, get_pool};
    use wiremock::matchers::path;
    use wiremock::{Mock, MockServer, ResponseTemplate};

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
            create_feed(&pool, &user_id, &mock_url.join("/feed").unwrap(), &mock_url).await;

        // Run the job

        let data = FetchFaviconJobData {
            feed_id: feed_id.clone(),
            site_link: mock_url,
        };

        run_fetch_favicon_job(&http_client, &pool, data)
            .await
            .unwrap();

        // Check the result

        let favicon = get_feed_favicon(&pool, &user_id, &feed_id).await.unwrap();
        assert!(favicon.is_some());
        assert_eq!(fake_icon_data, &favicon.unwrap()[..]);
    }
}
