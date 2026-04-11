use std::{fs, path::Path};

use image_server_store::RoomStore;

use crate::error::CoreError;

pub(crate) fn persist_store(path: &Path, store: &RoomStore) -> Result<(), CoreError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(CoreError::StoreDir)?;
    }
    store.save_to_path(path)?;
    Ok(())
}
