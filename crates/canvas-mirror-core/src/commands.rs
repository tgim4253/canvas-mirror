#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateRoomCommand {
    pub name: Option<String>,
    pub detection_enabled: Option<bool>,
    pub target_path: Option<std::path::PathBuf>,
    pub mode: Option<canvas_mirror_store::DetectionMode>,
    pub interval_ms: Option<u64>,
    pub debounce_ms: Option<u64>,
    pub stabilize_ms: Option<u64>,
    pub resolution: Option<canvas_mirror_store::OutputResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRoomCommand {
    /// Caller-provided, session-like identifier for the current connected endpoint.
    pub id: String,
    pub name: String,
    pub platform: canvas_mirror_model::DevicePlatform,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSnapshotCommand {
    pub content_hash: String,
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
