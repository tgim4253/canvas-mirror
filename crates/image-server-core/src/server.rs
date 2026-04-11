use std::{io, sync::Arc};

use image_server_config::ServerConfig;
use image_server_model::{LogEntryDto, LogLevel};
use image_server_store::{RoomStore, StoreError};
use indexmap::IndexMap;
use parking_lot::RwLock;

use crate::{
    error::CoreError,
    runtime::{RoomRuntime, ServerCoreInner},
};

#[derive(Debug, Clone)]
pub struct ServerCore {
    pub(crate) inner: Arc<RwLock<ServerCoreInner>>,
}

impl ServerCore {
    pub fn load(config: ServerConfig) -> Result<Self, CoreError> {
        let store = match RoomStore::load_from_path(&config.store_path) {
            Ok(store) => store,
            Err(StoreError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                RoomStore::default()
            }
            Err(error) => return Err(CoreError::Store(error)),
        };

        let rooms = store
            .rooms()
            .iter()
            .cloned()
            .map(|room| (room.id.clone(), RoomRuntime::new(room)))
            .collect::<IndexMap<_, _>>();

        let mut logs = Vec::new();
        push_log(
            &mut logs,
            LogLevel::Info,
            "server",
            "server runtime initialized".to_string(),
        );
        push_log(
            &mut logs,
            LogLevel::Info,
            "store",
            format!("loaded {} room(s) from store", store.rooms().len()),
        );

        Ok(Self {
            inner: Arc::new(RwLock::new(ServerCoreInner {
                config,
                store,
                rooms,
                logs,
            })),
        })
    }

    pub fn config(&self) -> ServerConfig {
        self.inner.read().config.clone()
    }
}

pub(crate) fn push_log(logs: &mut Vec<LogEntryDto>, level: LogLevel, scope: &str, message: String) {
    logs.push(LogEntryDto {
        at: chrono::Utc::now(),
        level,
        scope: scope.to_string(),
        message,
    });
}
