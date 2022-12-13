use secrecy::Secret;
use servare::startup::get_connection_pool;
use servare::startup::DashboardApplication;
use std::str::FromStr;

fn main() -> anyhow::Result<()> {
    // Build the Tokio runtime

    let runtime = tokio::runtime::Builder::new_current_thread()
        .worker_threads(4)
        .thread_name("servare")
        .thread_stack_size(3 * 1024 * 1024)
        .enable_all()
        .build()
        .unwrap();
    let _runtime_guard = runtime.enter();

    // TODO(vincent): replace me
    let connection_string =
        Secret::from_str("postgresql://vincent:vincent@localhost/servare_tests")?;

    let dashboard_pool = runtime.block_on(get_connection_pool(connection_string));
    let dashboard_app = DashboardApplication::build_with_pool(dashboard_pool)?;

    let future = dashboard_app.run_until_stopped();

    println!("running dashboard app");

    // Run the app future until done
    let _ = runtime.block_on(future);

    Ok(())
}
