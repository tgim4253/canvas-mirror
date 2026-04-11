use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMode {
    Watch,
    Interval,
}

impl Default for DetectionMode {
    fn default() -> Self {
        Self::Watch
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutputResolution {
    Source,
    Contain { max_width: u32, max_height: u32 },
}

impl Default for OutputResolution {
    fn default() -> Self {
        Self::Contain {
            max_width: 1440,
            max_height: 1440,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomRecord {
    pub id: String,
    pub name: String,
    pub target_path: PathBuf,
    #[serde(default)]
    pub mode: DetectionMode,
    #[serde(default = "default_interval_ms")]
    pub interval_ms: u64,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,
    #[serde(default = "default_stabilize_ms")]
    pub stabilize_ms: u64,
    #[serde(default)]
    pub resolution: OutputResolution,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoomStore {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    rooms: Vec<RoomRecord>,
}

impl Default for RoomStore {
    fn default() -> Self {
        Self {
            version: default_version(),
            rooms: Vec::new(),
        }
    }
}

impl RoomStore {
    pub fn from_json_str(input: &str) -> Result<Self, StoreError> {
        let store: Self = serde_json::from_str(input).map_err(StoreError::JsonDe)?;
        store.validate_unique_room_ids()?;
        Ok(store)
    }

    pub fn to_json_string(&self) -> Result<String, StoreError> {
        self.validate_unique_room_ids()?;
        serde_json::to_string_pretty(self).map_err(StoreError::JsonSer)
    }

    pub fn from_toml_str(input: &str) -> Result<Self, StoreError> {
        let store: Self = toml::from_str(input).map_err(StoreError::TomlDe)?;
        store.validate_unique_room_ids()?;
        Ok(store)
    }

    pub fn to_toml_string(&self) -> Result<String, StoreError> {
        self.validate_unique_room_ids()?;
        toml::to_string_pretty(self).map_err(StoreError::TomlSer)
    }

    pub fn load_from_path(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        let content = fs::read_to_string(path)?;
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Self::from_json_str(&content),
            _ => Self::from_toml_str(&content),
        }
    }

    pub fn save_to_path(&self, path: impl AsRef<Path>) -> Result<(), StoreError> {
        let path = path.as_ref();
        let content = match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => self.to_json_string()?,
            _ => self.to_toml_string()?,
        };
        fs::write(path, content)?;
        Ok(())
    }

    pub fn room(&self, room_id: &str) -> Option<&RoomRecord> {
        self.rooms.iter().find(|room| room.id == room_id)
    }

    pub fn rooms(&self) -> &[RoomRecord] {
        &self.rooms
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn upsert_room(&mut self, room: RoomRecord) -> Option<RoomRecord> {
        if let Some(index) = self
            .rooms
            .iter()
            .position(|existing| existing.id == room.id)
        {
            let previous = std::mem::replace(&mut self.rooms[index], room);
            Some(previous)
        } else {
            self.rooms.push(room);
            None
        }
    }

    pub fn remove_room(&mut self, room_id: &str) -> Option<RoomRecord> {
        let index = self.rooms.iter().position(|room| room.id == room_id)?;
        Some(self.rooms.remove(index))
    }

    fn validate_unique_room_ids(&self) -> Result<(), StoreError> {
        let mut seen = HashSet::new();

        for room in &self.rooms {
            if !seen.insert(room.id.as_str()) {
                return Err(StoreError::DuplicateRoomId {
                    room_id: room.id.clone(),
                });
            }
        }

        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to read room store file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse json room store: {0}")]
    JsonDe(serde_json::Error),
    #[error("failed to encode json room store: {0}")]
    JsonSer(serde_json::Error),
    #[error("failed to parse toml room store: {0}")]
    TomlDe(toml::de::Error),
    #[error("failed to encode toml room store: {0}")]
    TomlSer(toml::ser::Error),
    #[error("duplicate room id found in room store: {room_id}")]
    DuplicateRoomId { room_id: String },
}

fn default_debounce_ms() -> u64 {
    750
}

fn default_interval_ms() -> u64 {
    2_000
}

fn default_stabilize_ms() -> u64 {
    300
}

fn default_version() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn default_store_has_expected_defaults() {
        let store = RoomStore::default();

        assert_eq!(store.version(), 1);
        assert!(store.rooms().is_empty());
    }

    #[test]
    fn toml_round_trip_preserves_rooms() {
        let store = sample_store();

        let toml = store.to_toml_string().expect("toml encoding should work");
        let decoded = RoomStore::from_toml_str(&toml).expect("toml decoding should work");

        assert_eq!(decoded, store);
    }

    #[test]
    fn json_round_trip_preserves_rooms() {
        let store = sample_store();

        let json = store.to_json_string().expect("json encoding should work");
        let decoded = RoomStore::from_json_str(&json).expect("json decoding should work");

        assert_eq!(decoded, store);
    }

    #[test]
    fn save_and_load_toml_file_round_trip() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let path = dir.path().join("image-server-store.toml");
        let store = sample_store();

        store.save_to_path(&path).expect("store should be saved");
        let loaded = RoomStore::load_from_path(&path).expect("store should be loaded");

        let raw = fs::read_to_string(path).expect("saved file should be readable");
        assert!(raw.contains("[[rooms]]"));
        assert_eq!(loaded, store);
    }

    #[test]
    fn room_lookup_helpers_work() {
        let store = sample_store();

        assert_eq!(
            store.room("room-a").map(|room| room.name.as_str()),
            Some("Room A")
        );
    }

    #[test]
    fn upsert_room_replaces_existing_room() {
        let mut store = sample_store();
        let replacement = RoomRecord {
            id: "room-a".to_string(),
            name: "Updated Room".to_string(),
            target_path: PathBuf::from("./samples/updated.clip"),
            mode: DetectionMode::Watch,
            interval_ms: 3_000,
            debounce_ms: 900,
            stabilize_ms: 450,
            resolution: OutputResolution::Source,
        };

        let previous = store.upsert_room(replacement.clone());

        assert_eq!(previous.map(|room| room.name), Some("Room A".to_string()));
        assert_eq!(store.room("room-a"), Some(&replacement));
    }

    #[test]
    fn remove_room_returns_removed_record() {
        let mut store = sample_store();

        let removed = store
            .remove_room("room-a")
            .expect("remove_room should return existing room");

        assert_eq!(removed.id, "room-a");
        assert!(store.room("room-a").is_none());
    }

    #[test]
    fn enum_serde_uses_snake_case() {
        let toml = r#"
            [[rooms]]
            id = "room-a"
            name = "Room A"
            target_path = "./samples/a.clip"
            mode = "interval"
            interval_ms = 2000
            debounce_ms = 750
            stabilize_ms = 300

            [rooms.resolution]
            kind = "contain"
            max_width = 1440
            max_height = 810
        "#;

        let decoded = RoomStore::from_toml_str(toml).expect("toml should decode");
        assert_eq!(decoded.rooms[0].mode, DetectionMode::Interval);
        assert_eq!(
            decoded.rooms[0].resolution,
            OutputResolution::Contain {
                max_width: 1440,
                max_height: 810,
            }
        );
    }

    #[test]
    fn invalid_enum_returns_decode_error() {
        let error = RoomStore::from_json_str(
            r#"{
                "rooms": [{
                    "id": "room-a",
                    "name": "Room A",
                    "target_path": "./samples/a.clip",
                    "mode": "polling"
                }]
            }"#,
        )
        .expect_err("invalid enum should fail");

        assert!(matches!(error, StoreError::JsonDe(_)));
    }

    #[test]
    fn duplicate_room_ids_are_rejected_when_loading() {
        let error = RoomStore::from_toml_str(
            r#"
                [[rooms]]
                id = "room-a"
                name = "Room A"
                target_path = "./samples/a.clip"

                [[rooms]]
                id = "room-a"
                name = "Room A Duplicate"
                target_path = "./samples/b.clip"
            "#,
        )
        .expect_err("duplicate room IDs should fail");

        assert!(matches!(
            error,
            StoreError::DuplicateRoomId { room_id } if room_id == "room-a"
        ));
    }

    #[test]
    fn duplicate_room_ids_are_rejected_when_encoding() {
        let store = RoomStore {
            version: 1,
            rooms: vec![
                RoomRecord {
                    id: "room-a".to_string(),
                    name: "Room A".to_string(),
                    target_path: PathBuf::from("./samples/a.clip"),
                    mode: DetectionMode::Watch,
                    interval_ms: 2_000,
                    debounce_ms: 750,
                    stabilize_ms: 300,
                    resolution: OutputResolution::Source,
                },
                RoomRecord {
                    id: "room-a".to_string(),
                    name: "Room A Duplicate".to_string(),
                    target_path: PathBuf::from("./samples/b.clip"),
                    mode: DetectionMode::Interval,
                    interval_ms: 2_500,
                    debounce_ms: 800,
                    stabilize_ms: 350,
                    resolution: OutputResolution::Contain {
                        max_width: 1440,
                        max_height: 810,
                    },
                },
            ],
        };

        let error = store
            .to_json_string()
            .expect_err("duplicate room IDs should fail during encoding");

        assert!(matches!(
            error,
            StoreError::DuplicateRoomId { room_id } if room_id == "room-a"
        ));
    }

    fn sample_store() -> RoomStore {
        RoomStore {
            version: 1,
            rooms: vec![RoomRecord {
                id: "room-a".to_string(),
                name: "Room A".to_string(),
                target_path: PathBuf::from("./samples/room-a.clip"),
                mode: DetectionMode::Interval,
                interval_ms: 2_500,
                debounce_ms: 800,
                stabilize_ms: 350,
                resolution: OutputResolution::Contain {
                    max_width: 1440,
                    max_height: 810,
                },
            }],
        }
    }
}
