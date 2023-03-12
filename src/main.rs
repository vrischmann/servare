use read_input::InputBuild;
use secrecy::Secret;
use servare::authentication::create_user;
use servare::configuration::{get_configuration, Config};
use servare::domain::UserEmail;
use servare::job::JobRunner;
use servare::run_group::RunGroup;
use servare::startup::get_connection_pool;
use servare::startup::Application;
use servare::telemetry;
use tracing::{error, info};

async fn run_serve(config: Config, _matches: &clap::ArgMatches) -> anyhow::Result<()> {
    // Setup

    let subscriber = telemetry::SubscriberBuilder::new("servare")
        .with_logging_targets(config.tracing.targets.logging.into())
        .with_jaeger_endpoint(config.jaeger.map(|v| v.endpoint()))
        .with_jaeger_targets(config.tracing.targets.jaeger.map(|v| v.into()))
        .build(std::io::stdout);
    telemetry::init_global_default(subscriber);

    //
    // Build the application
    //

    let app_pool = get_connection_pool(&config.database).await?;
    let app = Application::build(&config.application, &config.session, app_pool)?;

    info!(
        url = format!(
            "{}:{}",
            config.application.base_url, config.application.port
        ),
        "running dashboard app"
    );

    //
    // Build the job runner
    //

    let job_runner_pool = get_connection_pool(&config.database).await?;
    let job_runner = JobRunner::new(config.job, job_runner_pool)?;

    //
    // Finally start everything
    //

    RunGroup::new()
        .run(|shutdown| app.run(shutdown))
        .run(|shutdown| job_runner.run(shutdown))
        .start()
        .await?;

    Ok(())
}

async fn run_users(config: Config, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    match matches.subcommand() {
        Some(("setup-admin", matches)) => {
            // Email comes from the cli arguments
            let email = {
                let tmp = matches.get_one::<String>("email").unwrap();
                UserEmail::parse(tmp.to_string())?
            };

            // Password is read from the terminal
            let password = {
                let tmp = read_input::prelude::input::<String>()
                    .msg("Password: ")
                    .get();

                Secret::new(tmp)
            };

            let pool = get_connection_pool(&config.database).await?;

            // Create the admin user
            let user_id = create_user(&pool, &email, password).await?;

            println!("created user {}. id={}", email, user_id);

            Ok(())
        }
        _ => Ok(()),
    }
}

async fn run_commands(config: Config, matches: &clap::ArgMatches) -> anyhow::Result<()> {
    match matches.subcommand() {
        Some(("users", matches)) => run_users(config, matches).await?,
        Some(("serve", matches)) => run_serve(config, matches).await?,
        _ => unreachable!("should never happen because of subcommand_required"),
    }
    Ok(())
}

fn main() {
    // Always read the configuration
    let config = match get_configuration() {
        Ok(config) => config,
        Err(err) => {
            error!(err = %err, "unable to get the configuration");
            std::process::exit(1)
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

    // Parse the command line arguments to know what to do
    let root_command = clap::Command::new("servare")
        .version(clap::crate_version!())
        .about("Servare")
        .subcommand_required(true)
        .subcommand(
            clap::Command::new("users")
                .about("Manage users of Sercare")
                .subcommand_required(true)
                .subcommand(
                    clap::Command::new("setup-admin")
                        .about("Setup the admin user")
                        .arg(
                            clap::Arg::new("email")
                                .help("The admin user email")
                                .action(clap::ArgAction::Set)
                                .value_name("EMAIL")
                                .required(true),
                        ),
                ),
        )
        .subcommand(clap::Command::new("serve").about("Serve the application"));

    let matches = root_command.get_matches();
    let future = run_commands(config, &matches);

    // Run the future until done
    let result = runtime.block_on(future);

    if let Err(err) = result {
        println!("{}", err);
        std::process::exit(1);
    }
}
