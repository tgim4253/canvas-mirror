use std::{fs, io, sync::Arc};

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use image_server_config::ServerConfig;
use image_server_model::{
    DevicePlatform, DeviceState, LogEntryDto, LogLevel, RoomDeviceDto, RoomDto, RoomState,
    RoomSummaryDto, ServerStatusDto, SnapshotMetaDto,
};
use image_server_store::{RoomRecord, RoomStore, StoreError};
use indexmap::IndexMap;
use parking_lot::RwLock;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct ServerCore {
    inner: Arc<RwLock<ServerCoreInner>>,
}

#[derive(Debug)]
struct ServerCoreInner {
    config: ServerConfig,
    store: RoomStore,
    rooms: IndexMap<String, RoomRuntime>,
    logs: Vec<LogEntryDto>,
}

#[derive(Debug, Clone)]
struct RoomRuntime {
    room: RoomRecord,
    state: RoomState,
    devices: IndexMap<String, RoomDeviceRuntime>,
    latest_snapshot: Option<SnapshotBuffer>,
    last_error: Option<String>,
}

#[derive(Debug, Clone)]
struct RoomDeviceRuntime {
    id: String,
    name: String,
    platform: DevicePlatform,
    joined_at: DateTime<Utc>,
    last_seen_at: Option<DateTime<Utc>>,
    last_snapshot_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SnapshotBuffer {
    pub meta: SnapshotMetaDto,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateRoomCommand {
    pub name: Option<String>,
    pub target_path: Option<std::path::PathBuf>,
    pub mode: Option<image_server_store::DetectionMode>,
    pub interval_ms: Option<u64>,
    pub debounce_ms: Option<u64>,
    pub stabilize_ms: Option<u64>,
    pub resolution: Option<image_server_store::OutputResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRoomCommand {
    pub id: String,
    pub name: String,
    pub platform: DevicePlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSnapshotCommand {
    pub device_id: Option<String>,
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

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

impl ServerCore {
    pub fn load(config: ServerConfig) -> Result<Self, CoreError> {
        let store = match RoomStore::load_from_path(&config.store_path) {
            Ok(store) => store,
            Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                RoomStore::default()
            }
            Err(error) => return Err(CoreError::Store(error)),
        };

        let rooms = store
            .rooms()
            .iter()
            .cloned()
            .map(|room| (room.id.clone(), RoomRuntime::new(room)))
            .collect();

        let mut logs = Vec::new();
        push_log(
            &mut logs,
            LogLevel::Info,
            "server",
            "server runtime initialized".to_string(),
        );
        push_log(
            &mut logs,
            LogLevel::Info,
            "store",
            format!("loaded {} room(s) from store", store.rooms().len()),
        );

        Ok(Self {
            inner: Arc::new(RwLock::new(ServerCoreInner {
                config,
                store,
                rooms,
                logs,
            })),
        })
    }

    pub fn config(&self) -> ServerConfig {
        self.inner.read().config.clone()
    }

    pub fn status(&self) -> ServerStatusDto {
        let inner = self.inner.read();
        let now = Utc::now();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);

        ServerStatusDto {
            generated_at: now,
            public_url: inner.config.public_url.clone(),
            rooms: inner
                .rooms
                .values()
                .map(|room| room_view(room, now, stale_timeout))
                .collect(),
            logs: inner.logs.clone(),
        }
    }

    pub fn room(&self, room_id: &str) -> Option<RoomDto> {
        let inner = self.inner.read();
        let now = Utc::now();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);

        inner
            .rooms
            .get(room_id)
            .map(|room| room_view(room, now, stale_timeout))
    }

    pub fn create_room(&self, room: RoomRecord) -> Result<RoomDto, CoreError> {
        let room_id = room.id.clone();
        let room_name = room.name.clone();
        let mut inner = self.inner.write();
        if inner.rooms.contains_key(&room_id) {
            return Err(CoreError::RoomAlreadyExists { room_id });
        }

        let mut next_store = inner.store.clone();
        next_store.upsert_room(room.clone());
        persist_store(&inner.config.store_path, &next_store)?;

        let runtime = RoomRuntime::new(room);
        let view = room_view(
            &runtime,
            Utc::now(),
            stale_timeout(inner.config.stale_timeout_ms),
        );

        inner.store = next_store;
        inner.rooms.insert(room_id.clone(), runtime);
        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "room",
            format!("room '{room_name}' ({room_id}) created"),
        );
        Ok(view)
    }

    pub fn update_room(
        &self,
        room_id: &str,
        update: UpdateRoomCommand,
    ) -> Result<RoomDto, CoreError> {
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let current_room = inner
            .rooms
            .get(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?
            .room
            .clone();

        let updated_room = apply_room_update(current_room, update);
        let mut next_store = inner.store.clone();
        next_store.upsert_room(updated_room.clone());
        persist_store(&inner.config.store_path, &next_store)?;

        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        runtime.room = updated_room;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        inner.store = next_store;
        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "room",
            format!("room '{}' updated", room_id),
        );
        Ok(view)
    }

    pub fn delete_room(&self, room_id: &str) -> Result<RoomDto, CoreError> {
        let mut inner = self.inner.write();
        let removed_runtime =
            inner
                .rooms
                .get(room_id)
                .cloned()
                .ok_or_else(|| CoreError::RoomNotFound {
                    room_id: room_id.to_string(),
                })?;

        let mut next_store = inner.store.clone();
        if next_store.remove_room(room_id).is_none() {
            return Err(CoreError::StoreOutOfSync {
                room_id: room_id.to_string(),
            });
        }
        persist_store(&inner.config.store_path, &next_store)?;

        inner.rooms.shift_remove(room_id);
        inner.store = next_store;
        push_log(
            &mut inner.logs,
            LogLevel::Warn,
            "room",
            format!("room '{}' deleted", room_id),
        );

        Ok(room_view(
            &removed_runtime,
            Utc::now(),
            stale_timeout(inner.config.stale_timeout_ms),
        ))
    }

    pub fn pause_room(&self, room_id: &str) -> Result<RoomDto, CoreError> {
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        runtime.state = RoomState::Paused;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner.logs,
            LogLevel::Warn,
            "room",
            format!("room '{}' paused", room_id),
        );
        Ok(view)
    }

    pub fn resume_room(&self, room_id: &str) -> Result<RoomDto, CoreError> {
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        runtime.state = RoomState::Running;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "room",
            format!("room '{}' resumed", room_id),
        );
        Ok(view)
    }

    pub fn set_room_error(
        &self,
        room_id: &str,
        message: impl Into<String>,
    ) -> Result<RoomDto, CoreError> {
        let message = message.into();
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        runtime.state = RoomState::Error;
        runtime.last_error = Some(message.clone());

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner.logs,
            LogLevel::Error,
            "room",
            format!("room '{}' entered error state: {}", room_id, message),
        );
        Ok(view)
    }

    pub fn clear_room_error(&self, room_id: &str) -> Result<RoomDto, CoreError> {
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        runtime.state = RoomState::Running;
        runtime.last_error = None;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "room",
            format!("room '{}' error cleared", room_id),
        );
        Ok(view)
    }

    pub fn join_room(
        &self,
        room_id: &str,
        join: JoinRoomCommand,
    ) -> Result<RoomDeviceDto, CoreError> {
        let mut inner = self.inner.write();
        let stale_timeout = stale_timeout(inner.config.stale_timeout_ms);
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        let now = Utc::now();
        let last_snapshot_at = runtime
            .latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.meta.created_at);

        let existing_joined_at = runtime.devices.get(&join.id).map(|device| device.joined_at);
        runtime.devices.insert(
            join.id.clone(),
            RoomDeviceRuntime {
                id: join.id.clone(),
                name: join.name.clone(),
                platform: join.platform.clone(),
                joined_at: existing_joined_at.unwrap_or(now),
                last_seen_at: Some(now),
                last_snapshot_at,
            },
        );

        let device = runtime
            .devices
            .get(&join.id)
            .ok_or_else(|| CoreError::DeviceNotFound {
                room_id: room_id.to_string(),
                device_id: join.id.clone(),
            })?;
        let view = room_device(device, runtime.state.clone(), now, stale_timeout);
        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "device",
            format!("device '{}' joined room '{}'", join.id, room_id),
        );
        Ok(view)
    }

    pub fn leave_room(&self, room_id: &str, device_id: &str) -> Result<RoomDeviceDto, CoreError> {
        let mut inner = self.inner.write();
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        let device =
            runtime
                .devices
                .shift_remove(device_id)
                .ok_or_else(|| CoreError::DeviceNotFound {
                    room_id: room_id.to_string(),
                    device_id: device_id.to_string(),
                })?;

        let view = RoomDeviceDto {
            id: device.id.clone(),
            name: device.name.clone(),
            platform: device.platform.clone(),
            state: DeviceState::Offline,
            last_seen_at: device.last_seen_at,
            last_snapshot_at: device.last_snapshot_at,
        };
        push_log(
            &mut inner.logs,
            LogLevel::Warn,
            "device",
            format!("device '{}' left room '{}'", device_id, room_id),
        );
        Ok(view)
    }

    pub fn publish_snapshot(
        &self,
        room_id: &str,
        snapshot: PublishSnapshotCommand,
    ) -> Result<SnapshotMetaDto, CoreError> {
        let mut inner = self.inner.write();
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        if runtime.state == RoomState::Paused {
            return Err(CoreError::RoomPaused {
                room_id: room_id.to_string(),
            });
        }

        let created_at = Utc::now();
        let bytes_len = snapshot.bytes.len();
        let content_hash = {
            let mut hasher = Sha256::new();
            hasher.update(&snapshot.bytes);
            format!("{:x}", hasher.finalize())
        };
        let meta = SnapshotMetaDto {
            room_id: room_id.to_string(),
            content_hash,
            mime_type: snapshot
                .mime_type
                .unwrap_or_else(|| "image/png".to_string()),
            bytes_len,
            width: snapshot.width,
            height: snapshot.height,
            created_at,
        };

        if let Some(device_id) = snapshot.device_id.as_deref() {
            let device =
                runtime
                    .devices
                    .get_mut(device_id)
                    .ok_or_else(|| CoreError::DeviceNotFound {
                        room_id: room_id.to_string(),
                        device_id: device_id.to_string(),
                    })?;
            device.last_seen_at = Some(created_at);
            device.last_snapshot_at = Some(created_at);
        }

        runtime.latest_snapshot = Some(SnapshotBuffer {
            meta: meta.clone(),
            bytes: Arc::from(snapshot.bytes.into_boxed_slice()),
        });

        push_log(
            &mut inner.logs,
            LogLevel::Info,
            "snapshot",
            format!("snapshot published for room '{}'", room_id),
        );
        Ok(meta)
    }

    pub fn snapshot(&self, room_id: &str) -> Result<Option<SnapshotBuffer>, CoreError> {
        let inner = self.inner.read();
        let runtime = inner
            .rooms
            .get(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        Ok(runtime.latest_snapshot.clone())
    }
}

impl RoomRuntime {
    fn new(room: RoomRecord) -> Self {
        Self {
            room,
            state: RoomState::Running,
            devices: IndexMap::new(),
            latest_snapshot: None,
            last_error: None,
        }
    }
}

fn apply_room_update(mut room: RoomRecord, update: UpdateRoomCommand) -> RoomRecord {
    if let Some(name) = update.name {
        room.name = name;
    }
    if let Some(target_path) = update.target_path {
        room.target_path = target_path;
    }
    if let Some(mode) = update.mode {
        room.mode = mode;
    }
    if let Some(interval_ms) = update.interval_ms {
        room.interval_ms = interval_ms;
    }
    if let Some(debounce_ms) = update.debounce_ms {
        room.debounce_ms = debounce_ms;
    }
    if let Some(stabilize_ms) = update.stabilize_ms {
        room.stabilize_ms = stabilize_ms;
    }
    if let Some(resolution) = update.resolution {
        room.resolution = resolution;
    }
    room
}

fn room_view(runtime: &RoomRuntime, now: DateTime<Utc>, stale_timeout: ChronoDuration) -> RoomDto {
    RoomDto {
        room: RoomSummaryDto::from(&runtime.room),
        state: runtime.state.clone(),
        devices: runtime
            .devices
            .values()
            .map(|device| room_device(device, runtime.state.clone(), now, stale_timeout))
            .collect(),
        latest_snapshot: runtime
            .latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.meta.clone()),
        last_error: runtime.last_error.clone(),
    }
}

fn room_device(
    device: &RoomDeviceRuntime,
    room_state: RoomState,
    now: DateTime<Utc>,
    stale_timeout: ChronoDuration,
) -> RoomDeviceDto {
    RoomDeviceDto {
        id: device.id.clone(),
        name: device.name.clone(),
        platform: device.platform.clone(),
        state: device_state(room_state, device.last_seen_at, now, stale_timeout),
        last_seen_at: device.last_seen_at,
        last_snapshot_at: device.last_snapshot_at,
    }
}

fn device_state(
    room_state: RoomState,
    last_seen_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    stale_timeout: ChronoDuration,
) -> DeviceState {
    if room_state == RoomState::Paused {
        return DeviceState::Paused;
    }

    let Some(last_seen_at) = last_seen_at else {
        return DeviceState::Offline;
    };

    if stale_timeout >= ChronoDuration::zero() && now - last_seen_at > stale_timeout {
        DeviceState::Stale
    } else {
        DeviceState::Online
    }
}

fn stale_timeout(stale_timeout_ms: u64) -> ChronoDuration {
    let capped = stale_timeout_ms.min(i64::MAX as u64) as i64;
    ChronoDuration::milliseconds(capped)
}

fn persist_store(path: &std::path::Path, store: &RoomStore) -> Result<(), CoreError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(CoreError::StoreDir)?;
    }
    store.save_to_path(path)?;
    Ok(())
}

fn push_log(logs: &mut Vec<LogEntryDto>, level: LogLevel, scope: &str, message: String) {
    logs.push(LogEntryDto {
        at: Utc::now(),
        level,
        scope: scope.to_string(),
        message,
    });
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, thread::sleep, time::Duration};

    use image_server_model::{DeviceState, RoomState};
    use image_server_store::{DetectionMode, OutputResolution};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn load_missing_store_starts_with_empty_status() {
        let dir = tempdir().expect("temp dir should exist");
        let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
            .expect("core should load");

        let status = core.status();
        assert!(status.rooms.is_empty());
        assert!(!status.logs.is_empty());
    }

    #[test]
    fn create_update_and_delete_room_persist_store_changes() {
        let dir = tempdir().expect("temp dir should exist");
        let store_path = dir.path().join("config/rooms.toml");
        let core =
            ServerCore::load(sample_config(store_path.clone(), 30_000)).expect("core should load");

        core.create_room(sample_room("room-a", "Room A"))
            .expect("room should be created");

        let stored = RoomStore::load_from_path(&store_path).expect("store should be saved");
        assert_eq!(stored.rooms().len(), 1);
        assert_eq!(
            stored.room("room-a").map(|room| room.name.as_str()),
            Some("Room A")
        );

        core.update_room(
            "room-a",
            UpdateRoomCommand {
                name: Some("Room A Updated".to_string()),
                mode: Some(DetectionMode::Interval),
                ..UpdateRoomCommand::default()
            },
        )
        .expect("room should be updated");

        let stored = RoomStore::load_from_path(&store_path).expect("updated store should load");
        assert_eq!(
            stored.room("room-a").map(|room| room.name.as_str()),
            Some("Room A Updated")
        );
        assert_eq!(
            stored.room("room-a").map(|room| room.mode.clone()),
            Some(DetectionMode::Interval)
        );

        core.delete_room("room-a").expect("room should be deleted");
        let stored = RoomStore::load_from_path(&store_path).expect("store should still load");
        assert!(stored.rooms().is_empty());
    }

    #[test]
    fn load_uses_existing_room_store() {
        let dir = tempdir().expect("temp dir should exist");
        let store_path = dir.path().join("rooms.toml");

        let mut store = RoomStore::default();
        store.upsert_room(sample_room("room-a", "Room A"));
        store
            .save_to_path(&store_path)
            .expect("seed store should be saved");

        let core = ServerCore::load(sample_config(store_path, 30_000)).expect("core should load");
        let status = core.status();

        assert_eq!(status.rooms.len(), 1);
        assert_eq!(
            status.room("room-a").map(|room| room.room.name.as_str()),
            Some("Room A")
        );
    }

    #[test]
    fn join_room_publish_snapshot_and_read_snapshot_bytes() {
        let dir = tempdir().expect("temp dir should exist");
        let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
            .expect("core should load");
        core.create_room(sample_room("room-a", "Room A"))
            .expect("room should be created");

        core.join_room(
            "room-a",
            JoinRoomCommand {
                id: "device-a".to_string(),
                name: "Front Desk Tablet".to_string(),
                platform: DevicePlatform::Tablet,
            },
        )
        .expect("device should join");

        let meta = core
            .publish_snapshot(
                "room-a",
                PublishSnapshotCommand {
                    device_id: Some("device-a".to_string()),
                    bytes: vec![1, 2, 3, 4],
                    mime_type: Some("image/png".to_string()),
                    width: Some(1440),
                    height: Some(810),
                },
            )
            .expect("snapshot should publish");

        assert_eq!(meta.bytes_len, 4);
        let snapshot = core
            .snapshot("room-a")
            .expect("room should exist")
            .expect("snapshot should exist");
        assert_eq!(snapshot.meta, meta);
        assert_eq!(&*snapshot.bytes, &[1, 2, 3, 4]);

        let room = core.room("room-a").expect("room view should exist");
        assert_eq!(room.devices.len(), 1);
        assert!(room.latest_snapshot.is_some());
        assert!(room.devices[0].last_snapshot_at.is_some());
    }

    #[test]
    fn paused_room_marks_devices_paused_and_rejects_snapshots() {
        let dir = tempdir().expect("temp dir should exist");
        let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
            .expect("core should load");
        core.create_room(sample_room("room-a", "Room A"))
            .expect("room should be created");
        core.join_room(
            "room-a",
            JoinRoomCommand {
                id: "device-a".to_string(),
                name: "Front Desk Tablet".to_string(),
                platform: DevicePlatform::Tablet,
            },
        )
        .expect("device should join");

        core.pause_room("room-a").expect("room should pause");
        let room = core.room("room-a").expect("room should exist");
        assert_eq!(room.state, RoomState::Paused);
        assert_eq!(room.devices[0].state, DeviceState::Paused);

        let error = core
            .publish_snapshot(
                "room-a",
                PublishSnapshotCommand {
                    device_id: None,
                    bytes: vec![1, 2, 3],
                    mime_type: None,
                    width: None,
                    height: None,
                },
            )
            .expect_err("paused room should reject snapshot");
        assert!(matches!(error, CoreError::RoomPaused { .. }));
    }

    #[test]
    fn stale_timeout_marks_room_devices_stale() {
        let dir = tempdir().expect("temp dir should exist");
        let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 1))
            .expect("core should load");
        core.create_room(sample_room("room-a", "Room A"))
            .expect("room should be created");
        core.join_room(
            "room-a",
            JoinRoomCommand {
                id: "device-a".to_string(),
                name: "Front Desk Tablet".to_string(),
                platform: DevicePlatform::Tablet,
            },
        )
        .expect("device should join");

        sleep(Duration::from_millis(10));

        let room = core.room("room-a").expect("room should exist");
        assert_eq!(room.devices[0].state, DeviceState::Stale);
    }

    #[test]
    fn set_and_clear_room_error_updates_runtime_status() {
        let dir = tempdir().expect("temp dir should exist");
        let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
            .expect("core should load");
        core.create_room(sample_room("room-a", "Room A"))
            .expect("room should be created");

        core.set_room_error("room-a", "watcher failed")
            .expect("room should enter error state");
        let room = core.room("room-a").expect("room should exist");
        assert_eq!(room.state, RoomState::Error);
        assert_eq!(room.last_error.as_deref(), Some("watcher failed"));

        core.clear_room_error("room-a")
            .expect("room should clear error");
        let room = core.room("room-a").expect("room should exist");
        assert_eq!(room.state, RoomState::Running);
        assert!(room.last_error.is_none());
    }

    fn sample_config(store_path: PathBuf, stale_timeout_ms: u64) -> ServerConfig {
        ServerConfig {
            store_path,
            stale_timeout_ms,
            ..ServerConfig::default()
        }
    }

    fn sample_room(id: &str, name: &str) -> RoomRecord {
        RoomRecord {
            id: id.to_string(),
            name: name.to_string(),
            target_path: PathBuf::from(format!("./samples/{id}.clip")),
            mode: DetectionMode::Watch,
            interval_ms: 2_000,
            debounce_ms: 750,
            stabilize_ms: 300,
            resolution: OutputResolution::Contain {
                max_width: 1440,
                max_height: 810,
            },
        }
    }
}
