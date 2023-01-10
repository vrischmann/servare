use bytes::Bytes;
use std::fmt;
use url::Url;

pub mod authentication;
pub mod configuration;
pub mod domain;
pub mod feed;
pub mod html;
pub mod job;
mod routes;
mod sessions;
pub mod shutdown;
pub mod startup;
pub mod telemetry;
pub mod tem;

pub fn error_chain_fmt(err: &impl std::error::Error, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    writeln!(f, "{}\n", err)?;
    let mut current = err.source();
    while let Some(cause) = current {
        writeln!(f, "Caused by:\n\t{}", cause)?;
        current = cause.source();
    }
    Ok(())
}

/// Fetches the content of a URL directly as a bytes buffer.
///
/// # Errors
///
/// This function will return an error if the fetch fails.
pub async fn fetch_bytes(client: &reqwest::Client, url: &Url) -> Result<Bytes, reqwest::Error> {
    let response = client.get(url.to_string()).send().await?;
    let response_bytes = response.bytes().await?;

    Ok(response_bytes)
}
