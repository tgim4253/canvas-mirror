use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use tokio::time::sleep;

use crate::error::WatcherError;

pub(crate) fn resolve_target_path(store_path: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        return target_path.to_path_buf();
    }

    let base_dir = store_path.parent().unwrap_or_else(|| Path::new("."));
    base_dir.join(target_path)
}

pub(crate) async fn stabilize(path: &Path, stabilize_ms: u64) -> Result<(), WatcherError> {
    if stabilize_ms == 0 {
        return Ok(());
    }

    let path = path.to_path_buf();
    let wait = Duration::from_millis(stabilize_ms);
    let mut previous = None;
    let attempts = 12usize;

    for _ in 0..attempts {
        let metadata = tokio::fs::metadata(&path).await?;
        let fingerprint = (metadata.len(), metadata.modified().ok());

        if previous == Some(fingerprint) {
            return Ok(());
        }

        previous = Some(fingerprint);
        sleep(wait).await;
    }

    Err(WatcherError::Stabilization { path, attempts })
}
