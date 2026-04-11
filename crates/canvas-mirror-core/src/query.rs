use canvas_mirror_model::{LogEntryDto, RoomDto, ServerStatusDto};
use canvas_mirror_store::RoomRecord;
use chrono::Utc;
use tokio::sync::broadcast;

use crate::{
    error::CoreError,
    projection::{room_view, stale_timeout},
    runtime::{RoomChangeEvent, SnapshotBuffer, SnapshotPublishedEvent},
    server::ServerCore,
};

impl ServerCore {
    pub fn room_record(&self, room_id: &str) -> Option<RoomRecord> {
        self.inner
            .read()
            .rooms
            .get(room_id)
            .map(|room| room.room.clone())
    }

    pub fn room_records(&self) -> Vec<RoomRecord> {
        self.room_records_with_revision().1
    }

    pub fn room_revision(&self) -> u64 {
        self.inner.read().room_revision
    }

    pub fn subscribe_room_changes(&self) -> broadcast::Receiver<RoomChangeEvent> {
        self.inner.read().room_events_tx.subscribe()
    }

    pub fn subscribe_snapshot_events(&self) -> broadcast::Receiver<SnapshotPublishedEvent> {
        self.inner.read().snapshot_events_tx.subscribe()
    }

    pub fn room_records_with_revision(&self) -> (u64, Vec<RoomRecord>) {
        let inner = self.inner.read();
        let rooms = inner.rooms.values().map(|room| room.room.clone()).collect();
        (inner.room_revision, rooms)
    }

    pub fn log_cursor(&self) -> u64 {
        let inner = self.inner.read();
        inner.log_cursor_start + inner.logs.len() as u64
    }

    pub fn logs_since(&self, cursor: u64) -> Vec<LogEntryDto> {
        self.read_logs_since(cursor).1
    }

    pub fn read_logs_since(&self, cursor: u64) -> (u64, Vec<LogEntryDto>) {
        let inner = self.inner.read();
        let start = cursor.max(inner.log_cursor_start);
        let skip = (start - inner.log_cursor_start) as usize;
        let next_cursor = inner.log_cursor_start + inner.logs.len() as u64;
        let logs = inner.logs.iter().skip(skip).cloned().collect();
        (next_cursor, logs)
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
            logs: inner.logs.iter().cloned().collect(),
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
