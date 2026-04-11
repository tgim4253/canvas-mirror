use std::{path::PathBuf, time::Duration};

use image_server_config::ServerConfig;
use image_server_core::ServerCore;
use image_server_model::RoomState;
use image_server_store::{DetectionMode, OutputResolution, RoomRecord};
use tempfile::tempdir;

use crate::{WatcherEvent, WatcherService};

#[tokio::test]
async fn interval_watcher_emits_room_event() {
    let dir = tempdir().expect("temp dir should exist");
    let clip_path = dir.path().join("sample.clip");
    tokio::fs::write(&clip_path, b"clip")
        .await
        .expect("clip should exist");

    let core =
        ServerCore::load(sample_config(dir.path().join("rooms.toml"))).expect("core should load");
    core.create_room(sample_room(
        "room-a",
        clip_path.clone(),
        DetectionMode::Interval,
    ))
    .expect("room should be created");

    let runtime = WatcherService::new(core.clone())
        .start()
        .await
        .expect("watcher should start");
    let mut events = runtime.subscribe();

    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("watcher should emit an event before timeout")
        .expect("watcher event channel should stay open");
    assert_eq!(
        event,
        WatcherEvent {
            room_id: "room-a".to_string()
        }
    );

    runtime.shutdown().await;
}

#[tokio::test]
async fn paused_room_skips_watcher_events() {
    let dir = tempdir().expect("temp dir should exist");
    let clip_path = dir.path().join("sample.clip");
    tokio::fs::write(&clip_path, b"clip")
        .await
        .expect("clip should exist");

    let core =
        ServerCore::load(sample_config(dir.path().join("rooms.toml"))).expect("core should load");
    core.create_room(sample_room("room-a", clip_path, DetectionMode::Interval))
        .expect("room should be created");
    core.pause_room("room-a")
        .expect("detection should be disabled");

    let runtime = WatcherService::new(core.clone())
        .start()
        .await
        .expect("watcher should start");

    tokio::time::sleep(Duration::from_millis(80)).await;

    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.state, RoomState::Paused);
    assert!(room.room.detection_enabled);

    runtime.shutdown().await;
}

#[tokio::test]
async fn create_room_after_start_gets_watcher_attached() {
    let dir = tempdir().expect("temp dir should exist");
    let clip_path = dir.path().join("late.clip");
    tokio::fs::write(&clip_path, b"clip")
        .await
        .expect("clip should exist");

    let core =
        ServerCore::load(sample_config(dir.path().join("rooms.toml"))).expect("core should load");
    let runtime = WatcherService::new(core.clone())
        .start()
        .await
        .expect("watcher should start");
    let mut events = runtime.subscribe();

    assert_eq!(runtime.attached_rooms(), 0);

    core.create_room(sample_room("room-a", clip_path, DetectionMode::Interval))
        .expect("room should be created");

    let event = tokio::time::timeout(Duration::from_secs(2), events.recv())
        .await
        .expect("watcher should emit an event before timeout")
        .expect("watcher event channel should stay open");
    assert_eq!(event.room_id, "room-a");

    runtime.shutdown().await;
}

#[tokio::test]
async fn start_isolated_from_bad_watch_room() {
    let dir = tempdir().expect("temp dir should exist");
    let core =
        ServerCore::load(sample_config(dir.path().join("rooms.toml"))).expect("core should load");
    core.create_room(sample_room(
        "room-a",
        dir.path().join("missing-dir").join("sample.clip"),
        DetectionMode::Watch,
    ))
    .expect("room should be created");

    let runtime = WatcherService::new(core.clone())
        .start()
        .await
        .expect("watcher service should still start");

    tokio::time::sleep(Duration::from_millis(600)).await;

    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.state, RoomState::Error);
    assert!(room.last_error.is_some());
    assert_eq!(runtime.attached_rooms(), 0);

    runtime.shutdown().await;
}

#[tokio::test]
async fn disabled_room_does_not_attach_watcher() {
    let dir = tempdir().expect("temp dir should exist");
    let clip_path = dir.path().join("sample.clip");
    tokio::fs::write(&clip_path, b"clip")
        .await
        .expect("clip should exist");

    let core =
        ServerCore::load(sample_config(dir.path().join("rooms.toml"))).expect("core should load");
    let mut room = sample_room("room-a", clip_path, DetectionMode::Interval);
    room.detection_enabled = false;
    core.create_room(room).expect("room should be created");

    let runtime = WatcherService::new(core.clone())
        .start()
        .await
        .expect("watcher should start");

    tokio::time::sleep(Duration::from_millis(80)).await;

    assert_eq!(runtime.attached_rooms(), 0);
    let room = core.room("room-a").expect("room should exist");
    assert_eq!(room.state, RoomState::Running);
    assert!(!room.room.detection_enabled);

    runtime.shutdown().await;
}

fn sample_config(store_path: PathBuf) -> ServerConfig {
    ServerConfig {
        store_path,
        stale_timeout_ms: 30_000,
        ..ServerConfig::default()
    }
}

fn sample_room(id: &str, target_path: PathBuf, mode: DetectionMode) -> RoomRecord {
    RoomRecord {
        id: id.to_string(),
        name: "Room A".to_string(),
        viewer_token: format!("viewer-token-{id}"),
        detection_enabled: true,
        target_path,
        mode,
        interval_ms: 20,
        debounce_ms: 10,
        stabilize_ms: 1,
        resolution: OutputResolution::Source,
    }
}
