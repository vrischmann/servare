use crate::domain::UserEmail;
use crate::tem;
use secrecy::Secret;
use std::time::Duration as StdDuration;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApplicationConfig {
    pub worker_threads: usize,
    pub host: String,
    pub port: usize,
    pub base_url: String,
    pub cookie_signing_key: Secret<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct SessionConfig {
    pub ttl_seconds: u64,
    pub cleanup_enabled: bool,
    pub cleanup_interval_seconds: i64,
}

impl SessionConfig {
    pub fn ttl(&self) -> StdDuration {
        StdDuration::from_secs(self.ttl_seconds)
    }

    pub fn cleanup_interval(&self) -> time::Duration {
        time::Duration::seconds(self.cleanup_interval_seconds)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct DatabaseConfig {
    pub username: String,
    pub password: Secret<String>,
    pub port: u16,
    pub host: String,
    pub name: String,
}

impl DatabaseConfig {}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct TEMConfig {
    pub base_url: String,
    pub project_id: tem::ProjectId,
    pub auth_key: Secret<String>,
    pub sender_email: String,
    pub timeout_milliseconds: u64,
}

impl TEMConfig {
    pub fn sender(&self) -> anyhow::Result<UserEmail> {
        UserEmail::parse(self.sender_email.clone())
    }

    pub fn timeout(&self) -> StdDuration {
        StdDuration::from_millis(self.timeout_milliseconds)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub application: ApplicationConfig,
    pub session: SessionConfig,
    pub database: DatabaseConfig,
    pub tem: TEMConfig,
}

pub fn get_configuration() -> Result<Config, config::ConfigError> {
    let config_reader = config::Config::builder()
        .add_source(
            config::File::new("configuration.toml", config::FileFormat::Toml).required(false),
        )
        .add_source(
            config::File::new("/etc/zero2prod.toml", config::FileFormat::Toml).required(false),
        )
        .add_source(
            config::Environment::default()
                .try_parsing(true)
                .separator("_"),
        )
        .build()?;

    config_reader.try_deserialize::<Config>()
}
