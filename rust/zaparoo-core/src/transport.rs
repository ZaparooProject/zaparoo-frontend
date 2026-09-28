// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Native WebSocket transports. Credentials are ephemeral, never serialized or
//! included in diagnostics. Locality describes the connection, not Core's platform.

use futures_util::{Sink, Stream};
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest,
    http::{header::AUTHORIZATION, HeaderValue, Uri},
    protocol::WebSocketConfig,
    Error, Message,
};

pub(crate) trait Socket:
    Stream<Item = Result<Message, Error>> + Sink<Message, Error = Error> + Send + Unpin
{
}
impl<T> Socket for T where
    T: Stream<Item = Result<Message, Error>> + Sink<Message, Error = Error> + Send + Unpin
{
}

/// A host may replace this value when its service generation changes. This type
/// deliberately has no serialization implementation; keys must not enter config.
#[derive(Clone, PartialEq, Eq)]
pub struct Transport {
    target: Target,
    api_key: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
enum Target {
    Tcp(String),
    #[cfg(unix)]
    Unix {
        path: std::path::PathBuf,
        generation: u64,
    },
}

impl std::fmt::Debug for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // URLs can themselves contain credentials, including legacy query keys.
        f.debug_struct("Transport")
            .field("kind", &self.kind())
            .field("is_local", &self.is_local())
            .field("authenticated", &self.api_key.is_some())
            .finish_non_exhaustive()
    }
}

impl Transport {
    /// Existing desktop/development endpoints retain their configured URL.
    /// Hosts can supply an API key without embedding it in that URL.
    pub fn tcp(endpoint: String, api_key: Option<String>) -> Self {
        Self {
            target: Target::Tcp(endpoint),
            api_key,
        }
    }

    /// A filesystem socket requires an explicit credential. Generation is part
    /// of identity even when a host reuses the same path and credential.
    #[cfg(unix)]
    pub fn unix(path: std::path::PathBuf, api_key: String, generation: u64) -> Self {
        Self {
            target: Target::Unix { path, generation },
            api_key: Some(api_key),
        }
    }

    pub fn kind(&self) -> &'static str {
        match &self.target {
            Target::Tcp(_) => "tcp-websocket",
            #[cfg(unix)]
            Target::Unix { .. } => "unix-websocket",
        }
    }

    pub fn is_local(&self) -> bool {
        match &self.target {
            Target::Tcp(endpoint) => endpoint.parse::<Uri>().ok().is_some_and(|uri| {
                matches!(uri.scheme_str(), Some("ws" | "wss"))
                    && uri.host().is_some_and(|host| {
                        host.eq_ignore_ascii_case("localhost")
                            || host
                                .trim_start_matches('[')
                                .trim_end_matches(']')
                                .parse::<std::net::IpAddr>()
                                .is_ok_and(|address| address.is_loopback())
                    })
            }),
            #[cfg(unix)]
            Target::Unix { .. } => true,
        }
    }

    pub(crate) async fn connect(
        &self,
        config: WebSocketConfig,
    ) -> Result<std::pin::Pin<Box<dyn Socket>>, Error> {
        let endpoint = match &self.target {
            Target::Tcp(endpoint) => endpoint.as_str(),
            // HTTP's Host field is required by the WS handshake, but this URI
            // is never resolved or passed to a TCP connector.
            #[cfg(unix)]
            Target::Unix { path, .. } => {
                if !path.is_absolute() {
                    return Err(invalid_input("Unix socket path must be absolute"));
                }
                "ws://localhost/api/v0.1"
            }
        };
        let mut request = endpoint.into_client_request()?;
        if let Some(key) = &self.api_key {
            if key.is_empty() {
                return Err(invalid_input("API key must not be empty"));
            }
            let mut value = HeaderValue::from_str(&format!("Bearer {key}"))
                .map_err(|_| invalid_input("Invalid API key header"))?;
            value.set_sensitive(true);
            request.headers_mut().insert(AUTHORIZATION, value);
        }
        match &self.target {
            Target::Tcp(_) => {
                let (socket, _) =
                    tokio_tungstenite::connect_async_with_config(request, Some(config), false)
                        .await?;
                Ok(Box::pin(socket))
            }
            #[cfg(unix)]
            Target::Unix { path, .. } => {
                let stream = tokio::net::UnixStream::connect(path).await?;
                let (socket, _) =
                    tokio_tungstenite::client_async_with_config(request, stream, Some(config))
                        .await?;
                Ok(Box::pin(socket))
            }
        }
    }
}

fn invalid_input(message: &'static str) -> Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message).into()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

    fn check_key(request: &Request, response: Response) -> Response {
        assert_eq!(request.uri().path(), "/api/v0.1");
        assert_eq!(
            request.headers()[AUTHORIZATION],
            "Bearer transport-test-key"
        );
        response
    }

    #[test]
    fn locality_uses_transport_not_names_containing_localhost() {
        for endpoint in [
            "ws://localhost:7497/api/v0.1",
            "ws://127.0.0.1/api/v0.1",
            "ws://[::1]/api/v0.1",
        ] {
            assert!(Transport::tcp(endpoint.into(), None).is_local());
        }
        for endpoint in [
            "ws://localhost.example/api",
            "ws://example/localhost",
            "ws://192.168.1.2/api",
            "disabled://localhost",
            "invalid",
        ] {
            assert!(!Transport::tcp(endpoint.into(), None).is_local());
        }
    }

    #[test]
    fn diagnostics_hide_keys_and_url_credentials() {
        let transport = Transport::tcp(
            "ws://secret@example/api?key=secret".into(),
            Some("secret".into()),
        );
        assert!(!format!("{transport:?}").contains("secret"));
    }

    #[tokio::test]
    #[allow(
        clippy::result_large_err,
        reason = "tungstenite handshake callback signature"
    )]
    async fn tcp_sends_authorization_and_keeps_rpc_frames() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("ws://{}/api/v0.1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &Request, response: Response| Ok(check_key(request, response)),
            )
            .await
            .unwrap();
            socket
                .send(Message::Text("rpc-reply".into()))
                .await
                .unwrap();
        });
        let mut socket = Transport::tcp(endpoint, Some("transport-test-key".into()))
            .connect(WebSocketConfig::default())
            .await
            .unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("rpc-reply".into())
        );
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    #[allow(
        clippy::result_large_err,
        reason = "tungstenite handshake callback signature"
    )]
    async fn unix_sends_authorization_without_tcp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("api.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        let transport = Transport::unix(path.clone(), "transport-test-key".into(), 1);
        assert!(transport.is_local());
        assert_ne!(
            transport,
            Transport::unix(path, "transport-test-key".into(), 2)
        );
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                |request: &Request, response: Response| Ok(check_key(request, response)),
            )
            .await
            .unwrap();
            socket
                .send(Message::Text("unix-reply".into()))
                .await
                .unwrap();
        });
        let mut socket = transport.connect(WebSocketConfig::default()).await.unwrap();
        assert_eq!(
            socket.next().await.unwrap().unwrap(),
            Message::Text("unix-reply".into())
        );
        server.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn invalid_unix_options_fail_before_dialing() {
        for transport in [
            Transport::unix("relative.sock".into(), "key".into(), 1),
            Transport::unix("/not-used.sock".into(), String::new(), 1),
            Transport::unix("/not-used.sock".into(), "key\r\ninjection".into(), 1),
        ] {
            let error = transport
                .connect(WebSocketConfig::default())
                .await
                .err()
                .unwrap();
            assert!(
                matches!(error, Error::Io(ref error) if error.kind() == std::io::ErrorKind::InvalidInput)
            );
            assert!(!error.to_string().contains("injection"));
        }
    }
}
