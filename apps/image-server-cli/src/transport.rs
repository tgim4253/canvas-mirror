use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::{
    extract::{ws::WebSocketUpgrade, Query, State},
    http::{header::CONTENT_TYPE, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use image_server_core::ServerCore;
use image_server_host::{generate_room_qr_svg, handle_viewer_socket, RoomQrSvgError};
use serde::Deserialize;
use tokio::{net::TcpListener, sync::oneshot, task::JoinHandle};
const DEFAULT_VIEWER_HTML: &str = include_str!("../../viewer/index.html");

#[derive(Clone)]
struct TransportAppState {
    core: ServerCore,
    viewer_html: Arc<str>,
    qr_viewer_base: url::Url,
}

pub struct TransportRuntime {
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: JoinHandle<()>,
}

impl TransportRuntime {
    pub async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        let _ = self.handle.await;
    }
}

pub async fn start_transport_server(
    core: ServerCore,
    bind_addr: SocketAddr,
    viewer_html: Arc<str>,
    qr_viewer_base: url::Url,
) -> Result<TransportRuntime> {
    let listener = TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind websocket transport to {bind_addr}"))?;
    let app = Router::new()
        .route("/", get(viewer_page))
        .route("/qr.svg", get(qr_svg_page))
        .route("/ws", get(ws_handler))
        .with_state(TransportAppState {
            core,
            viewer_html,
            qr_viewer_base,
        });
    let (shutdown_tx, shutdown_rx) = oneshot::channel();
    let handle = tokio::spawn(async move {
        let server = axum::serve(listener, app).with_graceful_shutdown(async move {
            let _ = shutdown_rx.await;
        });

        if let Err(error) = server.await {
            eprintln!("warn: websocket transport server stopped: {error}");
        }
    });

    Ok(TransportRuntime {
        shutdown_tx: Some(shutdown_tx),
        handle,
    })
}

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<TransportAppState>) -> Response {
    ws.on_upgrade(move |socket| handle_viewer_socket(state.core, socket))
}

pub(crate) fn default_viewer_html() -> Arc<str> {
    Arc::<str>::from(DEFAULT_VIEWER_HTML)
}

async fn viewer_page(State(state): State<TransportAppState>) -> Html<String> {
    Html(state.viewer_html.to_string())
}

async fn qr_svg_page(
    State(state): State<TransportAppState>,
    Query(query): Query<QrCodeQuery>,
) -> Result<impl IntoResponse, StatusCode> {
    if query.room.trim().is_empty() || query.token.trim().is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let svg = generate_room_qr_svg(
        &state.core,
        &state.qr_viewer_base,
        &query.room,
        &query.token,
    )
    .map_err(|error| match error {
        RoomQrSvgError::InvalidLink => StatusCode::NOT_FOUND,
        RoomQrSvgError::RenderFailed => StatusCode::INTERNAL_SERVER_ERROR,
    })?;

    Ok(([(CONTENT_TYPE, "image/svg+xml; charset=utf-8")], svg))
}

#[derive(Debug, Deserialize)]
struct QrCodeQuery {
    room: String,
    token: String,
}
