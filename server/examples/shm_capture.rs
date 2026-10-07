//! SHM capture probe: handshakes, resizes a window (so a terminal renders at a
//! real size), captures one raw `wl_shm` frame of that window and writes it to a
//! file for offline PNG conversion.
//!
//!     cargo run --example shm_capture -- <addr> <width> <height> <out.raw>

use std::{sync::Arc, time::Duration};

use aqua_server::net::tls;
use aqua_server::protocol::{capability, frame, v1, ClientMessage, ServerMessage, PROTOCOL_VERSION};
use prost::Message;

const CERT: &[u8] = include_bytes!("../certs/dev-cert.pem");
const KEY: &[u8] = include_bytes!("../certs/dev-key.pem");

async fn read_control(recv: &mut quinn::RecvStream) -> Option<(u32, Vec<u8>)> {
    let mut len_buf = [0u8; 8];
    recv.read_exact(&mut len_buf).await.ok()?;
    let (tag, len) = frame::split_header(&len_buf);
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload).await.ok()?;
    Some((tag, payload))
}

async fn read_surface_message(
    recv: &mut quinn::RecvStream,
) -> Option<(v1::SurfaceStreamHeader, Vec<u8>)> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await.ok()?;
    let header_len = frame::split_stream_header_length(&len_buf);
    if header_len == 0 || header_len > frame::MAX_STREAM_HEADER {
        return None;
    }
    let mut header_buf = vec![0u8; header_len];
    recv.read_exact(&mut header_buf).await.ok()?;
    let header = v1::SurfaceStreamHeader::decode(&header_buf[..]).ok()?;
    let payload_len = header.payload_len as usize;
    let mut payload = vec![0u8; payload_len];
    if payload_len > 0 {
        recv.read_exact(&mut payload).await.ok()?;
    }
    Some((header, payload))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let addr = args.get(1).cloned().unwrap_or_else(|| "127.0.0.1:52420".into());
    let width: i32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1128);
    let height: i32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(715);
    let out = args.get(4).cloned().unwrap_or_else(|| "/tmp/shm_capture.raw".into());

    let (_, identity) = tls::server_config(CERT, KEY)?;
    let rustls = tls::client_config(&identity.fingerprint_sha256)?;
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls)?;
    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic)));
    let connection = endpoint.connect(addr.parse()?, "localhost")?.await?;

    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(
        &ClientMessage::Hello(v1::ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_session_id: uuid::Uuid::new_v4().to_string(),
            capabilities: Some(v1::Capabilities {
                bits: capability::PHASE_3B | capability::SURFACE_VIDEO,
            }),
            client_name: "shm-capture".into(),
        })
        .encode(),
    )
    .await?;

    // Read ServerHello then Snapshot to learn the first window id.
    let mut window_id = String::new();
    let mut got_snapshot = false;
    while let Some((tag, payload)) = read_control(&mut recv).await {
        if let Some(ServerMessage::Snapshot(snapshot)) = ServerMessage::decode(tag, &payload) {
            window_id = snapshot
                .windows
                .first()
                .map(|w| w.window_id.clone())
                .unwrap_or_default();
            got_snapshot = true;
        }
        if got_snapshot {
            break;
        }
    }
    println!("window={window_id} -> resize {width}x{height}");
    send.write_all(
        &ClientMessage::ViewportChanged(v1::ViewportChanged {
            window_id: window_id.clone(),
            width,
            height,
            scale: 1.0,
            is_final: true,
        })
        .encode(),
    )
    .await?;

    // Accept surface streams; capture the first frame of the window's root.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        let recv = match tokio::time::timeout_at(deadline, connection.accept_uni()).await {
            Ok(Ok(recv)) => recv,
            _ => break,
        };
        let mut recv = recv;
        while let Some((header, payload)) = read_surface_message(&mut recv).await {
            // Skip tiny surfaces (cursor/decoration); take the terminal.
            if header.kind == v1::SurfaceStreamKind::SurfaceStreamFrame as u32
                && header.width >= 100
                && header.height >= 100
            {
                std::fs::write(&out, &payload)?;
                println!(
                    "CAPTURED surface={} window={} {}x{} stride={} format={} bytes={} -> {out}",
                    header.surface_id,
                    header.window_id,
                    header.width,
                    header.height,
                    header.stride,
                    header.format,
                    payload.len()
                );
                connection.close(0u32.into(), b"done");
                return Ok(());
            }
        }
    }
    eprintln!("no root frame captured");
    connection.close(0u32.into(), b"done");
    Ok(())
}
