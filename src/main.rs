use servare::configuration::get_configuration;
use servare::startup::get_connection_pool;
use servare::startup::Application;
use servare::telemetry;
use tracing::{error, info};

fn main() -> anyhow::Result<()> {
    let subscriber = telemetry::get_subscriber("servare".into(), "info".into(), std::io::stdout);
    telemetry::init_global_default(subscriber);

    let config = match get_configuration() {
        Ok(config) => config,
        Err(err) => {
            error!(err = %err, "unable to get the configuration");
            return Err(err.into());
        }
    };

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
            return Err(err.into());
        }
    };

    // Build the web application
    let app = match Application::build(&config.application, pool) {
        Ok(app) => app,
        Err(err) => {
            error!(err = %err, "failed to build application");
            return Err(err.into());
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

    Ok(())
}
