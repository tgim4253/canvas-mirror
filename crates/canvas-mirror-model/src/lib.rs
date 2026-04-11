use canvas_mirror_store::{DetectionMode, OutputResolution, RoomRecord};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoomState {
    Running,
    Paused,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeviceState {
    Online,
    Offline,
    Stale,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Desktop,
    Tablet,
    Phone,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotMetaDto {
    pub room_id: String,
    pub content_hash: String,
    pub mime_type: String,
    pub bytes_len: usize,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomDeviceDto {
    /// Session-like identifier for the current connected endpoint in this room.
    pub id: String,
    pub name: String,
    pub platform: DevicePlatform,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    pub state: DeviceState,
    pub last_seen_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomSummaryDto {
    pub id: String,
    pub name: String,
    pub detection_enabled: bool,
    pub mode: DetectionMode,
    pub interval_ms: u64,
    pub debounce_ms: u64,
    pub stabilize_ms: u64,
    pub resolution: OutputResolution,
}

impl From<&RoomRecord> for RoomSummaryDto {
    fn from(room: &RoomRecord) -> Self {
        Self {
            id: room.id.clone(),
            name: room.name.clone(),
            detection_enabled: room.detection_enabled,
            mode: room.mode.clone(),
            interval_ms: room.interval_ms,
            debounce_ms: room.debounce_ms,
            stabilize_ms: room.stabilize_ms,
            resolution: room.resolution.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomDto {
    pub room: RoomSummaryDto,
    pub state: RoomState,
    pub devices: Vec<RoomDeviceDto>,
    pub latest_snapshot: Option<SnapshotMetaDto>,
    pub last_error: Option<String>,
}

impl RoomDto {
    pub fn device(&self, device_id: &str) -> Option<&RoomDeviceDto> {
        self.devices.iter().find(|device| device.id == device_id)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntryDto {
    pub at: DateTime<Utc>,
    pub level: LogLevel,
    pub scope: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerStatusDto {
    pub generated_at: DateTime<Utc>,
    pub public_url: Option<Url>,
    pub rooms: Vec<RoomDto>,
    pub logs: Vec<LogEntryDto>,
}

impl ServerStatusDto {
    pub fn room(&self, room_id: &str) -> Option<&RoomDto> {
        self.rooms.iter().find(|room| room.room.id == room_id)
    }
}

#[cfg(test)]
mod tests {
    use canvas_mirror_store::{DetectionMode, OutputResolution};
    use chrono::TimeZone;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn enums_serde_round_trip_as_snake_case() {
        let room = serde_json::to_string(&RoomState::Running).expect("room state should encode");
        let device =
            serde_json::to_string(&DeviceState::Online).expect("device state should encode");
        let platform =
            serde_json::to_string(&DevicePlatform::Tablet).expect("device platform should encode");
        let level = serde_json::to_string(&LogLevel::Warn).expect("log level should encode");

        assert_eq!(room, "\"running\"");
        assert_eq!(device, "\"online\"");
        assert_eq!(platform, "\"tablet\"");
        assert_eq!(level, "\"warn\"");

        assert_eq!(
            serde_json::from_str::<RoomState>(&room).expect("room state should decode"),
            RoomState::Running
        );
        assert_eq!(
            serde_json::from_str::<DeviceState>(&device).expect("device state should decode"),
            DeviceState::Online
        );
    }

    #[test]
    fn room_view_preserves_room_record() {
        let view = sample_room_view();

        assert_eq!(view.room.id, "room-a");
        assert_eq!(view.room.mode, DetectionMode::Interval);
    }

    #[test]
    fn server_status_json_round_trip_preserves_nested_devices() {
        let status = sample_status();

        let json = serde_json::to_string_pretty(&status).expect("status should encode");
        let decoded: ServerStatusDto =
            serde_json::from_str(&json).expect("status should decode again");

        assert_eq!(decoded, status);
        assert_eq!(decoded.rooms[0].devices.len(), 1);
    }

    #[test]
    fn room_and_device_lookup_return_expected_items() {
        let status = sample_status();
        let room = status.room("room-a").expect("room lookup should work");
        let device = room.device("device-a").expect("device lookup should work");

        assert_eq!(room.room.name, "Room A");
        assert_eq!(device.name, "Front Desk Tablet");
    }

    #[test]
    fn snapshot_meta_does_not_serialize_server_local_paths() {
        let status = sample_status();
        let json = serde_json::to_string(&status).expect("status should encode");

        assert!(!json.contains("samples/room-a.clip"));
        assert!(!json.contains("source_name"));
    }

    #[test]
    fn optional_fields_round_trip_when_absent() {
        let view = RoomDto {
            room: RoomSummaryDto::from(&sample_room_record()),
            state: RoomState::Paused,
            devices: vec![RoomDeviceDto {
                id: "device-a".to_string(),
                name: "Front Desk Tablet".to_string(),
                platform: DevicePlatform::Tablet,
                screen_width: None,
                screen_height: None,
                state: DeviceState::Paused,
                last_seen_at: None,
            }],
            latest_snapshot: None,
            last_error: None,
        };

        let json = serde_json::to_string(&view).expect("view should encode");
        let decoded: RoomDto = serde_json::from_str(&json).expect("view should decode");

        assert_eq!(decoded, view);
    }

    #[test]
    fn room_view_does_not_serialize_target_path() {
        let view = sample_room_view();
        let json = serde_json::to_string(&view).expect("view should encode");

        assert!(!json.contains("target_path"));
        assert!(!json.contains("samples/room-a.clip"));
    }

    fn sample_status() -> ServerStatusDto {
        ServerStatusDto {
            generated_at: Utc
                .with_ymd_and_hms(2026, 4, 11, 12, 0, 0)
                .single()
                .expect("timestamp must be valid"),
            public_url: Some(
                "http://127.0.0.1:8787"
                    .parse()
                    .expect("sample URL should be valid"),
            ),
            rooms: vec![sample_room_view()],
            logs: vec![LogEntryDto {
                at: Utc
                    .with_ymd_and_hms(2026, 4, 11, 12, 0, 1)
                    .single()
                    .expect("timestamp must be valid"),
                level: LogLevel::Info,
                scope: "room:room-a".to_string(),
                message: "room watcher started".to_string(),
            }],
        }
    }

    fn sample_room_record() -> RoomRecord {
        RoomRecord {
            id: "room-a".to_string(),
            name: "Room A".to_string(),
            viewer_token: "viewer-token-a".to_string(),
            detection_enabled: true,
            target_path: PathBuf::from("./samples/room-a.clip"),
            mode: DetectionMode::Interval,
            interval_ms: 2_500,
            debounce_ms: 800,
            stabilize_ms: 350,
            resolution: OutputResolution::Contain {
                max_width: 1440,
                max_height: 810,
            },
        }
    }

    fn sample_room_view() -> RoomDto {
        RoomDto {
            room: RoomSummaryDto::from(&sample_room_record()),
            state: RoomState::Running,
            devices: vec![RoomDeviceDto {
                id: "device-a".to_string(),
                name: "Front Desk Tablet".to_string(),
                platform: DevicePlatform::Tablet,
                screen_width: Some(1024),
                screen_height: Some(768),
                state: DeviceState::Online,
                last_seen_at: Some(
                    Utc.with_ymd_and_hms(2026, 4, 11, 12, 0, 2)
                        .single()
                        .expect("timestamp must be valid"),
                ),
            }],
            latest_snapshot: Some(SnapshotMetaDto {
                room_id: "room-a".to_string(),
                content_hash: "abc123".to_string(),
                mime_type: "image/png".to_string(),
                bytes_len: 42_000,
                width: Some(1440),
                height: Some(810),
                created_at: Utc
                    .with_ymd_and_hms(2026, 4, 11, 12, 0, 4)
                    .single()
                    .expect("timestamp must be valid"),
            }),
            last_error: Some("preview extraction retrying".to_string()),
        }
    }
}
