use chrono::Utc;
use image_server_model::{LogLevel, RoomDto};
use image_server_store::RoomRecord;

use crate::{
    commands::UpdateRoomCommand,
    error::CoreError,
    persistence::persist_store,
    projection::{room_view, stale_timeout},
    runtime::RoomRuntime,
    server::{
        bump_room_revision, ensure_room_viewer_token, push_log, push_low_interval_warning,
        validate_room, ServerCore,
    },
};

impl ServerCore {
    pub fn create_room(&self, mut room: RoomRecord) -> Result<RoomDto, CoreError> {
        ensure_room_viewer_token(&mut room);
        validate_room(&room)?;
        let room_id = room.id.clone();
        let room_name = room.name.clone();
        let room_for_warning = room.clone();
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
        bump_room_revision(&mut inner);
        push_log(
            &mut inner,
            LogLevel::Info,
            "room",
            format!("room '{room_name}' ({room_id}) created"),
        );
        push_low_interval_warning(&mut inner, &room_for_warning);
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

        let mut updated_room = apply_room_update(current_room, update);
        ensure_room_viewer_token(&mut updated_room);
        validate_room(&updated_room)?;
        let room_for_warning = updated_room.clone();
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
        bump_room_revision(&mut inner);
        push_log(
            &mut inner,
            LogLevel::Info,
            "room",
            format!("room '{}' updated", room_id),
        );
        push_low_interval_warning(&mut inner, &room_for_warning);
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
        bump_room_revision(&mut inner);
        push_log(
            &mut inner,
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
        runtime.state = image_server_model::RoomState::Paused;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        bump_room_revision(&mut inner);
        push_log(
            &mut inner,
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
        runtime.state = image_server_model::RoomState::Running;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        bump_room_revision(&mut inner);
        push_log(
            &mut inner,
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
        runtime.last_error = Some(message.clone());
        runtime.state = image_server_model::RoomState::Error;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner,
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
        runtime.last_error = None;
        runtime.state = image_server_model::RoomState::Running;

        let view = room_view(runtime, Utc::now(), stale_timeout);
        push_log(
            &mut inner,
            LogLevel::Info,
            "room",
            format!("room '{}' error cleared", room_id),
        );
        Ok(view)
    }
}

fn apply_room_update(mut room: RoomRecord, update: UpdateRoomCommand) -> RoomRecord {
    if let Some(name) = update.name {
        room.name = name;
    }
    if let Some(detection_enabled) = update.detection_enabled {
        room.detection_enabled = detection_enabled;
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
