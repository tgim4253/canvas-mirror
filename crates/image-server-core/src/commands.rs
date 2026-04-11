#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UpdateRoomCommand {
    pub name: Option<String>,
    pub target_path: Option<std::path::PathBuf>,
    pub mode: Option<image_server_store::DetectionMode>,
    pub interval_ms: Option<u64>,
    pub debounce_ms: Option<u64>,
    pub stabilize_ms: Option<u64>,
    pub resolution: Option<image_server_store::OutputResolution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRoomCommand {
    pub id: String,
    pub name: String,
    pub platform: image_server_model::DevicePlatform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSnapshotCommand {
    pub device_id: Option<String>,
    pub bytes: Vec<u8>,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}
