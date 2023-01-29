use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::filter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Layer, SubscriberExt};
use tracing_subscriber::Registry;

pub struct SubscriberBuilder {
    name: String,
    logging_targets: filter::Targets,
    jaeger_endpoint: Option<String>,
    jaeger_targets: filter::Targets,
}

impl SubscriberBuilder {
    pub fn new<T>(name: T) -> Self
    where
        T: AsRef<str>,
    {
        Self {
            name: name.as_ref().to_string(),
            jaeger_endpoint: None,
            logging_targets: filter::Targets::default(),
            jaeger_targets: filter::Targets::default(),
        }
    }

    pub fn with_logging_targets(mut self, targets: filter::Targets) -> Self {
        self.logging_targets = targets;
        self
    }

    pub fn with_jaeger_endpoint(mut self, endpoint: Option<String>) -> Self {
        self.jaeger_endpoint = endpoint;
        self
    }

    pub fn with_jaeger_targets(mut self, targets: Option<filter::Targets>) -> Self {
        if let Some(targets) = targets {
            self.jaeger_targets = targets;
        }
        self
    }

    /// Creates a [`tracing::Subscriber`] configured to format logs with [`Bunyan`]
    ///
    /// [`Bunyan`]: https://docs.rs/tracing-bunyan-formatter/latest/tracing_bunyan_formatter/
    pub fn build<Sink>(self, sink: Sink) -> Box<dyn Subscriber + Sync + Send>
    where
        Sink: for<'a> MakeWriter<'a> + Sync + Send + 'static,
    {
        let logging_layer = {
            let formatting_layer = BunyanFormattingLayer::new(self.name.clone(), sink)
                .skip_fields(
                    vec!["file".to_string(), "line".to_string(), "target".to_string()].into_iter(),
                )
                .expect("unable to build the bunyan formatting layer");

            formatting_layer.with_filter(self.logging_targets)
        };

        match self.jaeger_endpoint {
            Some(endpoint) => {
                let otel_tracer = opentelemetry_jaeger::new_agent_pipeline()
                    .with_endpoint(endpoint)
                    .with_service_name(self.name)
                    .install_simple()
                    .expect("unable to get otel jaeger agent pipeline");

                let otel_layer = tracing_opentelemetry::layer()
                    .with_tracer(otel_tracer)
                    .with_filter(self.jaeger_targets);

                Box::new(
                    Registry::default()
                        .with(JsonStorageLayer)
                        .with(logging_layer)
                        .with(otel_layer),
                )
            }
            None => Box::new(
                Registry::default()
                    .with(JsonStorageLayer)
                    .with(logging_layer),
            ),
        }
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
