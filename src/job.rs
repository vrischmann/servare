use crate::configuration::JobConfig;
use crate::feed::find_favicon;
use crate::fetch_bytes;
use crate::shutdown::Shutdown;
use sqlx::PgPool;
use tracing::{error, info};
use url::Url;

// #[derive(Debug, Clone, Deserialize, Serialize)]
// pub enum Job {
//     FetchFavicon(FeedId),
// }

// #[derive(Debug, thiserror::Error)]
// pub enum PushError {
//     #[error(transparent)]
//     SQLx(#[from] sqlx::Error),
// }

// pub struct JobId(pub Uuid);

// impl Default for JobId {
//     fn default() -> Self {
//         Self(Uuid::new_v4())
//     }
// }

// async fn add_job<'e, E>(executor: E, job: Job) -> Result<JobId, PushError>
// where
//     E: sqlx::PgExecutor<'e>,
// {
//     let job_id = JobId::default();

//     sqlx::query!(
//         "INSERT INTO jobs(id, data) VALUES($1, $2)",
//         &job_id.0,
//         json!(job)
//     )
//     .execute(executor)
//     .await?;

//     Ok(job_id)
// }

pub struct JobRunner {
    http_client: reqwest::Client,
    config: JobConfig,
    pool: PgPool,
}

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

        // Hardcode some limits on the number of jobs to run in one tick.
        const LIMIT: usize = 50;

        'outer_loop: loop {
            tokio::select! {
                _ = shutdown.recv() => {
                    info!("job runner shutting down");
                    break 'outer_loop;
                },
                _ = interval.tick() => {
                    if let Err(err) = self.run_jobs(LIMIT).await {
                        error!(%err, "failed while managing jobs");
                    }
                },
            }
        }

        Ok(())
    }

    async fn run_jobs(&mut self, mut remaining: usize) -> anyhow::Result<()> {
        while remaining > 0 {
            self.fetch_favicons(&mut remaining).await?;

            remaining -= 1;
        }

        Ok(())
    }

    async fn fetch_favicons(&mut self, remaining: &mut usize) -> anyhow::Result<()> {
        // TODO(vincent): this is not safe if run on more than one instance, more than one
        // job runner could be running this job

        let feeds = sqlx::query!(
            r#"
            SELECT id, site_link, site_favicon
            FROM feeds f
            WHERE site_favicon IS NULL
            LIMIT $1
            "#,
            *remaining as i64,
        )
        .fetch_all(&self.pool)
        .await?;

        for feed in feeds {
            let site_link = Url::parse(&feed.site_link)?;

            if let Some(url) = find_favicon(&self.http_client, &site_link).await {
                let favicon = fetch_bytes(&self.http_client, &url).await?;

                sqlx::query!(
                    r#"
                    UPDATE feeds SET site_favicon = $1 WHERE id = $2
                    "#,
                    &favicon[..],
                    &feed.id,
                )
                .execute(&self.pool)
                .await?;
            }

            *remaining -= 1;
        }

        Ok(())
    }
}
