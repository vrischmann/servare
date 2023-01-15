use crate::configuration::{ApplicationConfig, DatabaseConfig, SessionConfig, TEMConfig};
use crate::sessions::{CleanupConfig as SessionStoreCleanupConfig, PgSessionStore};
use crate::shutdown::Shutdown;
use crate::{routes::*, tem};
use actix_session::SessionMiddleware;
use actix_web::{cookie, dev::Server};
use actix_web::{web, App, HttpServer};
use actix_web_flash_messages::storage::CookieMessageStore;
use actix_web_flash_messages::FlashMessagesFramework;
use secrecy::{ExposeSecret, Secret};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{ConnectOptions, PgPool};
use std::net::TcpListener;
use std::time::Duration as StdDuration;
use tracing::{error, info};
use tracing_actix_web::TracingLogger;
use tracing_log::log::LevelFilter;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid cookie key")]
    InvalidCookieKey(#[source] anyhow::Error),
    #[error("unable to bind tcp listener")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}

#[derive(Clone)]
pub struct HmacSecret<'a>(pub &'a Secret<String>);

pub struct Application {
    pub port: u16,
    server: Server,
}

impl Application {
    /// Builds a new application using `config` and `pool`.
    ///
    /// The application will have started but not completed, you need to await
    /// on `run_until_stopped` to run the server to completion.
    pub fn build(
        config: &ApplicationConfig,
        session_config: &SessionConfig,
        pool: PgPool,
    ) -> Result<Application, Error> {
        let cookie_signing_key =
            cookie::Key::from(config.cookie_signing_key.expose_secret().as_bytes());

        // Flash messages
        let flash_messages_store = CookieMessageStore::builder(cookie_signing_key.clone()).build();
        let flash_messages_framework =
            FlashMessagesFramework::builder(flash_messages_store).build();

        // Build the session store
        let session_store = PgSessionStore::new(
            pool.clone(),
            SessionStoreCleanupConfig::new(
                session_config.cleanup_enabled,
                session_config.cleanup_interval(),
            ),
        );

        // Build the TCP listener
        let listener = std::net::TcpListener::bind(format!("{}:{}", config.host, config.port))
            .map_err(Into::<Error>::into)?;
        let port = listener.local_addr().unwrap().port();

        // Finally create the HTTP server
        let server: Server = create_server(
            listener,
            pool,
            cookie_signing_key,
            session_store,
            session_config.ttl(),
            flash_messages_framework,
        )?;

        Ok(Application { port, server })
    }

    pub async fn run(self, mut shutdown: Shutdown) -> Result<(), Error> {
        tokio::select! {
            _ = shutdown.recv() => {
                    info!("application shutting down");
                    Ok(())
            }
            _ = self.server => {
                    info!("server shut down");
                    Ok(())
            }
        }
    }
}

fn create_server(
    listener: TcpListener,
    pool: PgPool,
    cookie_signing_key: actix_web::cookie::Key,
    session_store: PgSessionStore,
    session_ttl: StdDuration,
    flash_messages_framework: FlashMessagesFramework,
) -> Result<Server, anyhow::Error> {
    let pool = web::Data::new(pool);

    let http_client = {
        let tmp = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .cookie_store(true)
            .build()?;

        web::Data::new(tmp)
    };

    let session_ttl = time::Duration::try_from(session_ttl)
        .expect("StdDuration should always be convertible to time::Duration");

    let server = HttpServer::new(move || {
        let session_middleware =
            SessionMiddleware::builder(session_store.clone(), cookie_signing_key.clone())
                .session_length(actix_session::SessionLength::BrowserSession {
                    state_ttl: Some(session_ttl),
                })
                .cookie_name("session_id".to_string())
                .build();

        App::new()
            .wrap(flash_messages_framework.clone())
            .wrap(session_middleware)
            .wrap(TracingLogger::default())
            .service(actix_files::Files::new("/assets", "./assets").prefer_utf8(true))
            .route("/", web::get().to(handle_home))
            .route("/status", web::get().to(handle_status))
            .route("/login", web::get().to(handle_login_form))
            .route("/login", web::post().to(handle_login_submit))
            .route("/logout", web::to(handle_logout))
            .route("/settings", web::get().to(handle_settings))
            .route("/feeds", web::get().to(handle_feeds))
            .service(
                web::scope("/feeds")
                    .route("/add", web::post().to(handle_feeds_add))
                    .route("/add", web::get().to(handle_feeds_add_form))
                    .route("/refresh", web::post().to(handle_feeds_refresh))
                    .route("/{feed_id}/favicon", web::get().to(handle_feed_favicon)),
            )
            .app_data(pool.clone())
            .app_data(http_client.clone())
    })
    .listen(listener)?
    .run();

    Ok(server)
}

pub async fn get_connection_pool(config: &DatabaseConfig) -> Result<PgPool, sqlx::Error> {
    let mut connect_options = PgConnectOptions::new()
        .username(&config.username)
        .password(config.password.expose_secret())
        .port(config.port)
        .host(&config.host)
        .database(&config.name);
    connect_options.log_slow_statements(LevelFilter::Warn, StdDuration::from_millis(500));
    connect_options.log_statements(LevelFilter::Trace);

    PgPoolOptions::new()
        .max_connections(1024)
        .acquire_timeout(StdDuration::from_secs(1))
        .connect_with(connect_options)
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
