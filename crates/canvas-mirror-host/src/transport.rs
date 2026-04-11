use std::fmt::Display;

use axum::extract::ws::{Message, WebSocket};
use canvas_mirror_core::{CoreError, JoinRoomCommand, ServerCore, SnapshotBuffer};
use canvas_mirror_model::{DevicePlatform, RoomDeviceDto, SnapshotMetaDto};
use futures::{sink::Sink, SinkExt, StreamExt};
use qrcode_generator::QrCodeEcc;
use serde::{Deserialize, Serialize};
use tokio::time::{interval, sleep_until, timeout, Duration, Instant};
use url::Url;
use uuid::Uuid;

const PING_INTERVAL: Duration = Duration::from_secs(15);
const LIVENESS_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomQrSvgError {
    InvalidLink,
    RenderFailed,
}

pub fn generate_room_qr_svg(
    core: &ServerCore,
    qr_viewer_base: &Url,
    room_id: &str,
    token: &str,
) -> Result<String, RoomQrSvgError> {
    if room_id.trim().is_empty() || token.trim().is_empty() {
        return Err(RoomQrSvgError::InvalidLink);
    }

    let Some(room) = core.room_record(room_id) else {
        return Err(RoomQrSvgError::InvalidLink);
    };
    if room.viewer_token != token {
        return Err(RoomQrSvgError::InvalidLink);
    }

    let mut viewer_url = qr_viewer_base.clone();
    viewer_url
        .query_pairs_mut()
        .clear()
        .append_pair("room", room_id)
        .append_pair("token", token);

    qrcode_generator::to_svg_to_string(viewer_url.as_str(), QrCodeEcc::Medium, 320, None::<&str>)
        .map_err(|_| RoomQrSvgError::RenderFailed)
}

pub async fn handle_viewer_socket(core: ServerCore, mut socket: WebSocket) {
    let hello = match receive_hello(&mut socket).await {
        Ok(hello) => hello,
        Err(message) => {
            let _ = send_server_message(&mut socket, &ServerMessage::Error { message }).await;
            let _ = socket.close().await;
            return;
        }
    };
    let room_id = hello.room_id.clone();
    let device_id = Uuid::new_v4().to_string();
    let room = match core.room_record(&room_id) {
        Some(room) => room,
        None => {
            let _ = send_server_message(
                &mut socket,
                &ServerMessage::Error {
                    message: "invalid room link".to_string(),
                },
            )
            .await;
            let _ = socket.close().await;
            return;
        }
    };
    if hello.token != room.viewer_token {
        let _ = send_server_message(
            &mut socket,
            &ServerMessage::Error {
                message: "invalid room link".to_string(),
            },
        )
        .await;
        let _ = socket.close().await;
        return;
    }

    let joined_device = match core.join_room(
        &room_id,
        JoinRoomCommand {
            id: device_id.clone(),
            name: hello.name,
            platform: hello.platform,
            screen_width: hello.screen_width,
            screen_height: hello.screen_height,
        },
    ) {
        Ok(device) => device,
        Err(error) => {
            let _ = send_server_message(
                &mut socket,
                &ServerMessage::Error {
                    message: error.to_string(),
                },
            )
            .await;
            let _ = socket.close().await;
            return;
        }
    };
    let mut snapshot_events = core.subscribe_snapshot_events();

    if send_server_message(
        &mut socket,
        &ServerMessage::Joined {
            room_id: room_id.clone(),
            device_id: device_id.clone(),
            device: joined_device,
        },
    )
    .await
    .is_err()
    {
        let _ = core.leave_room(&room_id, &device_id);
        return;
    }

    if sync_latest_snapshot(&core, &mut socket, &room_id)
        .await
        .inspect_err(|error| {
            eprintln!("warn: failed to sync initial snapshot for room '{room_id}': {error}");
        })
        .is_err()
    {
        let _ = core.leave_room(&room_id, &device_id);
        return;
    }

    let (mut sender, mut receiver) = socket.split();
    let mut ping_interval = interval(PING_INTERVAL);
    let mut last_activity_at = Instant::now();

    loop {
        let idle_deadline = last_activity_at + LIVENESS_TIMEOUT;
        tokio::select! {
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(message)) => {
                        if !handle_client_message(&core, &mut sender, &room_id, &device_id, message).await {
                            break;
                        }
                        last_activity_at = Instant::now();
                    }
                    Some(Err(_)) | None => break,
                }
            }
            snapshot_event = snapshot_events.recv() => {
                match snapshot_event {
                    Ok(event) if event.room_id == room_id => {
                        if sync_latest_snapshot(&core, &mut sender, &room_id)
                            .await
                            .inspect_err(|error| {
                                eprintln!(
                                    "warn: failed to deliver snapshot update for room '{room_id}': {error}"
                                );
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if sync_latest_snapshot(&core, &mut sender, &room_id)
                            .await
                            .inspect_err(|error| {
                                eprintln!(
                                    "warn: failed to resync lagged snapshot for room '{room_id}': {error}"
                                );
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            _ = ping_interval.tick() => {
                if sender.send(Message::Ping(Vec::new().into())).await.is_err() {
                    break;
                }
            }
            _ = sleep_until(idle_deadline) => {
                eprintln!(
                    "warn: closing idle websocket session '{}' in room '{}' after {:?} without client activity",
                    device_id,
                    room_id,
                    LIVENESS_TIMEOUT
                );
                break;
            }
        }
    }

    let _ = core.leave_room(&room_id, &device_id);
}

async fn handle_client_message<S>(
    core: &ServerCore,
    sender: &mut S,
    room_id: &str,
    device_id: &str,
    message: Message,
) -> bool
where
    S: Sink<Message> + Unpin,
    S::Error: Display,
{
    match message {
        Message::Text(_) => {
            let _ = send_server_message(
                sender,
                &ServerMessage::Error {
                    message: "text client messages are not supported after handshake".to_string(),
                },
            )
            .await;
            false
        }
        Message::Ping(payload) => {
            touch_device_or_log(core, room_id, device_id, "client ping");
            sender.send(Message::Pong(payload)).await.is_ok()
        }
        Message::Pong(_) => {
            touch_device_or_log(core, room_id, device_id, "client pong");
            true
        }
        Message::Binary(_) => {
            let _ = send_server_message(
                sender,
                &ServerMessage::Error {
                    message: "binary client messages are not supported".to_string(),
                },
            )
            .await;
            false
        }
        Message::Close(_) => false,
    }
}

async fn receive_hello(socket: &mut WebSocket) -> Result<ClientHello, String> {
    let deadline = Instant::now() + Duration::from_secs(10);

    loop {
        let now = Instant::now();
        if now >= deadline {
            return Err("timed out waiting for hello message".to_string());
        }
        let remaining = deadline.checked_duration_since(now).unwrap_or_default();
        let Some(message) = timeout(remaining, socket.recv())
            .await
            .map_err(|_| "timed out waiting for hello message".to_string())?
        else {
            return Err("connection closed before hello message".to_string());
        };
        let message = message.map_err(|error| error.to_string())?;

        match message {
            Message::Text(text) => match serde_json::from_str::<HandshakeMessage>(&text) {
                Ok(HandshakeMessage::Hello {
                    room_id,
                    token,
                    name,
                    platform,
                    screen_width,
                    screen_height,
                }) => {
                    return Ok(ClientHello {
                        room_id,
                        token,
                        name,
                        platform,
                        screen_width,
                        screen_height,
                    });
                }
                Err(error) => {
                    return Err(format!("invalid hello message: {error}"));
                }
            },
            Message::Ping(payload) => {
                socket
                    .send(Message::Pong(payload))
                    .await
                    .map_err(|error| error.to_string())?;
            }
            Message::Pong(_) => {}
            Message::Binary(_) => {
                return Err("expected text hello message".to_string());
            }
            Message::Close(_) => return Err("connection closed before hello message".to_string()),
        }
    }
}

async fn send_snapshot_message<S>(
    sender: &mut S,
    room_id: &str,
    snapshot: SnapshotBuffer,
) -> Result<(), String>
where
    S: Sink<Message> + Unpin,
    S::Error: Display,
{
    send_server_message(
        sender,
        &ServerMessage::SnapshotMeta {
            room_id: room_id.to_string(),
            meta: snapshot.meta,
        },
    )
    .await?;

    sender
        .send(Message::Binary(snapshot.bytes.to_vec().into()))
        .await
        .map_err(|error| error.to_string())
}

async fn sync_latest_snapshot<S>(
    core: &ServerCore,
    sender: &mut S,
    room_id: &str,
) -> Result<(), String>
where
    S: Sink<Message> + Unpin,
    S::Error: Display,
{
    match core.snapshot(room_id) {
        Ok(Some(snapshot)) => send_snapshot_message(sender, room_id, snapshot).await,
        Ok(None) => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

async fn send_server_message<S>(sender: &mut S, message: &ServerMessage) -> Result<(), String>
where
    S: Sink<Message> + Unpin,
    S::Error: Display,
{
    let payload = serde_json::to_string(message).map_err(|error| error.to_string())?;
    sender
        .send(Message::Text(payload.into()))
        .await
        .map_err(|error| error.to_string())
}

fn touch_device_or_log(core: &ServerCore, room_id: &str, device_id: &str, source: &str) {
    match core.touch_device(room_id, device_id) {
        Ok(()) => {}
        Err(CoreError::RoomNotFound { .. } | CoreError::DeviceNotFound { .. }) => {}
        Err(error) => {
            eprintln!(
                "warn: failed to refresh device '{device_id}' in room '{room_id}' from {source}: {error}"
            );
        }
    }
}

#[derive(Debug)]
struct ClientHello {
    room_id: String,
    token: String,
    name: String,
    platform: DevicePlatform,
    screen_width: Option<u32>,
    screen_height: Option<u32>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum HandshakeMessage {
    Hello {
        room_id: String,
        token: String,
        name: String,
        platform: DevicePlatform,
        screen_width: Option<u32>,
        screen_height: Option<u32>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMessage {
    Joined {
        room_id: String,
        device_id: String,
        device: RoomDeviceDto,
    },
    SnapshotMeta {
        room_id: String,
        meta: SnapshotMetaDto,
    },
    Error {
        message: String,
    },
}
