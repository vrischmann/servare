use crate::configuration::{ApplicationConfig, DatabaseConfig};
use axum::routing::IntoMakeService;
use hyper::server::conn::AddrIncoming;
use secrecy::ExposeSecret;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Duration;

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
        let listener = std::net::TcpListener::bind(&format!("{}:{}", config.host, config.port))
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

async fn foobar() -> impl axum::response::IntoResponse {
    "foobar".to_string()
}

fn create_server(listener: std::net::TcpListener) -> Result<Server, anyhow::Error> {
    let web_app = axum::Router::new()
        .route("/foobar", axum::routing::get(foobar))
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
