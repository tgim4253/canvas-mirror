use std::{collections::VecDeque, io, sync::Arc};

use image_server_config::ServerConfig;
use image_server_model::{LogEntryDto, LogLevel};
use image_server_store::{DetectionMode, RoomRecord, RoomStore, StoreError};
use indexmap::IndexMap;
use parking_lot::RwLock;
use tokio::sync::broadcast;

use crate::{
    error::CoreError,
    runtime::{RoomChangeEvent, RoomRuntime, ServerCoreInner},
};

const MAX_LOG_ENTRIES: usize = 1_024;

#[derive(Debug, Clone)]
pub struct ServerCore {
    pub(crate) inner: Arc<RwLock<ServerCoreInner>>,
}

impl ServerCore {
    pub fn load(config: ServerConfig) -> Result<Self, CoreError> {
        let store = match RoomStore::load_from_path(&config.store_path) {
            Ok(store) => store,
            Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                RoomStore::default()
            }
            Err(error) => return Err(CoreError::Store(error)),
        };
        for room in store.rooms() {
            validate_room(room)?;
        }

        // Bootstrap runtime rooms from the persisted room store snapshot.
        let rooms = store
            .rooms()
            .iter()
            .cloned()
            .map(|room| (room.id.clone(), RoomRuntime::new(room)))
            .collect::<IndexMap<_, _>>();

        let mut inner = ServerCoreInner {
            config,
            store,
            rooms,
            room_revision: 0,
            room_events_tx: broadcast::channel(64).0,
            logs: VecDeque::new(),
            log_cursor_start: 0,
        };
        push_log(
            &mut inner,
            LogLevel::Info,
            "server",
            "server runtime initialized".to_string(),
        );
        let loaded_room_count = inner.store.rooms().len();
        push_log(
            &mut inner,
            LogLevel::Info,
            "store",
            format!("loaded {} room(s) from store", loaded_room_count),
        );
        let rooms_to_warn: Vec<RoomRecord> = inner.store.rooms().to_vec();
        for room in &rooms_to_warn {
            push_low_interval_warning(&mut inner, room);
        }

        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }

    pub fn config(&self) -> ServerConfig {
        // Wrapper layers only need read access to boot config values.
        self.inner.read().config.clone()
    }
}

pub(crate) fn push_log(inner: &mut ServerCoreInner, level: LogLevel, scope: &str, message: String) {
    inner.logs.push_back(LogEntryDto {
        at: chrono::Utc::now(),
        level,
        scope: scope.to_string(),
        message,
    });

    while inner.logs.len() > MAX_LOG_ENTRIES {
        inner.logs.pop_front();
        inner.log_cursor_start = inner.log_cursor_start.saturating_add(1);
    }
}

pub(crate) fn bump_room_revision(inner: &mut ServerCoreInner) {
    inner.room_revision = inner.room_revision.wrapping_add(1);
    let _ = inner.room_events_tx.send(RoomChangeEvent {
        revision: inner.room_revision,
    });
}

pub(crate) fn validate_room(room: &RoomRecord) -> Result<(), CoreError> {
    if room.id.trim().is_empty() {
        return Err(CoreError::EmptyRoomId);
    }
    if room.name.trim().is_empty() {
        return Err(CoreError::EmptyRoomName {
            room_id: room.id.clone(),
        });
    }
    if room.interval_ms < 1 {
        return Err(CoreError::InvalidIntervalMs {
            room_id: room.id.clone(),
            interval_ms: room.interval_ms,
        });
    }
    Ok(())
}

pub(crate) fn push_low_interval_warning(inner: &mut ServerCoreInner, room: &RoomRecord) {
    if room.mode == DetectionMode::Interval && room.interval_ms < 100 {
        push_log(
            inner,
            LogLevel::Warn,
            "room",
            format!(
                "room '{}' uses very low interval_ms={} (<100ms); this may be noisy in production",
                room.id, room.interval_ms
            ),
        );
    }
}
