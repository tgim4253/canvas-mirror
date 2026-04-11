use chrono::{DateTime, Duration as ChronoDuration, Utc};
use image_server_model::{DeviceState, RoomDeviceDto, RoomDto, RoomState, RoomSummaryDto};

use crate::runtime::{RoomDeviceRuntime, RoomRuntime};

pub(crate) fn room_view(
    runtime: &RoomRuntime,
    now: DateTime<Utc>,
    stale_timeout: ChronoDuration,
) -> RoomDto {
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

pub(crate) fn room_device(
    device: &RoomDeviceRuntime,
    room_state: RoomState,
    now: DateTime<Utc>,
    stale_timeout: ChronoDuration,
) -> RoomDeviceDto {
    RoomDeviceDto {
        id: device.id.clone(),
        name: device.name.clone(),
        platform: device.platform.clone(),
        screen_width: device.screen_width,
        screen_height: device.screen_height,
        state: device_state(room_state, device.last_seen_at, now, stale_timeout),
        last_seen_at: device.last_seen_at,
    }
}

pub(crate) fn device_state(
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

pub(crate) fn stale_timeout(stale_timeout_ms: u64) -> ChronoDuration {
    let capped = stale_timeout_ms.min(i64::MAX as u64) as i64;
    ChronoDuration::milliseconds(capped)
}
