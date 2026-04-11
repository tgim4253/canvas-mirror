use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};
use image_server_config::ServerConfig;
use image_server_core::{ServerCore, UpdateRoomCommand};
use image_server_model::{RoomDto, RoomState, ServerStatusDto};
use image_server_store::{DetectionMode, OutputResolution, RoomRecord};

#[derive(Debug, Parser)]
#[command(name = "image-server-cli")]
#[command(about = "CLI wrapper for the image-server runtime", long_about = None)]
struct Cli {
    /// Path to the server config file.
    #[arg(long, global = true, default_value = "./server-config.toml")]
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
    let config = core.config();
    let status = core.status();

    println!("config: {}", config_path.display());
    println!("public_url: {}", config.public_url);
    println!("store_path: {}", config.store_path.display());
    println!("rooms_loaded: {}", status.rooms.len());
    println!("runtime_ready: true");
    println!("watchers_attached: false");
    println!("http_ingress_attached: false");
    println!("message: press Ctrl-C to stop");

    tokio::signal::ctrl_c()
        .await
        .context("failed while waiting for Ctrl-C")?;

    println!("shutdown: received Ctrl-C");
    Ok(())
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
            let command = UpdateRoomCommand {
                name: args.name,
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

    if config.store_path.is_relative() {
        let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
        config.store_path = base_dir.join(&config.store_path);
    }

    Ok(config)
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

    println!("public_url: {}", status.public_url);
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
                .map(|error| format!(" error=\"{}\"", error))
                .unwrap_or_default()
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
        let config_path = PathBuf::from("/tmp/image-server/server-config.toml");

        if config.store_path.is_relative() {
            let base_dir = config_path.parent().unwrap_or_else(|| Path::new("."));
            config.store_path = base_dir.join(&config.store_path);
        }

        assert_eq!(
            config.store_path,
            PathBuf::from("/tmp/image-server/./data/rooms.toml")
        );
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
