use std::{collections::HashMap, sync::RwLock};

use image_server_core::{RoomChangeEvent, ServerCore};
use image_server_model::RoomState;
use image_server_store::RoomRecord;
use tokio::{
    sync::{broadcast, watch},
    task::JoinHandle,
};

use crate::{
    error::WatcherError,
    runtime::{WatcherEvent, WatcherRuntime},
    task::spawn_room_watcher,
    util::resolve_target_path,
};

#[derive(Clone)]
pub struct WatcherService {
    core: ServerCore,
}

impl WatcherService {
    pub fn new(core: ServerCore) -> Self {
        Self { core }
    }

    pub async fn start(&self) -> Result<WatcherRuntime, WatcherError> {
        let state = std::sync::Arc::new(RwLock::new(Vec::new()));
        let (events_tx, _) = broadcast::channel(64);
        let room_changes_rx = self.core.subscribe_room_changes();
        let mut manager = WatcherManager::new(self.core.clone(), state.clone(), events_tx.clone());
        manager.reconcile().await;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let supervisor = tokio::spawn(async move {
            manager.run(shutdown_rx, room_changes_rx).await;
        });

        Ok(WatcherRuntime::new(
            shutdown_tx,
            supervisor,
            state,
            events_tx,
        ))
    }
}

struct WatcherManager {
    core: ServerCore,
    state: std::sync::Arc<RwLock<Vec<String>>>,
    events_tx: broadcast::Sender<WatcherEvent>,
    active: HashMap<String, ActiveWatcher>,
}

struct ActiveWatcher {
    room: RoomRecord,
    handle: JoinHandle<()>,
}

impl WatcherManager {
    fn new(
        core: ServerCore,
        state: std::sync::Arc<RwLock<Vec<String>>>,
        events_tx: broadcast::Sender<WatcherEvent>,
    ) -> Self {
        Self {
            core,
            state,
            events_tx,
            active: HashMap::new(),
        }
    }

    async fn run(
        mut self,
        mut shutdown_rx: watch::Receiver<bool>,
        mut room_changes_rx: broadcast::Receiver<RoomChangeEvent>,
    ) {
        self.reconcile().await;

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                result = room_changes_rx.recv() => match result {
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => self.reconcile().await,
                    Err(broadcast::error::RecvError::Closed) => break,
                },
            }
        }

        self.abort_all().await;
    }

    async fn reconcile(&mut self) {
        self.collect_finished_watchers();
        let desired: HashMap<String, RoomRecord> = self
            .core
            .room_records()
            .into_iter()
            .map(|room| (room.id.clone(), room))
            .collect();

        let removed_ids: Vec<String> = self
            .active
            .keys()
            .filter(|room_id| !desired.contains_key(*room_id))
            .cloned()
            .collect();
        for room_id in removed_ids {
            self.abort_room(&room_id).await;
        }

        for room in desired.values() {
            let is_paused = self
                .core
                .room(&room.id)
                .map(|current| current.state == RoomState::Paused)
                .unwrap_or(false);

            if !room.detection_enabled || is_paused {
                self.abort_room(&room.id).await;
                continue;
            }

            let needs_restart = self
                .active
                .get(&room.id)
                .map(|watcher| watcher.room != *room || watcher.handle.is_finished())
                .unwrap_or(true);

            if !needs_restart {
                continue;
            }

            self.abort_room(&room.id).await;
            self.try_spawn(room.clone()).await;
        }

        self.publish_state();
    }

    async fn try_spawn(&mut self, room: RoomRecord) {
        let config = self.core.config();
        let target_path = resolve_target_path(&config.store_path, &room.target_path);

        match spawn_room_watcher(
            self.core.clone(),
            self.events_tx.clone(),
            room.clone(),
            target_path,
        ) {
            Ok(handle) => {
                self.active.insert(
                    room.id.clone(),
                    ActiveWatcher {
                        room: room.clone(),
                        handle,
                    },
                );
            }
            Err(error) => {
                let message = error.to_string();
                let should_report = self
                    .core
                    .room(&room.id)
                    .map(|current| {
                        current.state != image_server_model::RoomState::Error
                            || current.last_error.as_deref() != Some(message.as_str())
                    })
                    .unwrap_or(false);
                if should_report {
                    let _ = self.core.set_room_error(&room.id, message);
                }
            }
        }
    }

    fn collect_finished_watchers(&mut self) {
        let finished: Vec<String> = self
            .active
            .iter()
            .filter_map(|(room_id, watcher)| watcher.handle.is_finished().then(|| room_id.clone()))
            .collect();

        for room_id in finished {
            self.active.remove(&room_id);
        }
    }

    async fn abort_room(&mut self, room_id: &str) {
        if let Some(mut watcher) = self.active.remove(room_id) {
            watcher.handle.abort();
            let _ = (&mut watcher.handle).await;
        }
    }

    async fn abort_all(&mut self) {
        let room_ids: Vec<String> = self.active.keys().cloned().collect();
        for room_id in room_ids {
            self.abort_room(&room_id).await;
        }
        self.publish_state();
    }

    fn publish_state(&self) {
        let room_ids = self.active.keys().cloned().collect();
        *self.state.write().expect("watcher state lock poisoned") = room_ids;
    }
}
