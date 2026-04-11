use chrono::Utc;
use image_server_model::{RoomDto, ServerStatusDto};

use crate::{
    error::CoreError,
    projection::{room_view, stale_timeout},
    runtime::SnapshotBuffer,
    server::ServerCore,
};

impl ServerCore {
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
