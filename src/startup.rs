use crate::configuration::{ApplicationConfig, DatabaseConfig, TEMConfig};
use crate::{routes, tem};
use axum::extract::FromRef;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::routing::IntoMakeService;
use axum_extra::extract::cookie::Key as CookieKey;
use hyper::server::conn::AddrIncoming;
use secrecy::ExposeSecret;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Duration;
use tower::ServiceBuilder;
use tracing::error;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid cookie key")]
    InvalidCookieKey(#[source] anyhow::Error),
    #[error("hyper server failed")]
    Hyper(#[from] hyper::Error),
    #[error("unable to bind tcp listener")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

type Server = axum::Server<AddrIncoming, IntoMakeService<axum::Router>>;

#[derive(Clone)]
pub struct ApplicationState {
    pub pool: Arc<PgPool>,
    pub cookie_key: CookieKey,
}

impl FromRef<ApplicationState> for CookieKey {
    fn from_ref(state: &ApplicationState) -> Self {
        state.cookie_key.clone()
    }
}

pub struct Application {
    pub port: u16,
    server: Server,
}

impl Application {
    /// Builds a new application using `config` and `pool`.
    ///
    /// The application will have started but not completed, you need to await
    /// on `run_until_stopped` to run the server to completion.
    pub fn build(config: &ApplicationConfig, pool: PgPool) -> Result<Application, Error> {
        // Build the TCP listener
        let listener = std::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
            .map_err(Into::<Error>::into)?;
        let port = listener.local_addr().unwrap().port();

        // Get the cookie key from the configuration
        let cookie_key = {
            let data = hex::decode(config.cookie_key.expose_secret().as_bytes())
                .map_err(Into::<anyhow::Error>::into)
                .map_err(Error::InvalidCookieKey)?;

            CookieKey::try_from(data.as_slice())
                .map_err(Into::<anyhow::Error>::into)
                .map_err(Error::InvalidCookieKey)?
        };

        // Build the application state
        let state = ApplicationState {
            pool: Arc::new(pool),
            cookie_key,
        };

        // Finally create the HTTP server
        let server: Server = create_server(listener, state)?;

        Ok(Application { port, server })
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

fn create_server(
    listener: std::net::TcpListener,
    state: ApplicationState,
) -> Result<Server, anyhow::Error> {
    // Serves the assets from disk
    let assets_service = {
        let serve_dir = tower_http::services::ServeDir::new("assets");
        axum::routing::get_service(serve_dir).handle_error(error_handler)
    };

    let web_app = axum::Router::new()
        .route("/", get(routes::home))
        .route(
            "/login",
            get(routes::login::form).post(routes::login::submit),
        )
        .nest_service("/assets", assets_service)
        .fallback(fallback_handler)
        .layer(
            ServiceBuilder::new()
                .layer(tower_http::trace::TraceLayer::new_for_http())
                .layer(session_layer),
        )
        .with_state(state);

    let web_server_builder = axum::Server::from_tcp(listener)?;

    let web_server = web_server_builder.serve(web_app.into_make_service());

    Ok(web_server)
}

pub async fn get_connection_pool(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(1024)
        .acquire_timeout(Duration::from_secs(1))
        .connect(config.connection_string().expose_secret())
        .await
}

pub fn get_tem_client(configuration: &TEMConfig) -> anyhow::Result<tem::Client> {
    let sender_email = configuration.sender()?;

    Ok(tem::Client::new(
        configuration.base_url.clone(),
        configuration.project_id.clone(),
        configuration.auth_key.clone(),
        sender_email,
        configuration.timeout(),
    ))
}
