use crate::configuration::{ApplicationConfig, DatabaseConfig};
use crate::routes;
use axum::response::IntoResponse;
use axum::routing;
use axum::routing::IntoMakeService;
use hyper::server::conn::AddrIncoming;
use secrecy::ExposeSecret;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("hyper server failed")]
    Hyper(#[from] hyper::Error),
    #[error("unable to bind tcp listener")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

type Server = axum::Server<AddrIncoming, IntoMakeService<axum::Router>>;

pub struct Application {
    pub port: u16,
    pub pool: sqlx::PgPool,
    server: Server,
}

impl Application {
    pub fn build(config: &ApplicationConfig, pool: PgPool) -> Result<Application, Error> {
        let listener = std::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
            .map_err(Into::<Error>::into)?;
        let port = listener.local_addr().unwrap().port();

        let server: Server = create_server(listener)?;

        Ok(Application { port, pool, server })
    }

    pub async fn run_until_stopped(self) -> Result<(), Error> {
        self.server.await?;

        Ok(())
    }
}

async fn fallback_handler() -> (http::StatusCode, String) {
    (http::StatusCode::NOT_FOUND, "Page Not Found".to_owned())
}

async fn error_handler(err: std::io::Error) -> impl IntoResponse {
    error!(err = ?err, "got error");

    (
        http::StatusCode::NOT_FOUND,
        "Internal Server Error".to_owned(),
    )
}

fn create_server(listener: std::net::TcpListener) -> Result<Server, anyhow::Error> {
    // Serves the assets from disk
    let assets_service = {
        let serve_dir = tower_http::services::ServeDir::new("assets");
        axum::routing::get_service(serve_dir).handle_error(error_handler)
    };

    let login_router = axum::Router::new()
        .route("/", routing::get(routes::login::form))
        .route("/", routing::post(routes::login::submit));

    let web_app = axum::Router::new()
        .route("/", routing::get(routes::home))
        .nest("/login", login_router)
        .nest_service("/assets", assets_service)
        .fallback(fallback_handler)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(());

    let web_server_builder = axum::Server::from_tcp(listener)?;

    let web_server = web_server_builder.serve(web_app.into_make_service());

    Ok(web_server)
}

pub async fn get_connection_pool(config: &DatabaseConfig) -> PgPool {
    PgPoolOptions::new()
        .max_connections(1024)
        .acquire_timeout(Duration::from_secs(1))
        .connect(config.connection_string().expose_secret())
        .await
        .expect("Failed to connect to PostgreSQL")
}
