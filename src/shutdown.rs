use tokio::sync::broadcast::Receiver;

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
