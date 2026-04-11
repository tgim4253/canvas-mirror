mod transport;

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{bail, Context, Result};
use canvas_mirror_config::ServerConfig;
use canvas_mirror_core::{ServerCore, UpdateRoomCommand};
use canvas_mirror_host::{
    preferred_viewer_url, room_qr_url, room_viewer_urls, security_guidance, security_warnings,
    spawn_snapshot_pipeline, viewer_public_urls, ws_public_url,
};
use canvas_mirror_model::{LogEntryDto, LogLevel, RoomDto, RoomState, ServerStatusDto};
use canvas_mirror_store::{DetectionMode, OutputResolution, RoomRecord};
use canvas_mirror_watcher::WatcherService;
use clap::{Args, Parser, Subcommand, ValueEnum};
use tokio::{sync::watch, task::JoinHandle, time::Duration};
use transport::{default_viewer_html, start_transport_server};

#[derive(Debug, Parser)]
#[command(name = "canvas-mirror")]
#[command(
    about = "Canvas Mirror CLI for mirroring illustration previews over your local network",
    long_about = None
)]
struct Cli {
    /// Path to the server config file.
    #[arg(long, global = true, default_value = "./canvas-mirror-config.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Load the runtime and keep the process alive.
    Serve,
    /// Print the current runtime status snapshot.
    Status(OutputArgs),
    /// Manage persisted rooms in the room store.
    Room {
        #[command(subcommand)]
        command: RoomCommand,
    },
}

#[derive(Debug, Subcommand)]
enum RoomCommand {
    /// List rooms from the persisted store through the runtime.
    List(OutputArgs),
    /// Create a room in the persisted store.
    Create(RoomCreateArgs),
    /// Update an existing room in the persisted store.
    Update(RoomUpdateArgs),
    /// Delete a room from the persisted store.
    Delete(RoomDeleteArgs),
}

#[derive(Debug, Clone, Args)]
struct OutputArgs {
    /// Print JSON instead of the human-readable summary.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RoomCreateArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    name: String,
    #[arg(long)]
    detection_off: bool,
    #[arg(long)]
    target_path: PathBuf,
    #[arg(long, value_enum, default_value_t = ModeArg::Watch)]
    mode: ModeArg,
    #[arg(long, default_value_t = 2_000)]
    interval_ms: u64,
    #[arg(long, default_value_t = 750)]
    debounce_ms: u64,
    #[arg(long, default_value_t = 300)]
    stabilize_ms: u64,
    #[arg(long, value_enum, default_value_t = ResolutionArg::Contain)]
    resolution: ResolutionArg,
    #[arg(long)]
    max_width: Option<u32>,
    #[arg(long)]
    max_height: Option<u32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RoomUpdateArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    name: Option<String>,
    #[arg(long, conflicts_with = "detection_off")]
    detection_on: bool,
    #[arg(long, conflicts_with = "detection_on")]
    detection_off: bool,
    #[arg(long)]
    target_path: Option<PathBuf>,
    #[arg(long, value_enum)]
    mode: Option<ModeArg>,
    #[arg(long)]
    interval_ms: Option<u64>,
    #[arg(long)]
    debounce_ms: Option<u64>,
    #[arg(long)]
    stabilize_ms: Option<u64>,
    #[arg(long, value_enum)]
    resolution: Option<ResolutionArg>,
    #[arg(long)]
    max_width: Option<u32>,
    #[arg(long)]
    max_height: Option<u32>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Args)]
struct RoomDeleteArgs {
    #[arg(long)]
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ModeArg {
    Watch,
    Interval,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ResolutionArg {
    Source,
    Contain,
}

impl From<ModeArg> for DetectionMode {
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Watch => Self::Watch,
            ModeArg::Interval => Self::Interval,
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Serve => serve(&cli.config).await,
        Command::Status(output) => {
            let core = load_core(&cli.config)?;
            print_status(&core.status(), output.json)
        }
        Command::Room { command } => handle_room_command(&cli.config, command),
    }
}

async fn serve(config_path: &Path) -> Result<()> {
    let core = load_core(config_path)?;
    let initial_log_cursor = core.log_cursor();
    let (log_shutdown_tx, log_shutdown_rx) = watch::channel(false);
    let log_tailer = spawn_runtime_log_tailer(core.clone(), initial_log_cursor, log_shutdown_rx);
    let config = core.config();
    let viewer_urls = viewer_public_urls(&config);
    let qr_viewer_url = preferred_viewer_url(&viewer_urls)
        .context("failed to derive a viewer URL for QR generation")?;
    let viewer_html = load_viewer_html(&config)?;
    let transport = start_transport_server(
        core.clone(),
        config.bind_addr,
        viewer_html,
        qr_viewer_url.clone(),
    )
    .await
    .context("failed to start websocket transport")?;
    let watchers = WatcherService::new(core.clone())
        .start()
        .await
        .context("failed to start room watchers")?;
    let snapshot_pipeline = spawn_snapshot_pipeline(
        core.clone(),
        config.store_path.clone(),
        watchers.subscribe(),
    );
    let attached_watchers = watchers.attached_rooms();
    let room_records = core.room_records();
    let status = core.status();

    println!("config: {}", config_path.display());
    println!(
        "public_url: {}",
        config
            .public_url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "auto".to_string())
    );
    println!("store_path: {}", config.store_path.display());
    println!("rooms_loaded: {}", status.rooms.len());
    println!("runtime_ready: true");
    println!("watchers_attached: {}", attached_watchers);
    if attached_watchers > 0 {
        println!("watcher_rooms: {}", watchers.room_ids().join(", "));
    }
    println!("ws_transport_attached: true");
    print_public_urls("viewer_url", "viewer_urls", &viewer_urls);
    let ws_urls: Vec<String> = viewer_urls.iter().map(ws_public_url).collect();
    print_string_urls("ws_endpoint", "ws_endpoints", &ws_urls);
    print_security_notices(&config, &viewer_urls);
    print_room_viewer_links(&room_records, &viewer_urls);
    print_room_qr_links(&room_records, &qr_viewer_url);
    println!("message: press Ctrl-C to stop");

    tokio::signal::ctrl_c()
        .await
        .context("failed while waiting for Ctrl-C")?;

    transport.shutdown().await;
    watchers.shutdown().await;
    snapshot_pipeline.abort();
    let _ = snapshot_pipeline.await;
    let _ = log_shutdown_tx.send(true);
    let _ = log_tailer.await;
    println!("shutdown: received Ctrl-C");
    Ok(())
}

fn spawn_runtime_log_tailer(
    core: ServerCore,
    start_cursor: u64,
    mut shutdown_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut next_cursor = start_cursor;
        let mut ticker = tokio::time::interval(Duration::from_millis(250));

        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => break,
                _ = ticker.tick() => {
                    let (cursor, logs) = core.read_logs_since(next_cursor);
                    for log in &logs {
                        print_runtime_log(log);
                    }
                    next_cursor = cursor;
                }
            }
        }
    })
}

fn print_runtime_log(log: &LogEntryDto) {
    println!(
        "log: {} {} [{}] {}",
        log.at.to_rfc3339(),
        format_log_level(&log.level),
        log.scope,
        log.message
    );
}

fn format_log_level(level: &LogLevel) -> &'static str {
    match level {
        LogLevel::Info => "info",
        LogLevel::Warn => "warn",
        LogLevel::Error => "error",
    }
}

fn print_public_urls(single_label: &str, multi_label: &str, urls: &[url::Url]) {
    let rendered: Vec<String> = urls.iter().map(ToString::to_string).collect();
    print_string_urls(single_label, multi_label, &rendered);
}

fn print_security_notices(config: &ServerConfig, viewer_urls: &[url::Url]) {
    let warnings = security_warnings(config, viewer_urls);
    let guidance = security_guidance(config, viewer_urls);

    if !warnings.is_empty() {
        println!("security_warnings:");
        for warning in warnings {
            println!("- {warning}");
        }
    }

    if !guidance.is_empty() {
        println!("security_guidance:");
        for note in guidance {
            println!("- {note}");
        }
    }
}

fn print_room_viewer_links(rooms: &[RoomRecord], viewer_urls: &[url::Url]) {
    if rooms.is_empty() || viewer_urls.is_empty() {
        return;
    }

    println!("room_viewer_links:");
    for room in rooms {
        for viewer_url in room_viewer_urls(viewer_urls, room) {
            println!("- {} -> {}", room.id, viewer_url);
        }
    }
}

fn print_room_qr_links(rooms: &[RoomRecord], viewer_url: &url::Url) {
    if rooms.is_empty() {
        return;
    }

    println!("room_qr_links:");
    for room in rooms {
        println!("- {} -> {}", room.id, room_qr_url(viewer_url, room));
    }
}

fn print_string_urls(single_label: &str, multi_label: &str, urls: &[String]) {
    match urls {
        [] => println!("{single_label}: none"),
        [url] => println!("{single_label}: {url}"),
        _ => {
            println!("{multi_label}:");
            for url in urls {
                println!("- {url}");
            }
        }
    }
}

fn handle_room_command(config_path: &Path, command: RoomCommand) -> Result<()> {
    let core = load_core(config_path)?;

    match command {
        RoomCommand::List(output) => {
            let status = core.status();
            if output.json {
                println!("{}", serde_json::to_string_pretty(&status.rooms)?);
            } else {
                print_rooms(&status.rooms);
            }
            Ok(())
        }
        RoomCommand::Create(args) => {
            let room = RoomRecord {
                id: args.id,
                name: args.name,
                viewer_token: String::new(),
                detection_enabled: !args.detection_off,
                target_path: args.target_path,
                mode: args.mode.into(),
                interval_ms: args.interval_ms,
                debounce_ms: args.debounce_ms,
                stabilize_ms: args.stabilize_ms,
                resolution: build_resolution(
                    Some(args.resolution),
                    args.max_width,
                    args.max_height,
                )?
                .expect("create resolution must exist"),
            };
            let room = core.create_room(room)?;
            print_room(&room, args.json)
        }
        RoomCommand::Update(args) => {
            let resolution = build_resolution(args.resolution, args.max_width, args.max_height)?;
            let detection_enabled = match (args.detection_on, args.detection_off) {
                (true, false) => Some(true),
                (false, true) => Some(false),
                _ => None,
            };
            let command = UpdateRoomCommand {
                name: args.name,
                detection_enabled,
                target_path: args.target_path,
                mode: args.mode.map(Into::into),
                interval_ms: args.interval_ms,
                debounce_ms: args.debounce_ms,
                stabilize_ms: args.stabilize_ms,
                resolution,
            };

            if command == UpdateRoomCommand::default() {
                bail!("no update fields were provided");
            }

            let room = core.update_room(&args.id, command)?;
            print_room(&room, args.json)
        }
        RoomCommand::Delete(args) => {
            let room = core.delete_room(&args.id)?;
            print_room(&room, args.json)
        }
    }
}

fn load_core(config_path: &Path) -> Result<ServerCore> {
    let config = load_server_config(config_path)?;
    ServerCore::load(config).context("failed to initialize server core")
}

fn load_server_config(config_path: &Path) -> Result<ServerConfig> {
    let mut config = ServerConfig::load_from_path(config_path)
        .with_context(|| format!("failed to load config from {}", config_path.display()))?;
    let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

    if config.store_path.is_relative() {
        config.store_path = base_dir.join(&config.store_path);
    }
    if let Some(viewer_path) = &mut config.viewer_path {
        if viewer_path.is_relative() {
            *viewer_path = base_dir.join(&*viewer_path);
        }
    }

    Ok(config)
}

fn load_viewer_html(config: &ServerConfig) -> Result<Arc<str>> {
    let Some(viewer_path) = &config.viewer_path else {
        return Ok(default_viewer_html());
    };

    let viewer_html = std::fs::read_to_string(viewer_path)
        .with_context(|| format!("failed to read viewer HTML from {}", viewer_path.display()))?;
    Ok(Arc::<str>::from(viewer_html))
}

fn build_resolution(
    resolution: Option<ResolutionArg>,
    max_width: Option<u32>,
    max_height: Option<u32>,
) -> Result<Option<OutputResolution>> {
    match resolution {
        None => {
            if max_width.is_some() || max_height.is_some() {
                bail!("--max-width/--max-height require --resolution contain");
            }
            Ok(None)
        }
        Some(ResolutionArg::Source) => {
            if max_width.is_some() || max_height.is_some() {
                bail!("--max-width/--max-height are only valid with --resolution contain");
            }
            Ok(Some(OutputResolution::Source))
        }
        Some(ResolutionArg::Contain) => Ok(Some(OutputResolution::Contain {
            max_width: max_width.unwrap_or(1440),
            max_height: max_height.unwrap_or(1440),
        })),
    }
}

fn print_status(status: &ServerStatusDto, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(status)?);
        return Ok(());
    }

    println!(
        "public_url: {}",
        status
            .public_url
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_else(|| "auto".to_string())
    );
    println!("generated_at: {}", status.generated_at.to_rfc3339());
    println!("rooms: {}", status.rooms.len());
    println!("logs: {}", status.logs.len());
    print_rooms(&status.rooms);
    Ok(())
}

fn print_rooms(rooms: &[RoomDto]) {
    if rooms.is_empty() {
        println!("rooms: none");
        return;
    }

    for room in rooms {
        let snapshot = if room.latest_snapshot.is_some() {
            "yes"
        } else {
            "no"
        };
        println!(
            "{} [{}] name=\"{}\" devices={} snapshot={}{}",
            room.room.id,
            room_state_label(&room.state),
            room.room.name,
            room.devices.len(),
            snapshot,
            room.last_error
                .as_ref()
                .map(|error| format!(
                    " detection={} error=\"{}\"",
                    on_off_label(room.room.detection_enabled),
                    error
                ))
                .unwrap_or_else(|| {
                    format!(" detection={}", on_off_label(room.room.detection_enabled))
                })
        );
    }
}

fn print_room(room: &RoomDto, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(room)?);
        return Ok(());
    }

    println!("id: {}", room.room.id);
    println!("name: {}", room.room.name);
    println!("detection: {}", on_off_label(room.room.detection_enabled));
    println!("state: {}", room_state_label(&room.state));
    println!("mode: {}", mode_label(&room.room.mode));
    println!("interval_ms: {}", room.room.interval_ms);
    println!("debounce_ms: {}", room.room.debounce_ms);
    println!("stabilize_ms: {}", room.room.stabilize_ms);
    println!("resolution: {}", resolution_label(&room.room.resolution));
    println!("devices: {}", room.devices.len());
    println!(
        "latest_snapshot: {}",
        if room.latest_snapshot.is_some() {
            "present"
        } else {
            "none"
        }
    );
    if let Some(error) = &room.last_error {
        println!("last_error: {}", error);
    }
    Ok(())
}

fn room_state_label(state: &RoomState) -> &'static str {
    match state {
        RoomState::Running => "running",
        RoomState::Paused => "paused",
        RoomState::Error => "error",
    }
}

fn mode_label(mode: &DetectionMode) -> &'static str {
    match mode {
        DetectionMode::Watch => "watch",
        DetectionMode::Interval => "interval",
    }
}

fn on_off_label(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off"
    }
}

fn resolution_label(resolution: &OutputResolution) -> String {
    match resolution {
        OutputResolution::Source => "source".to_string(),
        OutputResolution::Contain {
            max_width,
            max_height,
        } => format!("contain({max_width}x{max_height})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_store_path_is_resolved_against_config_file() {
        let mut config = ServerConfig::default();
        config.store_path = PathBuf::from("./data/rooms.toml");
        let config_path = PathBuf::from("/tmp/canvas-mirror/canvas-mirror-config.toml");
        let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

        if config.store_path.is_relative() {
            config.store_path = base_dir.join(&config.store_path);
        }

        assert_eq!(
            config.store_path,
            PathBuf::from("/tmp/canvas-mirror/./data/rooms.toml")
        );
    }

    #[test]
    fn relative_viewer_path_is_resolved_against_config_file() {
        let mut config = ServerConfig::default();
        config.viewer_path = Some(PathBuf::from("./viewer/custom.html"));
        let config_path = PathBuf::from("/tmp/canvas-mirror/canvas-mirror-config.toml");
        let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));

        if let Some(viewer_path) = &mut config.viewer_path {
            if viewer_path.is_relative() {
                *viewer_path = base_dir.join(&*viewer_path);
            }
        }

        assert_eq!(
            config.viewer_path,
            Some(PathBuf::from("/tmp/canvas-mirror/./viewer/custom.html"))
        );
    }

    #[test]
    fn load_viewer_html_uses_custom_override_when_present() {
        let dir = tempfile::tempdir().expect("temp dir should exist");
        let viewer_path = dir.path().join("custom-viewer.html");
        std::fs::write(&viewer_path, "<html>custom viewer</html>")
            .expect("viewer file should be written");
        let config = ServerConfig {
            viewer_path: Some(viewer_path),
            ..ServerConfig::default()
        };

        let viewer_html = load_viewer_html(&config).expect("viewer HTML should load");

        assert_eq!(&*viewer_html, "<html>custom viewer</html>");
    }

    #[test]
    fn contain_resolution_uses_defaults_when_dimensions_are_missing() {
        let resolution = build_resolution(Some(ResolutionArg::Contain), None, None)
            .expect("resolution should build");

        assert_eq!(
            resolution,
            Some(OutputResolution::Contain {
                max_width: 1440,
                max_height: 1440,
            })
        );
    }

    #[test]
    fn source_resolution_rejects_contain_dimensions() {
        let error = build_resolution(Some(ResolutionArg::Source), Some(100), Some(100))
            .expect_err("source resolution should reject width and height");

        assert!(error
            .to_string()
            .contains("only valid with --resolution contain"));
    }
}
