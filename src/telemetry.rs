use crate::configuration::JaegerConfig;
use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::filter::{EnvFilter, LevelFilter};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::Registry;

pub struct Configuration {
    pub jaeger_config: Option<JaegerConfig>,
    pub name: String,
}

/// Creates a [`tracing::Subscriber`] configured to format logs with [`Bunyan`]
///
/// [`Bunyan`]: https://docs.rs/tracing-bunyan-formatter/latest/tracing_bunyan_formatter/
pub fn get_subscriber<Sink>(config: Configuration, sink: Sink) -> Box<dyn Subscriber + Sync + Send>
where
    Sink: for<'a> MakeWriter<'a> + Sync + Send + 'static,
{
    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    let formatting_layer = BunyanFormattingLayer::new(config.name.clone(), sink)
        .skip_fields(vec!["file".to_string(), "line".to_string(), "target".to_string()].into_iter())
        .expect("unable to build the bunyan formatting layer");

    match config.jaeger_config {
        Some(jaeger_config) => {
            let otel_tracer = opentelemetry_jaeger::new_agent_pipeline()
                .with_endpoint(jaeger_config.endpoint())
                .with_service_name(config.name)
                .install_simple()
                .expect("unable to get otel jaeger agent pipeline");
            let otel_layer = tracing_opentelemetry::layer().with_tracer(otel_tracer);

            Box::new(
                Registry::default()
                    .with(env_filter)
                    .with(JsonStorageLayer)
                    .with(formatting_layer)
                    .with(otel_layer),
            )
        }
        None => Box::new(
            Registry::default()
                .with(env_filter)
                .with(JsonStorageLayer)
                .with(formatting_layer),
        ),
    }
}

/// Sets `subscriber` as the global default [`tracing::Subscriber`].
pub fn init_global_default(subscriber: impl Subscriber + Sync + Send) {
    LogTracer::init().expect("Failed to set logger");
    set_global_default(subscriber).expect("Failed to set subscriber");
}

/// Spawns a blocking task in the scope of the current tracing span.
pub fn spawn_blocking_with_tracing<F, R>(f: F) -> tokio::task::JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let current_span = tracing::Span::current();
    tokio::task::spawn_blocking(move || current_span.in_scope(f))
}
