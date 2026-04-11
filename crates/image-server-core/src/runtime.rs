use std::{collections::VecDeque, sync::Arc};

use chrono::{DateTime, Utc};
use image_server_config::ServerConfig;
use image_server_model::{DevicePlatform, LogEntryDto, RoomState, SnapshotMetaDto};
use image_server_store::{RoomRecord, RoomStore};
use indexmap::IndexMap;

#[derive(Debug)]
pub(crate) struct ServerCoreInner {
    pub(crate) config: ServerConfig,
    pub(crate) store: RoomStore,
    pub(crate) rooms: IndexMap<String, RoomRuntime>,
    pub(crate) room_revision: u64,
    pub(crate) logs: VecDeque<LogEntryDto>,
    pub(crate) log_cursor_start: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct RoomRuntime {
    pub(crate) room: RoomRecord,
    pub(crate) state: RoomState,
    pub(crate) devices: IndexMap<String, RoomDeviceRuntime>,
    pub(crate) latest_snapshot: Option<SnapshotBuffer>,
    pub(crate) last_error: Option<String>,
}

impl RoomRuntime {
    pub(crate) fn new(room: RoomRecord) -> Self {
        Self {
            state: RoomState::Running,
            room,
            devices: IndexMap::new(),
            latest_snapshot: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RoomDeviceRuntime {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) platform: DevicePlatform,
    pub(crate) joined_at: DateTime<Utc>,
    pub(crate) last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SnapshotBuffer {
    pub meta: SnapshotMetaDto,
    pub bytes: Arc<[u8]>,
}
