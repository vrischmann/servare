use servare::configuration::get_configuration;
use servare::startup::get_connection_pool;
use servare::startup::Application;
use servare::telemetry;
use tracing::info;

fn main() -> anyhow::Result<()> {
    let subscriber = telemetry::get_subscriber("servare".into(), "info".into(), std::io::stdout);
    telemetry::init_global_default(subscriber);

    let config = get_configuration()?;

    // Build the Tokio runtime
    let runtime = tokio::runtime::Builder::new_current_thread()
        .worker_threads(config.application.worker_threads)
        .thread_name("servare")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();
    let _runtime_guard = runtime.enter();

    let pool = runtime.block_on(get_connection_pool(&config.database));
    let app = Application::build(&config.application, pool)?;

    let future = app.run_until_stopped();

    info!(
        url = format!(
            "{}:{}",
            config.application.base_url, config.application.port
        ),
        "running dashboard app"
    );

    // Run the app future until done
    runtime.block_on(future)?;

    Ok(())
}
