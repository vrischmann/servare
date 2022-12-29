use servare::configuration::get_configuration;
use servare::startup::get_connection_pool;
use servare::startup::Application;
use servare::telemetry;
use tracing::{error, info};

fn main() {
    let config = match get_configuration() {
        Ok(config) => config,
        Err(err) => {
            error!(err = %err, "unable to get the configuration");
            std::process::exit(1)
        }
    };

    let subscriber = telemetry::get_subscriber(
        telemetry::Configuration {
            jaeger_config: config.jaeger,
            name: "servare".to_string(),
        },
        std::io::stdout,
    );
    telemetry::init_global_default(subscriber);

    // Build the Tokio runtime
    let runtime = tokio::runtime::Builder::new_current_thread()
        .worker_threads(config.application.worker_threads)
        .thread_name("servare")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();
    let _runtime_guard = runtime.enter();

    // Connect to PostgreSQL and get a connection pool
    let pool = match runtime.block_on(get_connection_pool(&config.database)) {
        Ok(pool) => pool,
        Err(err) => {
            error!(err = %err, "unable to get a connection pool");
            std::process::exit(1)
        }
    };

    // Build the web application
    let app = match Application::build(&config.application, &config.session, pool) {
        Ok(app) => app,
        Err(err) => {
            error!(err = %err, "failed to build application");
            std::process::exit(1)
        }
    };

    // Finally start the application asynchronously
    let future = app.run_until_stopped();

    info!(
        url = format!(
            "{}:{}",
            config.application.base_url, config.application.port
        ),
        "running dashboard app"
    );

    // Run the app future until done
    match runtime.block_on(future) {
        Ok(()) => {}
        Err(err) => {
            error!(err = %err, "application failed");
        }
    }
}
