//! QUIC transport server (Quinn).
//!
//! Runs on a dedicated Tokio runtime. It never touches Wayland state directly:
//! it decodes client messages into [`ClientCommand`]s and forwards them to the
//! calloop loop through a `calloop` channel, and it forwards
//! [`ServerMessage`]s produced by the loop back to clients.

use std::{
    net::SocketAddr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use quinn::{Endpoint, RecvStream, SendStream};
use tokio::sync::{
    mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender},
    Mutex,
};

use crate::protocol::{frame, v1, ClientMessage, ServerMessage, PROTOCOL_VERSION};

pub use super::tls::TlsIdentity;

use super::frames::{dispatch_frames, FrameHub};
use super::tls;
use super::video::{dispatch_video, VideoHub};

/// Sender of server -> client messages for one connected client.
pub type Outgoing = UnboundedSender<ServerMessage>;

/// Where the network runtime delivers [`ServerEvent`]s (implemented for the
/// calloop channel in the server, and for a std channel in tests).
pub trait ServerEventSink: Send + Sync + 'static {
    fn send_event(&self, event: ServerEvent);
}

impl ServerEventSink for smithay::reexports::calloop::channel::Sender<ServerEvent> {
    fn send_event(&self, event: ServerEvent) {
        let _ = self.send(event);
    }
}

impl ServerEventSink for std::sync::mpsc::Sender<ServerEvent> {
    fn send_event(&self, event: ServerEvent) {
        let _ = self.send(event);
    }
}

/// Client commands decoded from the wire and applied to the Wayland seat/window.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientCommand {
    ViewportChanged {
        window_id: String,
        width: i32,
        height: i32,
        scale: f64,
        is_final: bool,
    },
    Focus {
        window_id: String,
    },
    PointerMoved {
        window_id: String,
        x: f64,
        y: f64,
    },
    PointerButton {
        window_id: String,
        button: u32,
        pressed: bool,
        x: f64,
        y: f64,
    },
    PointerScroll {
        window_id: String,
        dx: f64,
        dy: f64,
    },
    Key {
        window_id: String,
        keycode: u32,
        characters: String,
        modifiers: u32,
        pressed: bool,
    },
    Touch {
        window_id: String,
        id: u32,
        phase: u32,
        x: f64,
        y: f64,
    },
    FramePresented {
        surface_id: String,
        frame_id: u64,
        presentation_time_us: u64,
    },
    RequestKeyframe {
        window_id: String,
        reason: String,
    },
}

/// Events produced by the network runtime and consumed by the calloop loop.
#[derive(Debug)]
pub enum ServerEvent {
    ClientConnected {
        client_id: u64,
        client_session_id: String,
        /// Capability bits the client advertised in `ClientHello`.
        client_capabilities: u32,
        outgoing: Outgoing,
    },
    ClientDisconnected {
        client_id: u64,
    },
    Command {
        client_id: u64,
        command: ClientCommand,
    },
}

#[derive(Clone)]
pub struct NetworkConfig {
    pub bind: SocketAddr,
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

/// Owns the Tokio runtime that drives Quinn. Dropping it stops the transport.
pub struct NetworkServer {
    #[allow(dead_code)] // kept alive to drive the runtime
    runtime: tokio::runtime::Runtime,
    pub local_addr: SocketAddr,
    pub identity: TlsIdentity,
}

impl NetworkServer {
    pub fn spawn<S>(
        config: NetworkConfig,
        events: S,
        frames: Arc<FrameHub>,
        video: Arc<VideoHub>,
    ) -> Result<Self, String>
    where
        S: ServerEventSink + Clone,
    {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("aqua-quic")
            .build()
            .map_err(|e| e.to_string())?;

        let (addr_tx, addr_rx) = tokio::sync::oneshot::channel();
        let config_clone = config.clone();
        runtime.spawn(async move {
            if let Err(error) = run(config_clone, events, frames, video, addr_tx).await {
                tracing::error!(error = %error, "QUIC server stopped");
            }
        });

        let (local_addr, identity) =
            runtime.block_on(async move { addr_rx.await.map_err(|e| e.to_string()) })?;
        Ok(Self {
            runtime,
            local_addr,
            identity,
        })
    }
}

async fn run<S>(
    config: NetworkConfig,
    events: S,
    frames: Arc<FrameHub>,
    video: Arc<VideoHub>,
    ready: tokio::sync::oneshot::Sender<(SocketAddr, TlsIdentity)>,
) -> Result<(), String>
where
    S: ServerEventSink + Clone,
{
    let (server_config, identity) = tls::server_config(&config.cert_pem, &config.key_pem)?;
    let quic_server = quinn::crypto::rustls::QuicServerConfig::try_from(server_config)
        .map_err(|e| e.to_string())?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_server));

    let endpoint = Endpoint::server(server_config, config.bind).map_err(|e| e.to_string())?;
    let local_addr = endpoint.local_addr().map_err(|e| e.to_string())?;
    let _ = ready.send((local_addr, identity.clone()));
    tracing::info!(%local_addr, fingerprint = %identity.fingerprint_sha256, "quic listening");

    let next_client = Arc::new(std::sync::atomic::AtomicU64::new(1));
    while let Some(incoming) = endpoint.accept().await {
        let events = events.clone();
        let next_client = next_client.clone();
        let frames = frames.clone();
        let video = video.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(incoming, events, next_client, frames, video).await {
                tracing::debug!(error = %error, "connection ended with error");
            }
        });
    }
    Ok(())
}

async fn handle_connection<S>(
    incoming: quinn::Incoming,
    events: S,
    next_client: Arc<std::sync::atomic::AtomicU64>,
    frames: Arc<FrameHub>,
    video: Arc<VideoHub>,
) -> Result<(), String>
where
    S: ServerEventSink + Clone,
{
    let connection = incoming.await.map_err(|e| e.to_string())?;
    let client_id = next_client.fetch_add(1, Ordering::SeqCst);
    tracing::info!(client_id, remote = %connection.remote_address(), "connection.open");
    tokio::spawn(dispatch_frames(connection.clone(), frames));
    tokio::spawn(dispatch_video(connection.clone(), video));

    let (out_tx, out_rx) = unbounded_channel::<ServerMessage>();
    let out_rx = Arc::new(Mutex::new(Some(out_rx)));
    let registered = Arc::new(AtomicBool::new(false));

    loop {
        tokio::select! {
            stream = connection.accept_bi() => {
                match stream {
                    Ok((send, recv)) => {
                        let events = events.clone();
                        let out_rx = out_rx.clone();
                        let out_tx = out_tx.clone();
                        let registered = registered.clone();
                        tokio::spawn(serve_stream(
                            send, recv, client_id, events, out_rx, out_tx, registered,
                        ));
                    }
                    Err(error) => {
                        tracing::debug!(client_id, error = %error, "accept_bi ended");
                        break;
                    }
                }
            }
            reason = connection.closed() => {
                tracing::info!(client_id, error = %reason, "connection.closed");
                break;
            }
        }
    }

    events.send_event(ServerEvent::ClientDisconnected { client_id });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn serve_stream<S>(
    send: SendStream,
    mut recv: RecvStream,
    client_id: u64,
    events: S,
    out_rx: Arc<Mutex<Option<UnboundedReceiver<ServerMessage>>>>,
    out_tx: Outgoing,
    registered: Arc<AtomicBool>,
) where
    S: ServerEventSink + Clone,
{
    let mut send = Some(send);
    loop {
        let (tag, payload) = match next_frame(&mut recv).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return,
            Err(error) => {
                tracing::debug!(client_id, error = %error, "stream read ended");
                return;
            }
        };

        let Some(message) = ClientMessage::decode(tag, &payload) else {
            tracing::debug!(client_id, tag, "ignoring unknown message");
            continue;
        };

        match message {
            ClientMessage::Hello(hello) => {
                if hello.protocol_version != PROTOCOL_VERSION {
                    let reply = ServerMessage::Hello(v1::ServerHello {
                        protocol_version: PROTOCOL_VERSION,
                        error: "unsupported_protocol_version".to_string(),
                        ..Default::default()
                    });
                    if let Some(mut send) = send.take() {
                        let _ = send.write_all(&reply.encode()).await;
                        let _ = send.finish();
                    }
                    tracing::warn!(
                        client_id,
                        client_version = hello.protocol_version,
                        server_version = PROTOCOL_VERSION,
                        "handshake.version_mismatch"
                    );
                    return;
                }
                if registered.swap(true, Ordering::SeqCst) {
                    continue; // one control stream per connection
                }
                tracing::info!(
                    client_id,
                    client_session_id = %hello.client_session_id,
                    "handshake.client"
                );
                events.send_event(ServerEvent::ClientConnected {
                    client_id,
                    client_session_id: hello.client_session_id,
                    client_capabilities: hello.capabilities.map(|c| c.bits).unwrap_or(0),
                    outgoing: out_tx.clone(),
                });
                let pump_stream = send.take();
                match (pump_stream, out_rx.lock().await.take()) {
                    (Some(send), Some(rx)) => {
                        // Keep the read half alive and keep reading client
                        // commands on this control stream.
                        tokio::spawn(pump(send, rx));
                    }
                    (Some(mut send), None) => {
                        let _ = send.finish();
                    }
                    (None, _) => {}
                }
                continue;
            }
            other => {
                if let Some(command) = to_command(other) {
                    events.send_event(ServerEvent::Command { client_id, command });
                }
            }
        }
    }
}

async fn pump(mut send: SendStream, mut rx: UnboundedReceiver<ServerMessage>) {
    while let Some(message) = rx.recv().await {
        if send.write_all(&message.encode()).await.is_err() {
            break;
        }
    }
    let _ = send.finish();
}

async fn next_frame(recv: &mut RecvStream) -> Result<Option<(u32, Vec<u8>)>, String> {
    let mut header = [0u8; frame::HEADER_LEN];
    match recv.read_exact(&mut header).await {
        Ok(()) => {}
        Err(quinn::ReadExactError::FinishedEarly(_)) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let (tag, len) = frame::split_header(&header);
    if len > frame::MAX_PAYLOAD {
        return Err("frame payload too large".to_string());
    }
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload)
        .await
        .map_err(|e| e.to_string())?;
    Ok(Some((tag, payload)))
}

fn to_command(message: ClientMessage) -> Option<ClientCommand> {
    Some(match message {
        ClientMessage::ViewportChanged(v) => ClientCommand::ViewportChanged {
            window_id: v.window_id,
            width: v.width,
            height: v.height,
            scale: v.scale,
            is_final: v.is_final,
        },
        ClientMessage::WindowFocusRequested(v) => ClientCommand::Focus {
            window_id: v.window_id,
        },
        ClientMessage::PointerMoved(v) => ClientCommand::PointerMoved {
            window_id: v.window_id,
            x: v.x,
            y: v.y,
        },
        ClientMessage::PointerButton(v) => ClientCommand::PointerButton {
            window_id: v.window_id,
            button: v.button,
            pressed: v.pressed,
            x: v.x,
            y: v.y,
        },
        ClientMessage::PointerScroll(v) => ClientCommand::PointerScroll {
            window_id: v.window_id,
            dx: v.dx,
            dy: v.dy,
        },
        ClientMessage::Key(v) => ClientCommand::Key {
            window_id: v.window_id,
            keycode: v.keycode,
            characters: v.characters,
            modifiers: v.modifiers,
            pressed: v.pressed,
        },
        ClientMessage::Touch(v) => ClientCommand::Touch {
            window_id: v.window_id,
            id: v.touch_id,
            phase: v.phase,
            x: v.x,
            y: v.y,
        },
        ClientMessage::FramePresented(v) => ClientCommand::FramePresented {
            surface_id: v.surface_id,
            frame_id: v.frame_id,
            presentation_time_us: v.presentation_time_us,
        },
        ClientMessage::RequestKeyframe(v) => ClientCommand::RequestKeyframe {
            window_id: v.window_id,
            reason: v.reason,
        },
        ClientMessage::Hello(_) | ClientMessage::Ping(_) => return None,
    })
}
