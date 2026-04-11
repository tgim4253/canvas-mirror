use std::io;

use image_server_store::StoreError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("failed to load or save room store: {0}")]
    Store(#[from] StoreError),
    #[error("room '{room_id}' already exists")]
    RoomAlreadyExists { room_id: String },
    #[error("room '{room_id}' not found")]
    RoomNotFound { room_id: String },
    #[error("device '{device_id}' not found in room '{room_id}'")]
    DeviceNotFound { room_id: String, device_id: String },
    #[error("room '{room_id}' is paused")]
    RoomPaused { room_id: String },
    #[error("runtime store is out of sync for room '{room_id}'")]
    StoreOutOfSync { room_id: String },
    #[error("failed to create room store directory: {0}")]
    StoreDir(io::Error),
}
