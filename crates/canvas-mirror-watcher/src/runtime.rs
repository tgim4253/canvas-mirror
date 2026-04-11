use std::sync::{Arc, RwLock};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatcherEvent {
    pub room_id: String,
}

pub struct WatcherRuntime {
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    supervisor: tokio::task::JoinHandle<()>,
    state: Arc<RwLock<Vec<String>>>,
    events_tx: tokio::sync::broadcast::Sender<WatcherEvent>,
}

impl WatcherRuntime {
    pub(crate) fn new(
        shutdown_tx: tokio::sync::watch::Sender<bool>,
        supervisor: tokio::task::JoinHandle<()>,
        state: Arc<RwLock<Vec<String>>>,
        events_tx: tokio::sync::broadcast::Sender<WatcherEvent>,
    ) -> Self {
        Self {
            shutdown_tx,
            supervisor,
            state,
            events_tx,
        }
    }

    pub fn attached_rooms(&self) -> usize {
        self.state
            .read()
            .expect("watcher state lock poisoned")
            .len()
    }

    pub fn room_ids(&self) -> Vec<String> {
        self.state
            .read()
            .expect("watcher state lock poisoned")
            .clone()
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<WatcherEvent> {
        self.events_tx.subscribe()
    }

    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        let _ = (&mut self.supervisor).await;
    }
}

impl Drop for WatcherRuntime {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.supervisor.abort();
    }
}
