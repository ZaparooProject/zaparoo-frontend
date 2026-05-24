// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Mock Zaparoo Core server. WebSocket JSON-RPC 2.0. Binds a dev-only
// port (ws://127.0.0.1:27497/api/v0.1) deliberately offset from the
// real Core's 7497 so a developer running both side-by-side never
// collides. The frontend's dev preset (run-dev) and `ZAPAROO_CORE_ENDPOINT`
// override point at this address; production frontend.toml still
// defaults to 7497.

mod fixtures;
mod handler;

use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    let addr = std::env::var("MOCK_CORE_ADDR").unwrap_or_else(|_| "127.0.0.1:27497".to_string());
    let listener = TcpListener::bind(&addr).await?;
    info!("mock-core listening on ws://{addr}/api/v0.1");
    info!("dev port (offset from the real Core's 7497); `just run-dev` and ZAPAROO_CORE_ENDPOINT target this automatically");

    loop {
        tokio::select! {
            accept = listener.accept() => {
                let (stream, peer) = accept?;
                tokio::spawn(async move {
                    if let Err(e) = serve(stream, peer).await {
                        warn!(%peer, "connection error: {e}");
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => {
                info!("shutdown requested");
                return Ok(());
            }
        }
    }
}

async fn serve(
    stream: TcpStream,
    peer: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let ws = tokio_tungstenite::accept_async(stream).await?;
    info!(%peer, "client connected");

    let (mut tx, mut rx) = ws.split();
    while let Some(msg) = rx.next().await {
        match msg? {
            Message::Text(text) => {
                let response = handler::dispatch(&text);
                tx.send(Message::Text(response.into())).await?;
            }
            Message::Ping(data) => {
                tx.send(Message::Pong(data)).await?;
            }
            Message::Close(_) => {
                info!(%peer, "client disconnected");
                break;
            }
            _ => {}
        }
    }
    Ok(())
}
