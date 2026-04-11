use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use image_server_core::ServerCore;
use image_server_model::RoomState;
use image_server_store::{DetectionMode, RoomRecord};
use tokio::{
    sync::broadcast,
    task::JoinHandle,
    time::{interval_at, sleep, Instant},
};

use crate::{error::WatcherError, runtime::WatcherEvent, trigger::WatchTrigger, util::stabilize};

pub(crate) fn spawn_room_watcher(
    core: ServerCore,
    events_tx: broadcast::Sender<WatcherEvent>,
    room: RoomRecord,
    target_path: PathBuf,
) -> Result<JoinHandle<()>, WatcherError> {
    match room.mode {
        DetectionMode::Watch => {
            let watch = WatchTrigger::new(target_path.clone())?;
            Ok(tokio::spawn(async move {
                run_watch_loop(core, events_tx, room, target_path, watch).await;
            }))
        }
        DetectionMode::Interval => Ok(tokio::spawn(async move {
            run_interval_loop(core, events_tx, room, target_path).await;
        })),
    }
}

async fn run_interval_loop(
    core: ServerCore,
    events_tx: broadcast::Sender<WatcherEvent>,
    room: RoomRecord,
    target_path: PathBuf,
) {
    let interval = Duration::from_millis(room.interval_ms.max(1));
    let mut ticker = interval_at(Instant::now() + interval, interval);

    if process_room_trigger(&core, &events_tx, &room, &target_path)
        .await
        .is_stop()
    {
        return;
    }

    loop {
        ticker.tick().await;
        if process_room_trigger(&core, &events_tx, &room, &target_path)
            .await
            .is_stop()
        {
            return;
        }
    }
}

async fn run_watch_loop(
    core: ServerCore,
    events_tx: broadcast::Sender<WatcherEvent>,
    room: RoomRecord,
    target_path: PathBuf,
    mut watch: WatchTrigger,
) {
    if process_room_trigger(&core, &events_tx, &room, &target_path)
        .await
        .is_stop()
    {
        return;
    }

    loop {
        let next = watch.next().await;
        match next {
            Ok(()) => {
                if room.debounce_ms > 0 {
                    sleep(Duration::from_millis(room.debounce_ms)).await;
                    let _ = watch.drain_pending();
                }
            }
            Err(error) => {
                if note_room_error(&core, &room.id, &error).is_stop() {
                    return;
                }
                continue;
            }
        }

        if process_room_trigger(&core, &events_tx, &room, &target_path)
            .await
            .is_stop()
        {
            return;
        }
    }
}

async fn process_room_trigger(
    core: &ServerCore,
    events_tx: &broadcast::Sender<WatcherEvent>,
    room: &RoomRecord,
    target_path: &Path,
) -> LoopControl {
    let Some(_) = core.room(&room.id) else {
        return LoopControl::Stop;
    };
    if let Err(error) = stabilize(target_path, room.stabilize_ms).await {
        return note_room_error(core, &room.id, &error);
    }

    let _ = events_tx.send(WatcherEvent {
        room_id: room.id.clone(),
    });
    LoopControl::Continue
}

fn note_room_error(core: &ServerCore, room_id: &str, error: &WatcherError) -> LoopControl {
    let Some(room) = core.room(room_id) else {
        return LoopControl::Stop;
    };

    let message = error.to_string();
    if room.state != RoomState::Error || room.last_error.as_deref() != Some(message.as_str()) {
        let _ = core.set_room_error(room_id, message);
    }

    LoopControl::Continue
}

enum LoopControl {
    Continue,
    Stop,
}

impl LoopControl {
    fn is_stop(&self) -> bool {
        matches!(self, Self::Stop)
    }
}
