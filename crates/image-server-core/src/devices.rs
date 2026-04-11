use chrono::Utc;
use image_server_model::{LogLevel, RoomDeviceDto};

use crate::{
    commands::JoinRoomCommand,
    error::CoreError,
    projection::{room_device, stale_timeout},
    runtime::RoomDeviceRuntime,
    server::{push_log, ServerCore},
};

impl ServerCore {
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
            state: image_server_model::DeviceState::Offline,
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
}
