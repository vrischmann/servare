use crate::domain::UserEmail;
use crate::tem;
use secrecy::{ExposeSecret, Secret};
use std::time::Duration;

#[derive(Clone, Debug, serde::Deserialize)]
pub struct ApplicationConfig {
    pub worker_threads: usize,
    pub host: String,
    pub port: usize,
    pub base_url: String,
    pub cookie_key: Secret<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct DatabaseConfig {
    pub username: String,
    pub password: Secret<String>,
    pub port: u16,
    pub host: String,
    pub name: String,
}

impl DatabaseConfig {
    pub fn connection_string(&self) -> Secret<String> {
        Secret::new(format!(
            "postgres://{}:{}@{}:{}/{}",
            self.username,
            self.password.expose_secret(),
            self.host,
            self.port,
            self.name
        ))
    }
}

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

    pub fn timeout(&self) -> Duration {
        Duration::from_millis(self.timeout_milliseconds)
    }
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    pub application: ApplicationConfig,
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
