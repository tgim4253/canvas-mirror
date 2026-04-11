use std::path::PathBuf;

use image_server_core::CoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WatcherError {
    #[error("watch source failed: {0}")]
    Notify(#[from] notify::Error),
    #[error("failed to read watched file: {0}")]
    Io(#[from] std::io::Error),
    #[error("preview extraction task failed: {0}")]
    ExtractTask(#[from] tokio::task::JoinError),
    #[error("clip preview extraction failed: {0}")]
    ExtractPreview(#[from] clip2preview::ClipError),
    #[error("failed to resize preview: {0}")]
    Resize(#[from] image::ImageError),
    #[error("room runtime update failed: {0}")]
    Core(#[from] CoreError),
    #[error("file did not stabilize: {path} after {attempts} attempts")]
    Stabilization { path: PathBuf, attempts: usize },
    #[error("watch channel closed")]
    ChannelClosed,
}
