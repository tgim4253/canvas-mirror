use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("watch source failed: {0}")]
    Notify(#[from] notify::Error),
    #[error("failed to read watched file: {0}")]
    Io(#[from] std::io::Error),
    #[error("file did not stabilize: {path} after {attempts} attempts")]
    Stabilization { path: PathBuf, attempts: usize },
    #[error("watch channel closed")]
    ChannelClosed,
}
