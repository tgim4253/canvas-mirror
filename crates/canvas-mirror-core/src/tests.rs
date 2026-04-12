use std::{path::PathBuf, thread::sleep, time::Duration};

use canvas_mirror_config::ServerConfig;
use canvas_mirror_model::{DevicePlatform, DeviceState, LogLevel, RoomState};
use canvas_mirror_store::{
    DetectionMode, OutputResolution, RoomRecord, RoomStore, StoredIccProfile,
};
use tempfile::tempdir;

use crate::{CoreError, JoinRoomCommand, PublishSnapshotCommand, ServerCore, UpdateRoomCommand};

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
    assert!(stored
        .room("room-a")
        .map(|room| !room.viewer_token.is_empty())
        .unwrap_or(false));

    core.update_room(
        "room-a",
        UpdateRoomCommand {
            name: Some("Room A Updated".to_string()),
            detection_enabled: Some(false),
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
    assert_eq!(
        stored.room("room-a").map(|room| room.detection_enabled),
        Some(false)
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
fn load_backfills_missing_viewer_tokens_in_store() {
    let dir = tempdir().expect("temp dir should exist");
    let store_path = dir.path().join("rooms.toml");

    let mut store = RoomStore::default();
    let mut room = sample_room("room-a", "Room A");
    room.viewer_token.clear();
    store.upsert_room(room);
    store
        .save_to_path(&store_path)
        .expect("seed store should be saved");

    let core = ServerCore::load(sample_config(store_path.clone(), 30_000))
        .expect("core should load and backfill tokens");

    let stored = RoomStore::load_from_path(&store_path).expect("store should reload");
    let stored_token = stored
        .room("room-a")
        .map(|room| room.viewer_token.clone())
        .expect("room should exist");

    assert!(!stored_token.is_empty());
    assert_eq!(
        core.room_record("room-a")
            .map(|room| room.viewer_token)
            .expect("room should exist"),
        stored_token
    );
}

#[test]
fn update_room_can_set_and_clear_icc_profile() {
    let dir = tempdir().expect("temp dir should exist");
    let store_path = dir.path().join("config/rooms.toml");
    let core =
        ServerCore::load(sample_config(store_path.clone(), 30_000)).expect("core should load");
    core.create_room(sample_room("room-a", "Room A"))
        .expect("room should be created");

    let icc_profile = StoredIccProfile {
        name: "LG ULTRAFINE".to_string(),
        bytes: vec![0, 1, 2, 3],
    };

    core.update_room(
        "room-a",
        UpdateRoomCommand {
            icc_profile_enabled: Some(true),
            icc_profile: Some(Some(icc_profile.clone())),
            ..UpdateRoomCommand::default()
        },
    )
    .expect("room ICC profile should be set");

    let stored = RoomStore::load_from_path(&store_path).expect("store should load");
    assert_eq!(
        stored.room("room-a").map(|room| room.icc_profile_enabled),
        Some(true)
    );
    assert_eq!(
        stored
            .room("room-a")
            .and_then(|room| room.icc_profile.clone()),
        Some(icc_profile)
    );

    core.update_room(
        "room-a",
        UpdateRoomCommand {
            icc_profile_enabled: Some(false),
            icc_profile: Some(None),
            ..UpdateRoomCommand::default()
        },
    )
    .expect("room ICC profile should be cleared");

    let stored = RoomStore::load_from_path(&store_path).expect("store should load");
    assert_eq!(
        stored.room("room-a").map(|room| room.icc_profile_enabled),
        Some(false)
    );
    assert_eq!(
        stored
            .room("room-a")
            .and_then(|room| room.icc_profile.clone()),
        None
    );
}

#[test]
fn load_rejects_room_with_zero_interval() {
    let dir = tempdir().expect("temp dir should exist");
    let store_path = dir.path().join("rooms.toml");

    let mut store = RoomStore::default();
    let mut room = sample_room("room-a", "Room A");
    room.interval_ms = 0;
    store.upsert_room(room);
    store
        .save_to_path(&store_path)
        .expect("seed store should be saved");

    let error = ServerCore::load(sample_config(store_path, 30_000))
        .expect_err("invalid room should fail to load");

    assert!(matches!(
        error,
        CoreError::InvalidIntervalMs { ref room_id, interval_ms }
            if room_id == "room-a" && interval_ms == 0
    ));
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("device should join");

    let meta = core
        .publish_snapshot(
            "room-a",
            PublishSnapshotCommand {
                content_hash: "preview-hash-a".to_string(),
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
}

#[test]
fn publish_snapshot_emits_snapshot_event_without_bumping_room_revision() {
    let dir = tempdir().expect("temp dir should exist");
    let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
        .expect("core should load");
    core.create_room(sample_room("room-a", "Room A"))
        .expect("room should be created");

    let mut snapshot_events = core.subscribe_snapshot_events();
    let initial_revision = core.room_revision();

    core.publish_snapshot(
        "room-a",
        PublishSnapshotCommand {
            content_hash: "preview-hash-a".to_string(),
            bytes: vec![1, 2, 3, 4],
            mime_type: Some("image/png".to_string()),
            width: Some(1440),
            height: Some(810),
        },
    )
    .expect("snapshot should publish");

    let event = snapshot_events
        .try_recv()
        .expect("snapshot event should be emitted");
    assert_eq!(event.room_id, "room-a");
    assert_eq!(event.content_hash, "preview-hash-a");
    assert_eq!(core.room_revision(), initial_revision);
}

#[test]
fn duplicate_device_id_in_same_room_is_rejected() {
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("first join should succeed");

    let error = core
        .join_room(
            "room-a",
            JoinRoomCommand {
                id: "device-a".to_string(),
                name: "Front Desk Tablet 2".to_string(),
                platform: DevicePlatform::Desktop,
                screen_width: Some(1920),
                screen_height: Some(1080),
            },
        )
        .expect_err("duplicate device id in same room should be rejected");

    assert!(matches!(
        error,
        CoreError::DuplicateDeviceIdInRoom {
            ref room_id,
            ref device_id,
        } if room_id == "room-a" && device_id == "device-a"
    ));
}

#[test]
fn different_device_ids_can_join_same_room() {
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("first device should join");
    core.join_room(
        "room-a",
        JoinRoomCommand {
            id: "device-b".to_string(),
            name: "Lobby Display".to_string(),
            platform: DevicePlatform::Desktop,
            screen_width: Some(1920),
            screen_height: Some(1080),
        },
    )
    .expect("second device should join");

    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.devices.len(), 2);
}

#[test]
fn same_device_id_is_allowed_in_different_rooms() {
    let dir = tempdir().expect("temp dir should exist");
    let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
        .expect("core should load");
    core.create_room(sample_room("room-a", "Room A"))
        .expect("room a should be created");
    core.create_room(sample_room("room-b", "Room B"))
        .expect("room b should be created");

    core.join_room(
        "room-a",
        JoinRoomCommand {
            id: "device-a".to_string(),
            name: "Front Desk Tablet".to_string(),
            platform: DevicePlatform::Tablet,
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("room a join should succeed");
    core.join_room(
        "room-b",
        JoinRoomCommand {
            id: "device-a".to_string(),
            name: "Front Desk Tablet".to_string(),
            platform: DevicePlatform::Tablet,
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("room b join should also succeed");

    assert_eq!(
        core.room("room-a")
            .expect("room a should exist")
            .devices
            .len(),
        1
    );
    assert_eq!(
        core.room("room-b")
            .expect("room b should exist")
            .devices
            .len(),
        1
    );
}

#[test]
fn leave_room_allows_same_device_id_to_rejoin() {
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("first join should succeed");
    core.leave_room("room-a", "device-a")
        .expect("leave should succeed");
    core.join_room(
        "room-a",
        JoinRoomCommand {
            id: "device-a".to_string(),
            name: "Front Desk Tablet".to_string(),
            platform: DevicePlatform::Tablet,
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("rejoin after leave should succeed");

    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.devices.len(), 1);
    assert_eq!(room.devices[0].id, "device-a");
}

#[test]
fn create_room_rejects_empty_id_and_name() {
    let dir = tempdir().expect("temp dir should exist");
    let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
        .expect("core should load");

    let error = core
        .create_room(sample_room("", "Room A"))
        .expect_err("empty room id should be rejected");
    assert!(matches!(error, CoreError::EmptyRoomId));

    let error = core
        .create_room(sample_room("room-a", "   "))
        .expect_err("empty room name should be rejected");
    assert!(matches!(
        error,
        CoreError::EmptyRoomName { ref room_id } if room_id == "room-a"
    ));
}

#[test]
fn paused_room_remains_queryable_but_rejects_snapshot_publish() {
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("device should join");

    core.pause_room("room-a").expect("room should pause");
    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.state, RoomState::Paused);
    assert!(room.room.detection_enabled);
    assert_eq!(room.devices[0].state, DeviceState::Paused);

    let error = core
        .publish_snapshot(
            "room-a",
            PublishSnapshotCommand {
                content_hash: "paused-preview-hash".to_string(),
                bytes: vec![1, 2, 3],
                mime_type: None,
                width: None,
                height: None,
            },
        )
        .expect_err("paused room should reject snapshot publish");
    assert!(matches!(
        error,
        CoreError::RoomPaused { ref room_id } if room_id == "room-a"
    ));
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("device should join");

    sleep(Duration::from_millis(10));

    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.devices[0].state, DeviceState::Stale);
}

#[test]
fn touch_device_refreshes_last_seen_and_preserves_capabilities() {
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
            screen_width: Some(1024),
            screen_height: Some(768),
        },
    )
    .expect("device should join");

    let before = core.room("room-a").expect("room should exist").devices[0]
        .last_seen_at
        .expect("join should set last_seen_at");
    sleep(Duration::from_millis(2));
    core.touch_device("room-a", "device-a")
        .expect("touch should succeed");

    let device = &core.room("room-a").expect("room should exist").devices[0];
    assert_eq!(device.screen_width, Some(1024));
    assert_eq!(device.screen_height, Some(768));
    assert!(
        device
            .last_seen_at
            .expect("touch should refresh last_seen_at")
            >= before
    );
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

#[test]
fn detection_disabled_room_starts_running_and_resume_does_not_mutate_flag() {
    let dir = tempdir().expect("temp dir should exist");
    let store_path = dir.path().join("rooms.toml");
    let core =
        ServerCore::load(sample_config(store_path.clone(), 30_000)).expect("core should load");

    let mut room = sample_room("room-a", "Room A");
    room.detection_enabled = false;
    core.create_room(room).expect("room should be created");

    let initial = core.room("room-a").expect("room should exist");
    assert_eq!(initial.state, RoomState::Running);
    assert!(!initial.room.detection_enabled);

    core.pause_room("room-a").expect("room should pause");
    let paused = core.room("room-a").expect("room should exist");
    assert_eq!(paused.state, RoomState::Paused);
    assert!(!paused.room.detection_enabled);

    core.resume_room("room-a").expect("room should resume");
    let resumed = core.room("room-a").expect("room should exist");
    assert_eq!(resumed.state, RoomState::Running);
    assert!(!resumed.room.detection_enabled);

    let stored = RoomStore::load_from_path(&store_path).expect("store should load");
    assert_eq!(
        stored.room("room-a").map(|room| room.detection_enabled),
        Some(false)
    );
}

#[test]
fn low_interval_room_emits_warning_log() {
    let dir = tempdir().expect("temp dir should exist");
    let core = ServerCore::load(sample_config(dir.path().join("rooms.toml"), 30_000))
        .expect("core should load");
    let mut room = sample_room("room-a", "Room A");
    room.mode = DetectionMode::Interval;
    room.interval_ms = 50;

    core.create_room(room).expect("room should be created");

    let status = core.status();
    assert!(status.logs.iter().any(|log| {
        log.level == LogLevel::Warn
            && log
                .message
                .contains("room 'room-a' uses very low interval_ms=50")
    }));
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
        viewer_token: format!("viewer-token-{id}"),
        detection_enabled: true,
        target_path: PathBuf::from(format!("./samples/{id}.clip")),
        mode: DetectionMode::Watch,
        interval_ms: 2_000,
        debounce_ms: 750,
        stabilize_ms: 300,
        resolution: OutputResolution::Contain {
            max_width: 1440,
            max_height: 810,
        },
        icc_profile_enabled: false,
        icc_profile: None,
    }
}
