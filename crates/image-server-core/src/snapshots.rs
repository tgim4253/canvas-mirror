use std::sync::Arc;

use chrono::Utc;
use image_server_model::{LogLevel, RoomState, SnapshotMetaDto};

use crate::{
    commands::PublishSnapshotCommand,
    error::CoreError,
    runtime::{SnapshotBuffer, SnapshotPublishedEvent},
    server::{push_log, ServerCore},
};

impl ServerCore {
    pub fn publish_snapshot(
        &self,
        room_id: &str,
        snapshot: PublishSnapshotCommand,
    ) -> Result<SnapshotMetaDto, CoreError> {
        let mut inner = self.inner.write();
        let runtime = inner
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| CoreError::RoomNotFound {
                room_id: room_id.to_string(),
            })?;
        if runtime.state == RoomState::Paused {
            return Err(CoreError::RoomPaused {
                room_id: room_id.to_string(),
            });
        }

        let created_at = Utc::now();
        let bytes_len = snapshot.bytes.len();
        let meta = SnapshotMetaDto {
            room_id: room_id.to_string(),
            content_hash: snapshot.content_hash,
            mime_type: snapshot
                .mime_type
                .unwrap_or_else(|| "image/png".to_string()),
            bytes_len,
            width: snapshot.width,
            height: snapshot.height,
            created_at,
        };

        runtime.latest_snapshot = Some(SnapshotBuffer {
            meta: meta.clone(),
            bytes: Arc::from(snapshot.bytes.into_boxed_slice()),
        });

        push_log(
            &mut inner,
            LogLevel::Info,
            "snapshot",
            format!("snapshot published for room '{}'", room_id),
        );
        let _ = inner.snapshot_events_tx.send(SnapshotPublishedEvent {
            room_id: room_id.to_string(),
            content_hash: meta.content_hash.clone(),
        });
        Ok(meta)
    }
}
