mod runtime_log_file;
mod transport;

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
};

use canvas_mirror_config::ServerConfig;
use canvas_mirror_core::{CoreError, PublishSnapshotCommand, ServerCore};
use canvas_mirror_model::RoomState;
use canvas_mirror_preview::PreviewGenerator;
use canvas_mirror_store::RoomRecord;
use canvas_mirror_watcher::WatcherEvent;
use if_addrs::get_if_addrs;
pub use runtime_log_file::RuntimeLogFile;
use sha2::{Digest, Sha256};
use tokio::{sync::broadcast, task::JoinHandle};
pub use transport::{
    default_viewer_html, generate_room_qr_svg, handle_viewer_socket, load_viewer_html,
    start_transport_server, RoomQrSvgError, TransportRuntime,
};
use url::Url;

pub fn spawn_snapshot_pipeline(
    core: ServerCore,
    store_path: PathBuf,
    events: broadcast::Receiver<WatcherEvent>,
) -> JoinHandle<()> {
    spawn_snapshot_pipeline_with_generator(core, store_path, events, PreviewGenerator)
}

pub fn spawn_snapshot_pipeline_with_generator(
    core: ServerCore,
    store_path: PathBuf,
    mut events: broadcast::Receiver<WatcherEvent>,
    preview_generator: PreviewGenerator,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        process_all_rooms(&core, &store_path, &preview_generator).await;

        loop {
            match events.recv().await {
                Ok(event) => {
                    process_room_event(&core, &store_path, &preview_generator, &event.room_id)
                        .await;
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    process_all_rooms(&core, &store_path, &preview_generator).await;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    })
}

pub async fn process_all_rooms(
    core: &ServerCore,
    store_path: &Path,
    preview_generator: &PreviewGenerator,
) {
    for room in core.room_records() {
        process_room_event(core, store_path, preview_generator, &room.id).await;
    }
}

pub async fn process_room_event(
    core: &ServerCore,
    store_path: &Path,
    preview_generator: &PreviewGenerator,
    room_id: &str,
) {
    let Some(room) = core.room_record(room_id) else {
        return;
    };
    if !room.detection_enabled {
        return;
    }

    let target_path = resolve_target_path(store_path, &room.target_path);
    let preview = match preview_generator
        .generate(&target_path, &room.resolution)
        .await
    {
        Ok(preview) => preview,
        Err(error) => {
            let _ = core.set_room_error(room_id, error.to_string());
            return;
        }
    };
    let content_hash = hash_bytes(&preview.bytes);

    if should_skip_snapshot(core, room_id, &content_hash) {
        clear_room_error_if_present(core, room_id);
        return;
    }

    match core.publish_snapshot(
        room_id,
        PublishSnapshotCommand {
            content_hash,
            bytes: preview.bytes,
            mime_type: Some(preview.mime_type),
            width: preview.width,
            height: preview.height,
        },
    ) {
        Ok(_) => clear_room_error_if_present(core, room_id),
        Err(CoreError::RoomNotFound { .. }) | Err(CoreError::RoomPaused { .. }) => {}
        Err(error) => {
            let _ = core.set_room_error(room_id, error.to_string());
        }
    }
}

pub fn viewer_public_urls(config: &ServerConfig) -> Vec<Url> {
    if let Some(public_url) = &config.public_url {
        return vec![normalize_viewer_url(public_url)];
    }

    derived_public_urls(config.bind_addr)
}

pub fn ws_public_url(public_url: &Url) -> String {
    let mut ws_url = normalize_viewer_url(public_url);
    let scheme = if ws_url.scheme() == "https" {
        "wss"
    } else {
        "ws"
    };
    let _ = ws_url.set_scheme(scheme);
    let ws_path = viewer_path_with_suffix(ws_url.path(), "ws");
    ws_url.set_path(&ws_path);
    ws_url.set_query(None);
    ws_url.set_fragment(None);
    ws_url.to_string()
}

pub fn room_viewer_urls(viewer_urls: &[Url], room: &RoomRecord) -> Vec<Url> {
    viewer_urls
        .iter()
        .map(|viewer_url| {
            let mut viewer_url = normalize_viewer_url(viewer_url);
            viewer_url
                .query_pairs_mut()
                .clear()
                .append_pair("room", &room.id)
                .append_pair("token", &room.viewer_token);
            viewer_url
        })
        .collect()
}

pub fn room_qr_url(viewer_url: &Url, room: &RoomRecord) -> Url {
    let mut qr_url = normalize_viewer_url(viewer_url);
    let qr_path = viewer_path_with_suffix(qr_url.path(), "qr.svg");
    qr_url.set_path(&qr_path);
    qr_url
        .query_pairs_mut()
        .clear()
        .append_pair("room", &room.id)
        .append_pair("token", &room.viewer_token);
    qr_url
}

pub fn preferred_viewer_url(viewer_urls: &[Url]) -> Option<Url> {
    viewer_urls
        .iter()
        .find(|viewer_url| {
            viewer_url
                .host_str()
                .and_then(|host| host.parse::<IpAddr>().ok())
                .map(|ip| !ip.is_loopback())
                .unwrap_or(true)
        })
        .cloned()
        .or_else(|| viewer_urls.first().cloned())
}

pub fn security_warnings(config: &ServerConfig, viewer_urls: &[Url]) -> Vec<String> {
    let mut warnings = Vec::new();
    let bind_exposes_lan = bind_addr_exposes_lan(config.bind_addr);

    if bind_exposes_lan {
        warnings.push(format!(
            "bind_addr is {}, so any device on the same network can reach this server",
            config.bind_addr
        ));
        if config.bind_addr.ip().is_unspecified() && config.public_url.is_none() {
            warnings.push(
                "public_url is auto-derived; verify the printed viewer URLs before sharing links or QR codes"
                    .to_string(),
            );
        }
    }

    if insecure_transport_exposed(config, viewer_urls) {
        warnings.push(
            "the direct HTTP/WS listener is reachable on a non-loopback address, so room tokens and images may travel in plaintext on the network"
                .to_string(),
        );
    }

    warnings
}

pub fn security_guidance(config: &ServerConfig, viewer_urls: &[Url]) -> Vec<String> {
    if insecure_transport_exposed(config, viewer_urls) {
        return vec![
            "for sensitive images or external sharing, put the server behind HTTPS/WSS via a reverse proxy or tunnel".to_string(),
            "common options: Caddy, Nginx, Cloudflare Tunnel, or Tailscale Funnel".to_string(),
        ];
    }

    Vec::new()
}

fn clear_room_error_if_present(core: &ServerCore, room_id: &str) {
    if let Some(room) = core.room(room_id) {
        if room.state == RoomState::Error || room.last_error.is_some() {
            let _ = core.clear_room_error(room_id);
        }
    }
}

fn should_skip_snapshot(core: &ServerCore, room_id: &str, content_hash: &str) -> bool {
    let Some(current_hash) = core
        .room(room_id)
        .and_then(|room| room.latest_snapshot.map(|snapshot| snapshot.content_hash))
    else {
        return false;
    };

    current_hash == content_hash
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn normalize_viewer_url(public_url: &Url) -> Url {
    let mut viewer_url = public_url.clone();
    let normalized_path = normalize_viewer_base_path(viewer_url.path());
    viewer_url.set_path(&normalized_path);
    viewer_url.set_query(None);
    viewer_url.set_fragment(None);
    viewer_url
}

fn normalize_viewer_base_path(path: &str) -> String {
    match path {
        "" | "/" => "/".to_string(),
        _ if path.ends_with('/') => path.to_string(),
        _ => format!("{path}/"),
    }
}

fn viewer_path_with_suffix(base_path: &str, suffix: &str) -> String {
    let normalized_base = normalize_viewer_base_path(base_path);
    if normalized_base == "/" {
        format!("/{suffix}")
    } else {
        format!("{normalized_base}{suffix}")
    }
}

fn derived_public_urls(bind_addr: SocketAddr) -> Vec<Url> {
    let interface_addrs: Vec<InterfaceAddress> = get_if_addrs()
        .map(|interfaces| {
            interfaces
                .into_iter()
                .map(|interface| {
                    let ip = interface.ip();
                    InterfaceAddress {
                        name: interface.name,
                        ip,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    derived_public_urls_from_interfaces(bind_addr, &interface_addrs)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InterfaceAddress {
    name: String,
    ip: IpAddr,
}

fn derived_public_urls_from_interfaces(
    bind_addr: SocketAddr,
    interface_addrs: &[InterfaceAddress],
) -> Vec<Url> {
    let mut urls = Vec::new();
    let port = bind_addr.port();

    match bind_addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            push_http_url(&mut urls, IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            push_ranked_interface_urls(&mut urls, port, interface_addrs);
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            push_http_url(&mut urls, IpAddr::V4(Ipv4Addr::LOCALHOST), port);
            push_ranked_interface_urls(&mut urls, port, interface_addrs);
        }
        ip => push_http_url(&mut urls, ip, port),
    }

    if urls.is_empty() {
        push_http_url(&mut urls, IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    }

    urls
}

fn push_ranked_interface_urls(
    urls: &mut Vec<Url>,
    port: u16,
    interface_addrs: &[InterfaceAddress],
) {
    let mut candidates = interface_addrs
        .iter()
        .filter_map(|interface| match interface.ip {
            IpAddr::V4(ip) if !ip.is_loopback() && !ip.is_link_local() => {
                Some((interface_sort_key(&interface.name, ip), IpAddr::V4(ip)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|(left_key, left_ip), (right_key, right_ip)| {
        left_key
            .cmp(right_key)
            .then_with(|| left_ip.to_string().cmp(&right_ip.to_string()))
    });

    for (_, ip) in candidates {
        push_http_url(urls, ip, port);
    }
}

fn interface_sort_key(name: &str, ip: Ipv4Addr) -> (u8, u8, String) {
    let normalized_name = name.to_ascii_lowercase();
    (
        interface_name_rank(&normalized_name),
        ipv4_scope_rank(ip),
        normalized_name,
    )
}

fn interface_name_rank(name: &str) -> u8 {
    if is_primary_interface_name(name) {
        0
    } else if is_common_physical_interface_name(name) {
        1
    } else if is_virtual_interface_name(name) {
        3
    } else {
        2
    }
}

fn ipv4_scope_rank(ip: Ipv4Addr) -> u8 {
    if ip.is_private() {
        1
    } else {
        0
    }
}

fn is_primary_interface_name(name: &str) -> bool {
    matches!(name, "en0" | "eth0" | "wlan0" | "wifi0" | "wl0")
}

fn is_common_physical_interface_name(name: &str) -> bool {
    name.starts_with("en")
        || name.starts_with("eth")
        || name.starts_with("wlan")
        || name.starts_with("wifi")
        || name.starts_with("wl")
}

fn is_virtual_interface_name(name: &str) -> bool {
    name.starts_with("bridge")
        || name.starts_with("br-")
        || name.starts_with("docker")
        || name.starts_with("veth")
        || name.starts_with("virbr")
        || name.starts_with("vboxnet")
        || name.starts_with("vmnet")
        || name.starts_with("utun")
        || name.starts_with("tun")
        || name.starts_with("tap")
        || name.starts_with("tailscale")
        || name.starts_with("zt")
        || name.starts_with("lo")
        || name.starts_with("awdl")
        || name.starts_with("llw")
}

fn push_http_url(urls: &mut Vec<Url>, ip: IpAddr, port: u16) {
    let raw = match ip {
        IpAddr::V4(ip) => format!("http://{ip}:{port}"),
        IpAddr::V6(ip) => format!("http://[{ip}]:{port}"),
    };

    let Ok(url) = raw.parse::<Url>() else {
        return;
    };
    if !urls.iter().any(|existing| existing == &url) {
        urls.push(url);
    }
}

fn bind_addr_exposes_lan(bind_addr: SocketAddr) -> bool {
    bind_addr.ip().is_unspecified() || !bind_addr.ip().is_loopback()
}

fn insecure_transport_exposed(config: &ServerConfig, viewer_urls: &[Url]) -> bool {
    bind_addr_exposes_lan(config.bind_addr)
        || viewer_urls.iter().any(is_shareable_insecure_viewer_url)
}

fn is_shareable_insecure_viewer_url(viewer_url: &Url) -> bool {
    if viewer_url.scheme() != "http" {
        return false;
    }

    match viewer_url.host() {
        Some(url::Host::Ipv4(ip)) => !ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => !ip.is_loopback(),
        Some(url::Host::Domain(host)) => host != "localhost",
        None => false,
    }
}

fn resolve_target_path(store_path: &Path, target_path: &Path) -> PathBuf {
    if target_path.is_absolute() {
        return target_path.to_path_buf();
    }

    let base_dir = store_path.parent().unwrap_or_else(|| Path::new("."));
    base_dir.join(target_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas_mirror_store::{DetectionMode, OutputResolution};

    #[test]
    fn viewer_public_urls_use_explicit_public_url_when_present() {
        let config = ServerConfig {
            public_url: Some(
                "https://example.local:9000/base"
                    .parse()
                    .expect("sample URL should parse"),
            ),
            ..ServerConfig::default()
        };

        assert_eq!(
            viewer_public_urls(&config),
            vec!["https://example.local:9000/base/"
                .parse()
                .expect("normalized viewer URL should parse")]
        );
    }

    #[test]
    fn ws_public_url_uses_websocket_scheme() {
        let public_url = "http://127.0.0.1:8787/canvas-mirror/"
            .parse()
            .expect("sample URL should parse");

        assert_eq!(
            ws_public_url(&public_url),
            "ws://127.0.0.1:8787/canvas-mirror/ws"
        );
    }

    #[test]
    fn derived_public_urls_include_localhost_and_private_ipv4_for_wildcard_bind() {
        let urls = derived_public_urls_from_interfaces(
            SocketAddr::from(([0, 0, 0, 0], 8787)),
            &[
                sample_interface("lo0", Ipv4Addr::new(127, 0, 0, 1)),
                sample_interface("en0", Ipv4Addr::new(192, 168, 0, 23)),
                sample_interface("en1", Ipv4Addr::new(10, 0, 0, 14)),
            ],
        );

        assert_eq!(
            urls,
            vec![
                "http://127.0.0.1:8787"
                    .parse()
                    .expect("localhost URL should parse"),
                "http://192.168.0.23:8787"
                    .parse()
                    .expect("private URL should parse"),
                "http://10.0.0.14:8787"
                    .parse()
                    .expect("private URL should parse"),
            ]
        );
    }

    #[test]
    fn derived_public_urls_fall_back_to_non_loopback_ipv4_when_no_private_ip_exists() {
        let urls = derived_public_urls_from_interfaces(
            SocketAddr::from(([0, 0, 0, 0], 8787)),
            &[
                sample_interface("lo0", Ipv4Addr::new(127, 0, 0, 1)),
                sample_interface("en0", Ipv4Addr::new(110, 76, 78, 33)),
            ],
        );

        assert_eq!(
            urls,
            vec![
                "http://127.0.0.1:8787"
                    .parse()
                    .expect("localhost URL should parse"),
                "http://110.76.78.33:8787"
                    .parse()
                    .expect("public fallback URL should parse"),
            ]
        );
    }

    #[test]
    fn derived_public_urls_prefer_primary_interface_over_virtual_networks() {
        let urls = derived_public_urls_from_interfaces(
            SocketAddr::from(([0, 0, 0, 0], 8787)),
            &[
                sample_interface("lo0", Ipv4Addr::new(127, 0, 0, 1)),
                sample_interface("bridge100", Ipv4Addr::new(192, 168, 64, 1)),
                sample_interface("en0", Ipv4Addr::new(110, 76, 78, 33)),
            ],
        );

        assert_eq!(
            urls,
            vec![
                "http://127.0.0.1:8787"
                    .parse()
                    .expect("localhost URL should parse"),
                "http://110.76.78.33:8787"
                    .parse()
                    .expect("primary interface URL should parse"),
                "http://192.168.64.1:8787"
                    .parse()
                    .expect("virtual interface URL should parse"),
            ]
        );
    }

    #[test]
    fn room_viewer_urls_append_room_query_parameter() {
        let urls = room_viewer_urls(
            &[
                "http://127.0.0.1:8787/canvas-mirror/"
                    .parse()
                    .expect("viewer URL should parse"),
                "http://192.168.0.23:8787/canvas-mirror/"
                    .parse()
                    .expect("viewer URL should parse"),
            ],
            &sample_room(),
        );

        assert_eq!(
            urls,
            vec![
                "http://127.0.0.1:8787/canvas-mirror/?room=room-illustration&token=viewer-token-abc"
                    .parse()
                    .expect("room viewer URL should parse"),
                "http://192.168.0.23:8787/canvas-mirror/?room=room-illustration&token=viewer-token-abc"
                    .parse()
                    .expect("room viewer URL should parse"),
            ]
        );
    }

    #[test]
    fn room_qr_url_appends_room_query_parameter_and_qr_path() {
        let url = room_qr_url(
            &"http://192.168.0.23:8787/canvas-mirror/"
                .parse()
                .expect("viewer URL should parse"),
            &sample_room(),
        );

        assert_eq!(
            url,
            "http://192.168.0.23:8787/canvas-mirror/qr.svg?room=room-illustration&token=viewer-token-abc"
                .parse()
                .expect("QR URL should parse")
        );
    }

    #[test]
    fn preferred_viewer_url_prefers_non_loopback_address() {
        let viewer_urls = vec![
            "http://127.0.0.1:8787/"
                .parse()
                .expect("viewer URL should parse"),
            "http://192.168.0.23:8787/"
                .parse()
                .expect("viewer URL should parse"),
        ];

        assert_eq!(
            preferred_viewer_url(&viewer_urls),
            Some(
                "http://192.168.0.23:8787/"
                    .parse()
                    .expect("viewer URL should parse")
            )
        );
    }

    #[test]
    fn security_warnings_flag_wildcard_bind_and_auto_public_url() {
        let config = ServerConfig {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 8787)),
            public_url: None,
            ..ServerConfig::default()
        };
        let viewer_urls = vec![
            "http://127.0.0.1:8787/"
                .parse()
                .expect("viewer URL should parse"),
            "http://192.168.0.23:8787/"
                .parse()
                .expect("viewer URL should parse"),
        ];

        assert_eq!(
            security_warnings(&config, &viewer_urls),
            vec![
                "bind_addr is 0.0.0.0:8787, so any device on the same network can reach this server"
                    .to_string(),
                "public_url is auto-derived; verify the printed viewer URLs before sharing links or QR codes"
                    .to_string(),
                "the direct HTTP/WS listener is reachable on a non-loopback address, so room tokens and images may travel in plaintext on the network"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn security_guidance_recommends_tls_for_shareable_http_urls() {
        let config = ServerConfig::default();
        let viewer_urls = vec!["http://192.168.0.23:8787/"
            .parse()
            .expect("viewer URL should parse")];

        assert_eq!(
            security_guidance(&config, &viewer_urls),
            vec![
                "for sensitive images or external sharing, put the server behind HTTPS/WSS via a reverse proxy or tunnel"
                    .to_string(),
                "common options: Caddy, Nginx, Cloudflare Tunnel, or Tailscale Funnel"
                    .to_string(),
            ]
        );
    }

    #[test]
    fn security_guidance_is_empty_for_https_public_url() {
        let config = ServerConfig {
            public_url: Some(
                "https://viewer.example.com/"
                    .parse()
                    .expect("viewer URL should parse"),
            ),
            ..ServerConfig::default()
        };
        let viewer_urls = viewer_public_urls(&config);

        assert!(security_warnings(&config, &viewer_urls).is_empty());
        assert!(security_guidance(&config, &viewer_urls).is_empty());
    }

    #[test]
    fn security_warnings_still_flag_plaintext_listener_when_public_url_is_https() {
        let config = ServerConfig {
            bind_addr: SocketAddr::from(([0, 0, 0, 0], 8787)),
            public_url: Some(
                "https://viewer.example.com/"
                    .parse()
                    .expect("viewer URL should parse"),
            ),
            ..ServerConfig::default()
        };
        let viewer_urls = viewer_public_urls(&config);

        assert_eq!(
            security_warnings(&config, &viewer_urls),
            vec![
                "bind_addr is 0.0.0.0:8787, so any device on the same network can reach this server"
                    .to_string(),
                "the direct HTTP/WS listener is reachable on a non-loopback address, so room tokens and images may travel in plaintext on the network"
                    .to_string(),
            ]
        );
        assert_eq!(
            security_guidance(&config, &viewer_urls),
            vec![
                "for sensitive images or external sharing, put the server behind HTTPS/WSS via a reverse proxy or tunnel"
                    .to_string(),
                "common options: Caddy, Nginx, Cloudflare Tunnel, or Tailscale Funnel"
                    .to_string(),
            ]
        );
    }

    fn sample_room() -> RoomRecord {
        RoomRecord {
            id: "room-illustration".to_string(),
            name: "Illustration Board".to_string(),
            viewer_token: "viewer-token-abc".to_string(),
            detection_enabled: true,
            target_path: PathBuf::from("./sample.clip"),
            mode: DetectionMode::Watch,
            interval_ms: 2_000,
            debounce_ms: 750,
            stabilize_ms: 300,
            resolution: OutputResolution::Source,
        }
    }

    fn sample_interface(name: &str, ip: Ipv4Addr) -> InterfaceAddress {
        InterfaceAddress {
            name: name.to_string(),
            ip: IpAddr::V4(ip),
        }
    }
}
