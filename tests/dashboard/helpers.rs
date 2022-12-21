use fake::faker::internet::en::SafeEmail;
use fake::Fake;
use servare::configuration::get_configuration;
use servare::startup::get_connection_pool;
use servare::startup::Application;

use sqlx::PgPool;

pub struct TestUser {
    pub email: String,
}

impl Default for TestUser {
    fn default() -> Self {
        Self {
            email: SafeEmail().fake(),
        }
    }
}

pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub pool: PgPool,
    pub http_client: reqwest::Client,

    pub test_user: TestUser,
}

impl TestApp {
    pub async fn get_home_html(&self) -> String {
        let response = self
            .http_client
            .get(&self.address)
            .send()
            .await
            .expect("Failed to execute request.");

        response.text().await.unwrap()
    }

    pub async fn get_login(&self) -> reqwest::Response {
        self
            .http_client
            .get(&format!("{}/login", self.address))
            .send()
            .await
           .expect("Failed to execute request.")
    }

    pub async fn post_login(&self, body: &LoginBody) -> reqwest::Response {
        self.post(body).await
    }

    async fn post<T>(&self, body: &T) -> reqwest::Response
    where
        T: serde::Serialize,
    {
        self.http_client
            .post(&format!("{}/login", self.address))
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
}

/// Spawns a new [`TestApp`] instance.
///
/// The instance is ready to be used for testing.
pub async fn spawn_app() -> TestApp {
    let config = get_configuration().expect("Failed to get configuration");

    let pool = get_connection_pool(&config.database).await;

    spawn_app_with_pool(pool).await
}

/// Spawns a new [`TestApp`] instance with the provided [`PgPool`]
///
/// The instance is ready to be used for testing.
pub async fn spawn_app_with_pool(pool: PgPool) -> TestApp {
    let mut configuration = get_configuration().expect("Failed to get configuration");
    configuration.application.port = 0;

    // Build the application
    let app_pool = pool.clone();

    let app = Application::build(&configuration.application, app_pool)
        .expect("Failed to build application");
    let app_port = app.port;

    let _ = tokio::spawn(app.run_until_stopped());

    //

    let http_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .expect("Failed to build HTTP client");

    TestApp {
        address: format!("http://127.0.0.1:{}", app_port),
        port: app_port,
        pool,
        http_client,
        test_user: TestUser::default(),
    }
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
