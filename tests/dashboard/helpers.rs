use anyhow::Context;
use argon2::password_hash::{PasswordHasher, SaltString};
use argon2::Argon2;
use fake::faker::internet::en::{Password as FakerPassword, SafeEmail as FakerSafeEmail};
use fake::Fake;
use once_cell::sync::Lazy;
use servare::configuration::get_configuration;
use servare::domain::UserId;
use servare::job::JobRunner;
use servare::run_group::RunGroup;
use servare::startup::Application;
use servare::startup::{get_connection_pool, get_tem_client};
use servare::{telemetry, tem};
use sqlx::PgPool;
use tracing::Level;
use tracing_subscriber::filter;
use uuid::Uuid;
use wiremock::MockServer;

static TRACING: Lazy<()> = Lazy::new(|| {
    std::env::set_var("RUST_LOG", "sqlx=error,info");
    let has_test_log = std::env::var("TEST_LOG").is_ok();

    let targets = {
        let tmp = filter::Targets::new().with_targets(vec![
            ("html5ever", Level::INFO),
            ("hyper", Level::INFO),
            ("cookie_store", Level::INFO),
            ("reqwest", Level::INFO),
            ("wiremock", Level::INFO),
        ]);

        if has_test_log {
            tmp.with_default(Level::DEBUG)
        } else {
            tmp.with_default(Level::INFO)
        }
    };

    let subscriber_builder =
        telemetry::SubscriberBuilder::new("test").with_logging_targets(targets);

    let subscriber = if has_test_log {
        subscriber_builder.build(std::io::stdout)
    } else {
        subscriber_builder.build(std::io::sink)
    };
    telemetry::init_global_default(subscriber);
});

pub struct TestUser {
    pub id: UserId,
    pub email: String,
    pub password: String,
}

impl Default for TestUser {
    fn default() -> Self {
        Self {
            id: UserId(Uuid::new_v4()),
            email: FakerSafeEmail().fake(),
            password: FakerPassword(10..20).fake(),
        }
    }
}

impl TestUser {
    async fn store(&self, pool: &PgPool) -> anyhow::Result<()> {
        let salt = SaltString::generate(&mut rand::thread_rng());

        let hasher = Argon2::new(
            argon2::Algorithm::Argon2id,
            argon2::Version::V0x13,
            argon2::Params::new(15000, 2, 1, None).unwrap(),
        );

        let password_hash = hasher
            .hash_password(self.password.as_bytes(), &salt)
            .context("unable to compute password hash")?
            .to_string();

        sqlx::query!(
            r#"
            INSERT INTO users(id, email, password_hash)
            VALUES ($1, $2, $3)
            "#,
            &self.id.0,
            self.email,
            password_hash,
        )
        .execute(pool)
        .await
        .context("unable to insert test user")?;

        Ok(())
    }
}

/// TestApp is a test harness for integration testing of Servare.
pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub pool: PgPool,
    pub http_client: reqwest::Client,
    pub email_server: MockServer,
    pub email_client: tem::Client,

    pub test_user: TestUser,
}

impl TestApp {
    pub async fn get_html(&self, path: &str) -> String {
        let response = self
            .http_client
            .get(&format!("{}{}", self.address, path))
            .send()
            .await
            .expect("Failed to execute request.");

        response.text().await.unwrap()
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.http_client
            .get(&format!("{}{}", self.address, path))
            .send()
            .await
            .expect("Failed to execute request.")
    }

    pub async fn post<T>(&self, path: &str, body: &T) -> reqwest::Response
    where
        T: serde::Serialize,
    {
        self.http_client
            .post(&format!("{}{}", self.address, path))
            .form(body)
            .send()
            .await
            .expect("Failed to execute request.")
    }
}

/// Used when submitting a POST /login with the `TestApp` helper.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct LoginBody {
    pub email: String,
    pub password: String,
}

/// Spawns a new [`TestApp`] instance.
///
/// The instance is ready to be used for testing.
pub async fn spawn_app() -> TestApp {
    let config = get_configuration().expect("Failed to get configuration");

    let pool = get_connection_pool(&config.database).await.unwrap();

    spawn_app_with_pool(pool).await
}

/// Spawns a new [`TestApp`] instance with the provided [`PgPool`]
///
/// The instance is ready to be used for testing.
pub async fn spawn_app_with_pool(pool: PgPool) -> TestApp {
    // Enable tracing
    Lazy::force(&TRACING);

    // We mock the minimal needed from the TEM API using wiremock
    let email_server = MockServer::start().await;

    // Get the configuration from the local file and modify it to be suitable for testing.
    // This means:
    // * set the port to 0 so that the OS is responsible for choosing a free port
    // * set the TEM base url to the URL of the mock email server
    let mut configuration = get_configuration().expect("Failed to get configuration");
    configuration.application.port = 0;
    configuration.tem.base_url = email_server.uri();

    //
    // Build the test email client and test HTTP client
    //

    let email_client = get_tem_client(&configuration.tem).expect("Failed to get TEM client");

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .expect("Failed to build HTTP client");

    //
    // Build the application and job runner
    //

    let app_pool = pool.clone();
    let app = Application::build(&configuration.application, &configuration.session, app_pool)
        .expect("Failed to build application");
    let app_port = app.port;

    let job_pool = pool.clone();
    let job_runner =
        JobRunner::new(configuration.job, job_pool).expect("Failed to build job runner");

    //
    // Run everything in a run group
    //

    let run_group = RunGroup::new()
        .run(|shutdown| app.run(shutdown))
        .run(|shutdown| job_runner.run(shutdown));

    #[allow(clippy::let_underscore_future)]
    let _ = tokio::spawn(run_group.start());

    // Build the test harness

    let test_app = TestApp {
        address: format!("http://127.0.0.1:{}", app_port),
        port: app_port,
        pool,
        http_client,
        email_server,
        email_client,
        test_user: TestUser::default(),
    };

    // Store the test user
    test_app
        .test_user
        .store(&test_app.pool)
        .await
        .expect("Failed to store the test user");

    test_app
}

pub fn assert_is_redirect_to(response: &reqwest::Response, location: &str) {
    assert_eq!(
        response.status().as_u16(),
        303,
        "got {}, expected {}",
        response.status().as_u16(),
        303
    );
    assert_eq!(response.headers().get("Location").unwrap(), location);
}

#[derive(rust_embed::RustEmbed)]
#[folder = "testdata/"]
pub struct TestData;
