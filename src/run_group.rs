use std::future::Future;
use tokio::sync::broadcast::Receiver;
use tracing::{debug, info, trace};

/// Shutdown is a basic wrapper around a [`Receiver`]
pub struct Shutdown {
    shutdown_recv: Receiver<()>,
}

impl Shutdown {
    pub fn new(shutdown_recv: Receiver<()>) -> Self {
        Self { shutdown_recv }
    }

    pub async fn recv(&mut self) {
        let _ = self.shutdown_recv.recv().await;
    }
}

/// A "run group" is an abstraction that can be used to spawn tasks that want to be notified when
/// the application shuts down.
///
/// Functions that are to be run as tasks within a run group must take a [`Shutdown`] object as
/// first argument.
/// Then, the function must await somehow on the future returned by [`Shutdown::recv`], for example using [`tokio::select`].
///
/// The function provided to [`RunGroup::run`] must be async and return a [`anyhow::Result`].
/// Once you've added all functions to run, call [`RunGroup::start`].
///
/// All told using [`RunGroup`] looks like this:
/// ```rust,no_run
/// use servare::run_group::{RunGroup,Shutdown};
///
/// # #[tokio::main(flavor = "current_thread")] async fn main() {
///
/// async fn foo(shutdown: Shutdown) -> anyhow::Result<()> {
///     Ok(())
/// }
/// async fn bar(shutdown: Shutdown) -> anyhow::Result<()> {
///     Ok(())
/// }
///
/// let run_group = RunGroup::new()
///     .run(|shutdown| foo(shutdown))
///     .run(|shutdown| bar(shutdown));
///
///
/// run_group.start().await.unwrap();
///
/// # }
/// ```
pub struct RunGroup {
    set: tokio::task::JoinSet<anyhow::Result<()>>,
    shutdown_sender: tokio::sync::broadcast::Sender<()>,
}

impl Default for RunGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl RunGroup {
    pub fn new() -> Self {
        let (shutdown_sender, _) = tokio::sync::broadcast::channel(2);

        Self {
            set: tokio::task::JoinSet::new(),
            shutdown_sender,
        }
    }

    /// Creates a new task that will run the function `f`.
    pub fn run<Func, F>(mut self, f: Func) -> Self
    where
        Func: FnOnce(Shutdown) -> F,
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let shutdown = Shutdown::new(self.shutdown_sender.subscribe());

        let future = f(shutdown);

        self.set.spawn(future);

        self
    }

    /// Start the run group
    pub async fn start(mut self) -> anyhow::Result<()> {
        // Add a final task that will notify all other tasks of a shutdown
        self.set.spawn(async move {
            Self::shutdown_signal().await;

            trace!("got shutdown signal");
            let _ = self.shutdown_sender.send(())?;
            trace!("shutdown notification sent");

            Ok(())
        });

        info!("starting");

        // Wait for all tasks to be done
        while let Some(result) = self.set.join_next().await {
            // First ? operator for the future returned by spawn()
            // Second ? operator for the Result returned by the function.
            result??;

            trace!("future is done");
        }

        info!("shutdown complete");

        Ok(())
    }

    async fn shutdown_signal() {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install Ctrl+C handler");
        };

        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("failed to install signal handler")
                .recv()
                .await;
        };

        tokio::select! {
            _ = ctrl_c => {},
            _ = terminate => {},
        }

        debug!("signal received, starting graceful shutdown");
    }
}
