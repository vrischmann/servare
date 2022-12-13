use secrecy::Secret;
use servare::startup::get_connection_pool;
use servare::startup::DashboardApplication;
use sqlx::PgPool;
use std::str::FromStr;

pub struct TestApp {
    pub address: String,
    pub port: u16,
    pub pool: PgPool,

    pub http_client: reqwest::Client,
}

impl TestApp {
    pub async fn get_foobar_html(&self) -> String {
        let response = self
            .http_client
            .get(&format!("{}/foobar", &self.address))
            .send()
            .await
            .expect("Failed to execute request.");

        response.text().await.unwrap()
    }
}

pub async fn spawn_app() -> TestApp {
    let connection_string =
        Secret::from_str("postgresql://vincent:vincent@localhost/servare_tests").unwrap();

    let pool = get_connection_pool(connection_string).await;

    spawn_app_with_pool(pool).await
}

pub async fn spawn_app_with_pool(pool: PgPool) -> TestApp {
    // Build the application
    let app_pool = pool.clone();

    let app = DashboardApplication::build_with_pool(app_pool).expect("Failed to build application");
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
    }
}
