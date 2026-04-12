use std::{
    fs,
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use canvas_mirror_config::ServerConfig;
use canvas_mirror_core::{ServerCore, SnapshotBuffer, UpdateRoomCommand};
use canvas_mirror_host::{
    generate_room_qr_svg, load_viewer_html, preferred_viewer_url, process_room_event,
    room_viewer_urls, spawn_snapshot_pipeline, start_transport_server, viewer_public_urls,
    RuntimeLogFile, TransportRuntime,
};
use canvas_mirror_icc::list_display_icc_profiles;
use canvas_mirror_model::{LogEntryDto, RoomDto, ServerStatusDto, SnapshotMetaDto};
use canvas_mirror_preview::PreviewGenerator;
use canvas_mirror_store::{DetectionMode, OutputResolution, RoomRecord, StoredIccProfile};
use canvas_mirror_watcher::{WatcherRuntime, WatcherService};
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::JoinHandle;
use url::Url;
use uuid::Uuid;

const CONFIG_FILE_NAME: &str = "canvas-mirror-config.toml";
const STORE_FILE_NAME: &str = "canvas-mirror-store.toml";
pub const STUDIO_ROOMS_CHANGED_EVENT: &str = "studio://rooms-changed";
pub const STUDIO_ROOM_PREVIEWS_CHANGED_EVENT: &str = "studio://room-previews-changed";
pub const STUDIO_RUNTIME_LOGS_CHANGED_EVENT: &str = "studio://runtime-logs-changed";

pub struct AppState {
    runtime: StudioRuntime,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self> {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .context("failed to resolve app data directory")?;
        let config_path = app_data_dir.join(CONFIG_FILE_NAME);
        let log_file = RuntimeLogFile::create(app_data_dir.join("logs"), "canvas-mirror-studio")
            .with_context(|| {
                format!(
                    "failed to create runtime log file under {}",
                    app_data_dir.join("logs").display()
                )
            })?;
        let _ = log_file.append_line(
            "info",
            "studio",
            format!(
                "starting studio runtime with config {}",
                config_path.display()
            ),
        );
        let runtime =
            StudioRuntime::load_from_config_path(&config_path, Some(app.clone()), log_file)?;

        Ok(Self { runtime })
    }

    pub fn runtime(&self) -> &StudioRuntime {
        &self.runtime
    }
}

pub struct StudioRuntime {
    core: ServerCore,
    config_path: PathBuf,
    log_file: RuntimeLogFile,
    app: Option<AppHandle>,
    background: Mutex<RuntimeBackground>,
}

struct RuntimeBackground {
    transport: Option<TransportRuntime>,
    _watchers: WatcherRuntime,
    snapshot_pipeline: JoinHandle<()>,
    event_bridge: JoinHandle<()>,
    log_state: Arc<Mutex<RuntimeLogState>>,
}

impl Drop for RuntimeBackground {
    fn drop(&mut self) {
        self.snapshot_pipeline.abort();
        self.event_bridge.abort();
    }
}

struct RuntimeLogState {
    file_cursor: u64,
    event_cursor: u64,
}

impl RuntimeLogState {
    fn new(initial_cursor: u64) -> Self {
        Self {
            file_cursor: initial_cursor,
            event_cursor: initial_cursor,
        }
    }
}

impl StudioRuntime {
    pub fn load_from_config_path(
        config_path: &Path,
        app: Option<AppHandle>,
        log_file: RuntimeLogFile,
    ) -> Result<Self> {
        let config = ensure_config_exists(config_path)?;
        let core = ServerCore::load(config).context("failed to initialize server core")?;
        let initial_log_cursor = persist_runtime_logs(&core, &log_file, 0)?;
        let background = tauri::async_runtime::block_on(Self::start_background(
            core.clone(),
            app.clone(),
            log_file.clone(),
            initial_log_cursor,
        ))
        .context("failed to start studio background runtime")?;

        Ok(Self {
            core,
            config_path: config_path.to_path_buf(),
            log_file,
            app,
            background: Mutex::new(background),
        })
    }

    pub fn list_rooms(&self) -> Result<Vec<ManagedRoomDto>> {
        self.core
            .room_records()
            .into_iter()
            .map(|room_record| self.managed_room_from_record(room_record))
            .collect()
    }

    pub fn room_icc_profile(&self, room_id: &str) -> Result<Option<StoredIccProfile>> {
        Ok(self
            .core
            .room_record(room_id)
            .and_then(|room| room.icc_profile))
    }

    pub fn create_room(&self, input: CreateRoomInput) -> Result<ManagedRoomDto> {
        let room_id = format!("room-{}", Uuid::new_v4().simple());
        let room = RoomRecord {
            id: room_id.clone(),
            name: input.name,
            viewer_token: String::new(),
            detection_enabled: input.detection_enabled,
            target_path: PathBuf::from(input.target_path),
            mode: input.mode,
            interval_ms: input.interval_ms,
            debounce_ms: input.debounce_ms,
            stabilize_ms: input.stabilize_ms,
            resolution: input.resolution,
            icc_profile_enabled: input.icc_profile_enabled,
            icc_profile: input.icc_profile,
        };

        self.core
            .create_room(room)
            .context("failed to create room")?;
        self.refresh_room_preview(&room_id);

        self.managed_room("failed to load created room", &room_id)
    }

    pub fn update_room(&self, room_id: &str, input: UpdateRoomInput) -> Result<ManagedRoomDto> {
        let command = UpdateRoomCommand {
            name: Some(input.name),
            detection_enabled: Some(input.detection_enabled),
            target_path: Some(PathBuf::from(input.target_path)),
            mode: Some(input.mode),
            interval_ms: Some(input.interval_ms),
            debounce_ms: Some(input.debounce_ms),
            stabilize_ms: Some(input.stabilize_ms),
            resolution: Some(input.resolution),
            icc_profile_enabled: Some(input.icc_profile_enabled),
            icc_profile: input.icc_profile,
        };

        self.core
            .update_room(room_id, command)
            .with_context(|| format!("failed to update room '{room_id}'"))?;
        self.refresh_room_preview(room_id);

        self.managed_room("failed to load updated room", room_id)
    }

    pub fn delete_room(&self, room_id: &str) -> Result<ManagedRoomDto> {
        let room = self
            .managed_room("failed to load room before delete", room_id)
            .with_context(|| format!("failed to delete room '{room_id}'"))?;

        self.core
            .delete_room(room_id)
            .with_context(|| format!("failed to delete room '{room_id}'"))?;

        Ok(room)
    }

    pub fn set_room_running(&self, room_id: &str, running: bool) -> Result<ManagedRoomDto> {
        if running {
            self.core
                .resume_room(room_id)
                .with_context(|| format!("failed to resume room '{room_id}'"))?;
            self.refresh_room_preview(room_id);
        } else {
            self.core
                .pause_room(room_id)
                .with_context(|| format!("failed to pause room '{room_id}'"))?;
        }

        self.managed_room("failed to load updated running state", room_id)
    }

    pub fn server_status(&self) -> ServerStatusDto {
        self.core.status()
    }

    pub fn server_settings(&self) -> ServerSettingsDto {
        server_settings_from_config(&self.core.config())
    }

    pub fn update_server_settings(
        &self,
        input: UpdateServerSettingsInput,
    ) -> Result<ServerSettingsDto> {
        let current_config = self.core.config();
        let next_config = apply_server_settings_input(&current_config, input)?;
        let transport_restart_required = transport_restart_required(&current_config, &next_config);
        let previous_transport = if transport_restart_required {
            Some(
                self.background
                    .lock()
                    .transport
                    .take()
                    .context("transport runtime missing before settings update")?,
            )
        } else {
            None
        };

        let mut next_transport = if let Some(previous_transport) = previous_transport {
            tauri::async_runtime::block_on(previous_transport.shutdown());

            let started_transport = match tauri::async_runtime::block_on(Self::start_transport(
                self.core.clone(),
                &next_config,
            )) {
                Ok(transport) => transport,
                Err(error) => {
                    let restored_transport = tauri::async_runtime::block_on(Self::start_transport(
                        self.core.clone(),
                        &current_config,
                    ))
                    .context("failed to restore previous transport after settings update error")?;
                    self.background.lock().transport = Some(restored_transport);
                    return Err(error);
                }
            };

            Some(started_transport)
        } else {
            None
        };

        if let Err(error) = next_config
            .save_to_path(&self.config_path)
            .with_context(|| format!("failed to save config to {}", self.config_path.display()))
        {
            if let Some(started_transport) = next_transport.take() {
                tauri::async_runtime::block_on(started_transport.shutdown());
                let restored_transport = tauri::async_runtime::block_on(Self::start_transport(
                    self.core.clone(),
                    &current_config,
                ))
                .context("failed to restore previous transport after config save error")?;
                self.background.lock().transport = Some(restored_transport);
            }

            return Err(error);
        }

        self.core.update_config(next_config.clone());

        if let Some(transport) = next_transport.take() {
            self.background.lock().transport = Some(transport);
        }

        self.emit_runtime_events_if_available()?;

        Ok(server_settings_from_config(&next_config))
    }

    async fn start_background(
        core: ServerCore,
        app: Option<AppHandle>,
        log_file: RuntimeLogFile,
        initial_log_cursor: u64,
    ) -> Result<RuntimeBackground> {
        let config = core.config();
        let log_state = Arc::new(Mutex::new(RuntimeLogState::new(initial_log_cursor)));
        let transport = Self::start_transport(core.clone(), &config).await?;
        let watchers = WatcherService::new(core.clone())
            .start()
            .await
            .context("failed to start room watchers")?;
        let snapshot_pipeline = spawn_snapshot_pipeline(
            core.clone(),
            core.config().store_path.clone(),
            watchers.subscribe(),
        );
        let event_bridge = if let Some(app) = app {
            Self::spawn_event_bridge(core.clone(), app, log_state.clone(), log_file)
        } else {
            tokio::spawn(async {})
        };

        Ok(RuntimeBackground {
            transport: Some(transport),
            _watchers: watchers,
            snapshot_pipeline,
            event_bridge,
            log_state,
        })
    }

    async fn start_transport(core: ServerCore, config: &ServerConfig) -> Result<TransportRuntime> {
        let viewer_urls = viewer_public_urls(config);
        let qr_viewer_url = preferred_viewer_url(&viewer_urls)
            .context("failed to derive a viewer URL for QR generation")?;
        let viewer_html =
            load_viewer_html(config).context("failed to load viewer HTML for transport server")?;

        start_transport_server(core, config.bind_addr, viewer_html, qr_viewer_url)
            .await
            .with_context(|| {
                format!(
                    "failed to start websocket transport on {}",
                    config.bind_addr
                )
            })
    }

    fn spawn_event_bridge(
        core: ServerCore,
        app: AppHandle,
        log_state: Arc<Mutex<RuntimeLogState>>,
        log_file: RuntimeLogFile,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut room_changes = core.subscribe_room_changes();
            let mut snapshot_changes = core.subscribe_snapshot_events();

            loop {
                tokio::select! {
                    room_change = room_changes.recv() => {
                        match room_change {
                            Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                if let Err(error) = emit_rooms_changed_event(&app, &core)
                                    .and_then(|_| emit_runtime_logs_delta_event(&app, &core, &log_state, &log_file))
                                {
                                    log_bridge_warning(&log_file, format!("failed to emit studio room update events: {error}"));
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                    snapshot_change = snapshot_changes.recv() => {
                        match snapshot_change {
                            Ok(event) => {
                                if let Err(error) = emit_room_preview_event(&app, &core, &event.room_id)
                                    .and_then(|_| emit_runtime_logs_delta_event(&app, &core, &log_state, &log_file))
                                {
                                    log_bridge_warning(&log_file, format!("failed to emit studio preview update events: {error}"));
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                                if let Err(error) = emit_all_room_preview_events(&app, &core)
                                    .and_then(|_| emit_runtime_logs_delta_event(&app, &core, &log_state, &log_file))
                                {
                                    log_bridge_warning(&log_file, format!("failed to emit lagged studio preview events: {error}"));
                                }
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                }
            }
        })
    }

    fn refresh_room_preview(&self, room_id: &str) {
        let store_path = self.core.config().store_path;
        tauri::async_runtime::block_on(process_room_event(
            &self.core,
            &store_path,
            &PreviewGenerator,
            room_id,
        ));
    }

    fn managed_room(&self, error_context: &str, room_id: &str) -> Result<ManagedRoomDto> {
        let room_record = self
            .core
            .room_record(room_id)
            .with_context(|| format!("{error_context}: missing room record"))?;

        self.managed_room_from_record(room_record)
    }

    fn managed_room_from_record(&self, room_record: RoomRecord) -> Result<ManagedRoomDto> {
        managed_room_from_record(&self.core, room_record)
    }

    fn emit_runtime_events_if_available(&self) -> Result<()> {
        let Some(app) = &self.app else {
            return Ok(());
        };
        let log_state = self.background.lock().log_state.clone();

        emit_rooms_changed_event(app, &self.core)?;
        emit_runtime_logs_delta_event(app, &self.core, &log_state, &self.log_file)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedRoomViewerLinkDto {
    pub viewer_url: String,
    pub qr_svg: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ManagedRoomDto {
    pub room: RoomDto,
    pub target_path: String,
    pub viewer_links: Vec<ManagedRoomViewerLinkDto>,
    pub preview_data_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RoomPreviewDto {
    pub room_id: String,
    pub preview_data_url: Option<String>,
    pub latest_snapshot: Option<SnapshotMetaDto>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLogsChangedDto {
    pub generated_at: DateTime<Utc>,
    pub logs: Vec<LogEntryDto>,
    pub replace: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AvailableIccProfileDto {
    pub id: String,
    pub display_name: String,
    pub is_primary: bool,
    pub profile: StoredIccProfile,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerSettingsDto {
    pub bind_host: String,
    pub bind_port: u16,
    pub public_url: Option<String>,
    pub stale_timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateRoomInput {
    pub name: String,
    pub detection_enabled: bool,
    pub target_path: String,
    pub mode: DetectionMode,
    pub interval_ms: u64,
    pub debounce_ms: u64,
    pub stabilize_ms: u64,
    pub resolution: OutputResolution,
    #[serde(default)]
    pub icc_profile_enabled: bool,
    #[serde(default)]
    pub icc_profile: Option<StoredIccProfile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateRoomInput {
    pub name: String,
    pub detection_enabled: bool,
    pub target_path: String,
    pub mode: DetectionMode,
    pub interval_ms: u64,
    pub debounce_ms: u64,
    pub stabilize_ms: u64,
    pub resolution: OutputResolution,
    #[serde(default)]
    pub icc_profile_enabled: bool,
    #[serde(default)]
    pub icc_profile: Option<Option<StoredIccProfile>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateServerSettingsInput {
    pub bind_host: String,
    pub bind_port: u16,
    pub public_url: Option<String>,
    pub stale_timeout_ms: u64,
}

pub fn load_icc_profile_from_path(path: impl AsRef<Path>) -> Result<StoredIccProfile> {
    let path = path.as_ref();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read ICC profile from {}", path.display()))?;

    if bytes.is_empty() {
        anyhow::bail!("ICC profile file is empty.");
    }

    let name = path
        .file_stem()
        .or_else(|| path.file_name())
        .and_then(|component| component.to_str())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or("Imported ICC Profile")
        .to_string();

    Ok(StoredIccProfile { name, bytes })
}

pub fn list_available_icc_profiles_for_app() -> Vec<AvailableIccProfileDto> {
    list_display_icc_profiles()
        .unwrap_or_default()
        .into_iter()
        .map(|profile| AvailableIccProfileDto {
            id: profile.display_id,
            display_name: profile.display_name.clone(),
            is_primary: profile.is_primary,
            profile: StoredIccProfile {
                name: profile.display_name,
                bytes: profile.icc_profile,
            },
        })
        .collect()
}

fn ensure_config_exists(config_path: &Path) -> Result<ServerConfig> {
    if config_path.exists() {
        return ServerConfig::load_from_path_resolved(config_path)
            .with_context(|| format!("failed to load config from {}", config_path.display()));
    }

    let config_dir = config_path
        .parent()
        .with_context(|| format!("invalid config path: {}", config_path.display()))?;
    fs::create_dir_all(config_dir)
        .with_context(|| format!("failed to create {}", config_dir.display()))?;

    let config = ServerConfig {
        bind_addr: SocketAddr::from(([0, 0, 0, 0], 8787)),
        store_path: config_dir.join(STORE_FILE_NAME),
        ..ServerConfig::default()
    };
    config.save_to_path(config_path).with_context(|| {
        format!(
            "failed to write default config to {}",
            config_path.display()
        )
    })?;

    Ok(config)
}

fn server_settings_from_config(config: &ServerConfig) -> ServerSettingsDto {
    ServerSettingsDto {
        bind_host: config.bind_addr.ip().to_string(),
        bind_port: config.bind_addr.port(),
        public_url: config.public_url.as_ref().map(ToString::to_string),
        stale_timeout_ms: config.stale_timeout_ms,
    }
}

fn apply_server_settings_input(
    current_config: &ServerConfig,
    input: UpdateServerSettingsInput,
) -> Result<ServerConfig> {
    let bind_host = input.bind_host.trim();
    if bind_host.is_empty() {
        anyhow::bail!("Bind host is required.");
    }

    if input.bind_port == 0 {
        anyhow::bail!("Port must be between 1 and 65535.");
    }

    let bind_ip = bind_host
        .parse::<IpAddr>()
        .with_context(|| format!("invalid bind host '{bind_host}'"))?;
    let public_url = parse_optional_public_url(input.public_url.as_deref())?;

    Ok(ServerConfig {
        bind_addr: SocketAddr::new(bind_ip, input.bind_port),
        public_url,
        stale_timeout_ms: input.stale_timeout_ms,
        ..current_config.clone()
    })
}

fn transport_restart_required(current_config: &ServerConfig, next_config: &ServerConfig) -> bool {
    current_config.bind_addr != next_config.bind_addr
        || current_config.public_url != next_config.public_url
        || current_config.viewer_path != next_config.viewer_path
}

fn parse_optional_public_url(value: Option<&str>) -> Result<Option<Url>> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };

    let url = Url::parse(value).with_context(|| format!("invalid public URL '{value}'"))?;
    match url.scheme() {
        "http" | "https" => Ok(Some(url)),
        scheme => anyhow::bail!("unsupported public URL scheme '{scheme}'"),
    }
}

fn emit_rooms_changed_event(app: &AppHandle, core: &ServerCore) -> Result<()> {
    let rooms = core
        .room_records()
        .into_iter()
        .map(|room_record| managed_room_from_record(core, room_record))
        .collect::<Result<Vec<_>>>()?;

    app.emit(STUDIO_ROOMS_CHANGED_EVENT, &rooms)
        .context("failed to emit room update event")
}

fn emit_room_preview_event(app: &AppHandle, core: &ServerCore, room_id: &str) -> Result<()> {
    let Some(preview) = room_preview_from_room_id(core, room_id)? else {
        return Ok(());
    };

    app.emit(STUDIO_ROOM_PREVIEWS_CHANGED_EVENT, vec![preview])
        .context("failed to emit room preview update event")
}

fn emit_all_room_preview_events(app: &AppHandle, core: &ServerCore) -> Result<()> {
    let previews = core
        .room_records()
        .into_iter()
        .map(|room_record| room_preview_from_room_id(core, &room_record.id))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();

    if previews.is_empty() {
        return Ok(());
    }

    app.emit(STUDIO_ROOM_PREVIEWS_CHANGED_EVENT, previews)
        .context("failed to emit full room preview update event")
}

fn emit_runtime_logs_delta_event(
    app: &AppHandle,
    core: &ServerCore,
    log_state: &Mutex<RuntimeLogState>,
    log_file: &RuntimeLogFile,
) -> Result<()> {
    let mut log_state = log_state.lock();
    let (next_file_cursor, logs_to_persist) = core.read_logs_since(log_state.file_cursor);
    if !logs_to_persist.is_empty() {
        log_file
            .append_runtime_logs(&logs_to_persist)
            .with_context(|| {
                format!(
                    "failed to append runtime logs to {}",
                    log_file.path().display()
                )
            })?;
        log_state.file_cursor = next_file_cursor;
    }

    let (next_event_cursor, logs_to_emit) = core.read_logs_since(log_state.event_cursor);
    if logs_to_emit.is_empty() {
        return Ok(());
    }

    app.emit(
        STUDIO_RUNTIME_LOGS_CHANGED_EVENT,
        RuntimeLogsChangedDto {
            generated_at: Utc::now(),
            logs: logs_to_emit,
            replace: false,
        },
    )
    .context("failed to emit runtime log delta event")?;

    log_state.event_cursor = next_event_cursor;
    Ok(())
}

fn persist_runtime_logs(
    core: &ServerCore,
    log_file: &RuntimeLogFile,
    start_cursor: u64,
) -> Result<u64> {
    let (next_cursor, logs) = core.read_logs_since(start_cursor);
    if !logs.is_empty() {
        log_file.append_runtime_logs(&logs).with_context(|| {
            format!(
                "failed to append runtime logs to {}",
                log_file.path().display()
            )
        })?;
    }
    Ok(next_cursor)
}

fn log_bridge_warning(log_file: &RuntimeLogFile, message: String) {
    if let Err(error) = log_file.append_line("warn", "tauri", &message) {
        eprintln!(
            "warn: {message} (failed to write to {}: {error})",
            log_file.path().display()
        );
    }
}

fn managed_room_from_record(core: &ServerCore, room_record: RoomRecord) -> Result<ManagedRoomDto> {
    let room_id = room_record.id.clone();
    let room = core
        .room(&room_id)
        .with_context(|| format!("missing runtime view for room '{room_id}'"))?;
    let viewer_links = room_viewer_urls(&viewer_public_urls(&core.config()), &room_record)
        .into_iter()
        .map(|viewer_url| ManagedRoomViewerLinkDto {
            qr_svg: generate_room_qr_svg(
                core,
                &viewer_url,
                &room_record.id,
                &room_record.viewer_token,
            )
            .ok(),
            viewer_url: viewer_url.to_string(),
        })
        .collect();

    Ok(ManagedRoomDto {
        room,
        target_path: room_record.target_path.display().to_string(),
        viewer_links,
        preview_data_url: snapshot_preview_data_url(core, &room_id)
            .with_context(|| format!("failed to build preview URL for room '{room_id}'"))?,
    })
}

fn room_preview_from_room_id(core: &ServerCore, room_id: &str) -> Result<Option<RoomPreviewDto>> {
    let Some(room) = core.room(room_id) else {
        return Ok(None);
    };

    Ok(Some(RoomPreviewDto {
        room_id: room_id.to_string(),
        preview_data_url: snapshot_preview_data_url(core, room_id)?,
        latest_snapshot: room.latest_snapshot,
    }))
}

fn snapshot_preview_data_url(core: &ServerCore, room_id: &str) -> Result<Option<String>> {
    let Some(snapshot) = core
        .snapshot(room_id)
        .with_context(|| format!("failed to read snapshot for room '{room_id}'"))?
    else {
        return Ok(None);
    };

    Ok(Some(snapshot_preview_data_url_from_snapshot(&snapshot)))
}

fn snapshot_preview_data_url_from_snapshot(snapshot: &SnapshotBuffer) -> String {
    let encoded = BASE64_STANDARD.encode(snapshot.bytes.as_ref());
    format!("data:{};base64,{}", snapshot.meta.mime_type, encoded)
}

#[cfg(test)]
mod tests {
    use std::{
        net::TcpListener,
        thread::sleep,
        time::{Duration, Instant},
    };

    use canvas_mirror_model::RoomState;
    use canvas_mirror_store::RoomStore;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn runtime_bootstraps_config_and_supports_room_crud() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        write_test_config(&config_path, reserve_local_port());
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");

        assert!(config_path.exists());
        assert!(runtime.list_rooms().expect("rooms should load").is_empty());
        assert!(!runtime.server_status().logs.is_empty());

        let created = runtime
            .create_room(CreateRoomInput {
                name: "Room A".to_string(),
                detection_enabled: false,
                target_path: "./samples/room-a.clip".to_string(),
                mode: DetectionMode::Interval,
                interval_ms: 2_000,
                debounce_ms: 750,
                stabilize_ms: 300,
                resolution: OutputResolution::Contain {
                    max_width: 1_440,
                    max_height: 900,
                },
                icc_profile_enabled: false,
                icc_profile: None,
            })
            .expect("room should be created");

        let room_id = created.room.room.id.clone();
        assert!(room_id.starts_with("room-"));
        assert_eq!(created.target_path, "./samples/room-a.clip");
        assert!(!created.viewer_links.is_empty());
        assert!(created.viewer_links.iter().any(|viewer_link| {
            viewer_link
                .qr_svg
                .as_deref()
                .unwrap_or_default()
                .contains("<svg")
        }));

        let updated = runtime
            .update_room(
                &room_id,
                UpdateRoomInput {
                    name: "Room A Updated".to_string(),
                    detection_enabled: false,
                    target_path: "./samples/room-a-updated.clip".to_string(),
                    mode: DetectionMode::Watch,
                    interval_ms: 3_000,
                    debounce_ms: 900,
                    stabilize_ms: 500,
                    resolution: OutputResolution::Source,
                    icc_profile_enabled: false,
                    icc_profile: None,
                },
            )
            .expect("room should be updated");

        assert_eq!(updated.room.room.name, "Room A Updated");
        assert_eq!(updated.room.room.mode, DetectionMode::Watch);
        assert!(!updated.room.room.detection_enabled);
        assert_eq!(updated.target_path, "./samples/room-a-updated.clip");

        let paused = runtime
            .set_room_running(&room_id, false)
            .expect("room should pause");
        assert_eq!(paused.room.state, RoomState::Paused);

        let resumed = runtime
            .set_room_running(&room_id, true)
            .expect("room should resume");
        assert_eq!(resumed.room.state, RoomState::Running);

        let deleted = runtime.delete_room(&room_id).expect("room should delete");
        assert_eq!(deleted.room.room.id, room_id);
        assert!(runtime.list_rooms().expect("rooms should load").is_empty());

        let saved_store =
            RoomStore::load_from_path(dir.path().join("studio/config/canvas-mirror-store.toml"))
                .expect("store should load");
        assert!(saved_store.rooms().is_empty());
    }

    #[test]
    fn runtime_starts_snapshot_pipeline_for_new_rooms() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        write_test_config(&config_path, reserve_local_port());
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");
        let sample_png = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png");

        let created = runtime
            .create_room(CreateRoomInput {
                name: "Room A".to_string(),
                detection_enabled: true,
                target_path: sample_png.display().to_string(),
                mode: DetectionMode::Interval,
                interval_ms: 50,
                debounce_ms: 0,
                stabilize_ms: 0,
                resolution: OutputResolution::Source,
                icc_profile_enabled: false,
                icc_profile: None,
            })
            .expect("room should be created");
        let room_id = created.room.room.id.clone();
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let room = runtime.core.room(&room_id).expect("room should exist");
            if room.latest_snapshot.is_some() {
                let managed = runtime
                    .list_rooms()
                    .expect("managed rooms should load")
                    .into_iter()
                    .find(|managed| managed.room.room.id == room_id)
                    .expect("managed room should exist");
                assert_eq!(room.state, RoomState::Running);
                assert!(room.last_error.is_none());
                assert!(managed.preview_data_url.is_some());
                break;
            }

            assert!(
                Instant::now() < deadline,
                "expected snapshot pipeline to publish a preview for '{room_id}'"
            );
            sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn runtime_refreshes_snapshot_when_room_settings_change() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        write_test_config(&config_path, reserve_local_port());
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");
        let sample_png = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("icons/32x32.png");

        let created = runtime
            .create_room(CreateRoomInput {
                name: "Room A".to_string(),
                detection_enabled: true,
                target_path: sample_png.display().to_string(),
                mode: DetectionMode::Watch,
                interval_ms: 2_000,
                debounce_ms: 0,
                stabilize_ms: 0,
                resolution: OutputResolution::Source,
                icc_profile_enabled: false,
                icc_profile: None,
            })
            .expect("room should be created");
        let room_id = created.room.room.id.clone();
        let initial_deadline = Instant::now() + Duration::from_secs(2);

        loop {
            let room = runtime.core.room(&room_id).expect("room should exist");
            let Some(snapshot) = room.latest_snapshot.as_ref() else {
                assert!(
                    Instant::now() < initial_deadline,
                    "expected initial snapshot for '{room_id}'"
                );
                sleep(Duration::from_millis(25));
                continue;
            };

            if snapshot.width == Some(32) && snapshot.height == Some(32) {
                break;
            }

            assert!(
                Instant::now() < initial_deadline,
                "expected initial snapshot dimensions for '{room_id}'"
            );
            sleep(Duration::from_millis(25));
        }

        runtime
            .update_room(
                &room_id,
                UpdateRoomInput {
                    name: "Room A".to_string(),
                    detection_enabled: true,
                    target_path: sample_png.display().to_string(),
                    mode: DetectionMode::Watch,
                    interval_ms: 2_000,
                    debounce_ms: 0,
                    stabilize_ms: 0,
                    resolution: OutputResolution::Contain {
                        max_width: 8,
                        max_height: 8,
                    },
                    icc_profile_enabled: false,
                    icc_profile: None,
                },
            )
            .expect("room should update");

        let update_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let room = runtime.core.room(&room_id).expect("room should exist");
            let Some(snapshot) = room.latest_snapshot.as_ref() else {
                assert!(
                    Instant::now() < update_deadline,
                    "expected updated snapshot for '{room_id}'"
                );
                sleep(Duration::from_millis(25));
                continue;
            };

            if snapshot.width == Some(8) && snapshot.height == Some(8) {
                break;
            }

            assert!(
                Instant::now() < update_deadline,
                "expected updated snapshot dimensions for '{room_id}'"
            );
            sleep(Duration::from_millis(25));
        }
    }

    #[test]
    fn runtime_starts_transport_server_and_serves_viewer_routes() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        let port = reserve_local_port();
        write_test_config(&config_path, port);
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");

        let created = runtime
            .create_room(CreateRoomInput {
                name: "Room A".to_string(),
                detection_enabled: false,
                target_path: "./samples/room-a.clip".to_string(),
                mode: DetectionMode::Interval,
                interval_ms: 2_000,
                debounce_ms: 0,
                stabilize_ms: 0,
                resolution: OutputResolution::Source,
                icc_profile_enabled: false,
                icc_profile: None,
            })
            .expect("room should be created");
        let viewer_url = created
            .viewer_links
            .first()
            .map(|viewer_link| viewer_link.viewer_url.clone())
            .expect("viewer URL should exist");
        let viewer_response = http_get(&viewer_url);

        assert!(
            viewer_response.contains("Canvas Mirror Viewer")
                || viewer_response.contains("id=\"preview\""),
            "unexpected viewer response: {viewer_response}"
        );

        let qr_url = viewer_url.replacen("/?", "/qr.svg?", 1);
        let qr_response = http_get(&qr_url);

        assert!(
            qr_response.contains("<svg"),
            "unexpected QR response: {qr_response}"
        );

        let locale_response = http_get(&format!("http://127.0.0.1:{port}/locales/ko/common.json"));

        assert!(
            locale_response.contains("\"viewer.title\""),
            "unexpected locale response: {locale_response}"
        );
    }

    #[test]
    fn ensure_config_exists_bootstraps_wildcard_bind_for_tauri_app() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");

        let config = ensure_config_exists(&config_path).expect("config should bootstrap");

        assert_eq!(config.bind_addr, SocketAddr::from(([0, 0, 0, 0], 8787)));
        assert!(config_path.exists());
    }

    #[test]
    fn runtime_updates_server_settings_and_persists_them() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        write_test_config(&config_path, reserve_local_port());
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");
        let next_port = reserve_local_port();

        let updated = runtime
            .update_server_settings(UpdateServerSettingsInput {
                bind_host: "127.0.0.1".to_string(),
                bind_port: next_port,
                public_url: Some(format!("http://127.0.0.1:{next_port}")),
                stale_timeout_ms: 45_000,
            })
            .expect("settings should update");

        assert_eq!(updated.bind_host, "127.0.0.1");
        assert_eq!(updated.bind_port, next_port);
        assert_eq!(
            updated.public_url.as_deref(),
            Some(format!("http://127.0.0.1:{next_port}/").as_str())
        );
        assert_eq!(updated.stale_timeout_ms, 45_000);

        let saved =
            ServerConfig::load_from_path_resolved(&config_path).expect("config should reload");
        assert_eq!(
            saved.bind_addr,
            SocketAddr::from(([127, 0, 0, 1], next_port))
        );
        assert_eq!(
            saved.public_url.as_ref().map(ToString::to_string),
            updated.public_url
        );
        assert_eq!(saved.stale_timeout_ms, 45_000);
    }

    #[test]
    fn runtime_restarts_transport_when_public_url_changes() {
        let dir = tempdir().expect("temp dir should exist");
        let config_path = dir.path().join("studio/config/canvas-mirror-config.toml");
        let port = reserve_local_port();
        write_test_config(&config_path, port);
        let runtime = StudioRuntime::load_from_config_path(
            &config_path,
            None,
            create_test_log_file(dir.path()),
        )
        .expect("runtime should load");

        let created = runtime
            .create_room(CreateRoomInput {
                name: "Room A".to_string(),
                detection_enabled: false,
                target_path: "./samples/room-a.clip".to_string(),
                mode: DetectionMode::Interval,
                interval_ms: 2_000,
                debounce_ms: 0,
                stabilize_ms: 0,
                resolution: OutputResolution::Source,
                icc_profile_enabled: false,
                icc_profile: None,
            })
            .expect("room should be created");

        let qr_url = created
            .viewer_links
            .first()
            .map(|viewer_link| viewer_link.viewer_url.clone())
            .expect("viewer URL should exist")
            .replacen("/?", "/qr.svg?", 1);
        let initial_qr = http_get(&qr_url);

        runtime
            .update_server_settings(UpdateServerSettingsInput {
                bind_host: "127.0.0.1".to_string(),
                bind_port: port,
                public_url: Some("https://viewer.example.com/canvas-mirror/".to_string()),
                stale_timeout_ms: 30_000,
            })
            .expect("settings should update");

        let refreshed_qr = http_get(&qr_url);
        let refreshed_room = runtime
            .list_rooms()
            .expect("rooms should load after settings update")
            .into_iter()
            .find(|room| room.room.room.id == created.room.room.id)
            .expect("created room should still exist");

        assert_ne!(initial_qr, refreshed_qr);
        let refreshed_viewer_url = refreshed_room
            .viewer_links
            .first()
            .map(|viewer_link| viewer_link.viewer_url.clone())
            .expect("viewer URL should be rebuilt from the new public URL");
        assert!(
            refreshed_viewer_url.starts_with("https://viewer.example.com/canvas-mirror/?room="),
            "unexpected refreshed viewer URL: {refreshed_viewer_url}"
        );
        assert!(
            refreshed_viewer_url.contains(&format!("room={}", created.room.room.id)),
            "viewer URL should include the room id: {refreshed_viewer_url}"
        );
        assert!(
            refreshed_viewer_url.contains("&token="),
            "viewer URL should include a room token: {refreshed_viewer_url}"
        );
    }

    #[test]
    fn load_icc_profile_from_path_reads_bytes_and_uses_file_stem_as_name() {
        let dir = tempdir().expect("temp dir should exist");
        let icc_path = dir.path().join("LG ULTRAFINE.icc");
        fs::write(&icc_path, [0_u8, 1, 2, 3]).expect("icc file should write");

        let profile = load_icc_profile_from_path(&icc_path).expect("icc profile should load");

        assert_eq!(profile.name, "LG ULTRAFINE");
        assert_eq!(profile.bytes, vec![0, 1, 2, 3]);
    }

    fn reserve_local_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("ephemeral listener should bind");
        listener
            .local_addr()
            .expect("listener address should resolve")
            .port()
    }

    fn write_test_config(config_path: &Path, port: u16) {
        let config_dir = config_path
            .parent()
            .expect("config path should have a parent directory");
        fs::create_dir_all(config_dir).expect("config dir should be created");
        fs::write(
            config_path,
            format!("bind_addr = \"127.0.0.1:{port}\"\nstore_path = \"./{STORE_FILE_NAME}\"\n"),
        )
        .expect("config file should be written");
    }

    fn create_test_log_file(base_dir: &Path) -> RuntimeLogFile {
        RuntimeLogFile::create(base_dir.join("logs"), "canvas-mirror-studio-test")
            .expect("test log file should be created")
    }

    fn http_get(url: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            match reqwest::blocking::get(url) {
                Ok(response) => {
                    return response.text().expect("response body should be readable");
                }
                Err(_) if Instant::now() < deadline => {
                    sleep(Duration::from_millis(25));
                }
                Err(error) => panic!("expected transport server to accept connections: {error}"),
            }
        }
    }
}
