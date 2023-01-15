use crate::configuration::JobConfig;
use crate::feed::{find_favicon, FeedId};
use crate::fetch_bytes;
use crate::shutdown::Shutdown;
use blake2::{Blake2b512, Digest};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sqlx::PgPool;
use std::fmt;
use tracing::warn;
use tracing::{error, info};
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
async fn add_job<'e, E>(executor: E, job: Job) -> Result<(), AddError>
where
    E: sqlx::PgExecutor<'e>,
{
    let job_id = JobId::default();

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

    async fn manage_jobs(&mut self) -> anyhow::Result<()> {
        let mut remaining = MANAGE_JOBS_LIMIT;

        self.add_fetch_favicons_jobs(&mut remaining).await?;

        Ok(())
    }

    async fn run_jobs(&mut self) -> anyhow::Result<()> {
        let mut tx = self.pool.begin().await?;

        let records = sqlx::query!(
            r#"
            SELECT id, data, status as "status: String", attempts
            FROM jobs
            WHERE status != 'done'
            LIMIT $1
            "#,
            RUN_JOBS_LIMIT as i64,
        )
        .fetch_all(&mut tx)
        .await?;

        for record in records {
            let job: Job = serde_json::from_value(record.data)?;

            match job {
                Job::FetchFavicon(feed_id) => {
                    warn!("got data: {:?}", feed_id);
                }
            }
        }

        Ok(())
    }

    async fn add_fetch_favicons_jobs(&mut self, remaining: &mut usize) -> anyhow::Result<()> {
        let records = sqlx::query!(
            r#"
            SELECT id, site_link
            FROM feeds f
            WHERE has_favicon IS NULL
            LIMIT $1
            "#,
            *remaining as i64,
        )
        .fetch_all(&self.pool)
        .await?;

        let mut tx = self.pool.begin().await?;

        for record in records {
            let feed_id = FeedId::from(record.id);

            add_fetch_favicon_job(&mut tx, feed_id, &record.site_link).await?;
        }

        tx.commit().await?;

        Ok(())
    }

    // async fn fetch_favicons(&mut self, remaining: &mut usize) -> anyhow::Result<()> {
    //     // TODO(vincent): try to use the generic job table

    //     // TODO(vincent): this is not safe if run on more than one instance, more than one
    //     // job runner could be running this job

    //     let feeds = sqlx::query!(
    //         r#"
    //     SELECT id, site_link, site_favicon
    //     FROM feeds f
    //     WHERE site_favicon IS NULL
    //     LIMIT $1
    //     "#,
    //         *remaining as i64,
    //     )
    //     .fetch_all(&self.pool)
    //     .await?;

    //     for feed in feeds {
    //         let site_link = Url::parse(&feed.site_link)?;

    //         if let Some(url) = find_favicon(&self.http_client, &site_link).await {
    //             let favicon = fetch_bytes(&self.http_client, &url).await?;

    //             sqlx::query!(
    //                 r#"
    //             UPDATE feeds SET site_favicon = $1 WHERE id = $2
    //             "#,
    //                 &favicon[..],
    //                 &feed.id,
    //             )
    //             .execute(&self.pool)
    //             .await?;
    //         }

    //         *remaining -= 1;
    //     }

    //     Ok(())
    // }
}

//

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
enum Job {
    FetchFavicon(FetchFaviconJobData),
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
        }

        hasher.finalize().into()
    }
}

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
