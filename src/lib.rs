use std::fmt;

pub mod authentication;
pub mod configuration;
pub mod domain;
pub mod feed;
mod routes;
mod sessions;
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
