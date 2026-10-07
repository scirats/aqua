//! Minimal Aqua QUIC client used for end-to-end verification of the server.
//!
//! Connects to a running `aqua-server`, performs the v1 handshake, prints the
//! ServerHello and ServerSnapshot (real Wayland windows), optionally requests a
//! viewport change, and exits.
//!
//!     cargo run --example quic_client -- 127.0.0.1:52420 [window-id]

use std::{sync::Arc, time::Duration};

use aqua_server::net::tls;
use aqua_server::protocol::{frame, v1, ClientMessage, ServerMessage, PROTOCOL_VERSION};

const CERT: &[u8] = include_bytes!("../certs/dev-cert.pem");
const KEY: &[u8] = include_bytes!("../certs/dev-key.pem");

async fn read_message(recv: &mut quinn::RecvStream) -> Option<ServerMessage> {
    let mut header = [0u8; frame::HEADER_LEN];
    recv.read_exact(&mut header).await.ok()?;
    let (tag, len) = frame::split_header(&header);
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload).await.ok()?;
    ServerMessage::decode(tag, &payload)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:52420".to_string());
    let resize_target = std::env::args().nth(2);

    let (_, identity) = tls::server_config(CERT, KEY)?;
    let rustls = tls::client_config(&identity.fingerprint_sha256)?;
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls)?;
    let client_config = quinn::ClientConfig::new(Arc::new(quic));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);
    let connection = endpoint.connect(addr.parse()?, "localhost")?.await?;
    println!("connected to {addr}");

    let (mut send, mut recv) = connection.open_bi().await?;
    let hello = ClientMessage::Hello(v1::ClientHello {
        protocol_version: PROTOCOL_VERSION,
        client_session_id: uuid::Uuid::new_v4().to_string(),
        capabilities: Some(v1::Capabilities {
            bits: aqua_server::protocol::capability::PHASE_3A,
        }),
        client_name: "quic-client".into(),
    });
    send.write_all(&hello.encode()).await?;

    loop {
        match read_message(&mut recv).await {
            Some(ServerMessage::Hello(hello)) => {
                println!(
                    "ServerHello version={} session={} revision={} error={:?}",
                    hello.protocol_version, hello.server_session_id, hello.revision, hello.error
                );
            }
            Some(ServerMessage::Snapshot(snapshot)) => {
                println!(
                    "Snapshot revision={} windows={}",
                    snapshot.revision,
                    snapshot.windows.len()
                );
                for window in &snapshot.windows {
                    println!(
                        "  window {} app={:?} title={:?} state={} mapped={}",
                        window.window_id,
                        window.application_id,
                        window.title,
                        window.state,
                        window.mapped
                    );
                }
                if let Some(target) = &resize_target {
                    let viewport = ClientMessage::ViewportChanged(v1::ViewportChanged {
                        window_id: target.clone(),
                        width: 800,
                        height: 600,
                        scale: 1.0,
                        is_final: true,
                    });
                    send.write_all(&viewport.encode()).await?;
                    println!("sent ViewportChanged for {target} (800x600)");
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
                break;
            }
            Some(other) => println!("ignored {:?}", other.tag()),
            None => {
                println!("stream closed before snapshot");
                break;
            }
        }
    }

    connection.close(0u32.into(), b"done");
    Ok(())
}
