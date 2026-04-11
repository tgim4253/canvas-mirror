use std::{path::PathBuf, thread::sleep, time::Duration};

use image_server_config::ServerConfig;
use image_server_model::{DevicePlatform, DeviceState, RoomState};
use image_server_store::{DetectionMode, OutputResolution, RoomRecord, RoomStore};
use tempfile::tempdir;

use crate::{JoinRoomCommand, PublishSnapshotCommand, ServerCore, UpdateRoomCommand};

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
    assert!(matches!(error, crate::CoreError::RoomPaused { .. }));
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
