use std::path::{Path, PathBuf};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;

use crate::error::WatcherError;

pub(crate) struct WatchTrigger {
    receiver: mpsc::Receiver<notify::Result<Event>>,
    _watcher: RecommendedWatcher,
    target_path: PathBuf,
}

impl WatchTrigger {
    pub(crate) fn new(target_path: PathBuf) -> Result<Self, WatcherError> {
        let watch_root = target_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));

        let (sender, receiver) = mpsc::channel(64);
        let mut watcher = notify::recommended_watcher(move |result| {
            let _ = sender.blocking_send(result);
        })?;
        watcher.watch(&watch_root, RecursiveMode::NonRecursive)?;

        Ok(Self {
            receiver,
            _watcher: watcher,
            target_path,
        })
    }

    pub(crate) async fn next(&mut self) -> Result<(), WatcherError> {
        while let Some(result) = self.receiver.recv().await {
            match result {
                Ok(event) if self.event_matches_target(&event) => return Ok(()),
                Ok(_) => continue,
                Err(error) => return Err(WatcherError::Notify(error)),
            }
        }

        Err(WatcherError::ChannelClosed)
    }

    pub(crate) fn drain_pending(&mut self) -> Result<(), WatcherError> {
        loop {
            match self.receiver.try_recv() {
                Ok(Ok(_)) => continue,
                Ok(Err(error)) => return Err(WatcherError::Notify(error)),
                Err(mpsc::error::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(WatcherError::ChannelClosed);
                }
            }
        }
    }

    fn event_matches_target(&self, event: &Event) -> bool {
        event.paths.iter().any(|path| {
            path == &self.target_path || path.file_name() == self.target_path.file_name()
        })
    }
}
