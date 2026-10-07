//! Data-plane probe: receives raw SHM frames over per-surface QUIC streams.
//!
//!     cargo run --example frame_client -- 127.0.0.1:52420 [max-frames]

use std::{sync::Arc, time::Duration};

use aqua_server::net::tls;
use aqua_server::protocol::{frame, v1, ClientMessage, ServerMessage, PROTOCOL_VERSION};
use prost::Message;

const CERT: &[u8] = include_bytes!("../certs/dev-cert.pem");
const KEY: &[u8] = include_bytes!("../certs/dev-key.pem");

async fn read_frame_message(
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
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:52420".into());
    let _ = std::env::args().nth(2);

    let (_, identity) = tls::server_config(CERT, KEY)?;
    let rustls = tls::client_config(&identity.fingerprint_sha256)?;
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls)?;
    let client_config = quinn::ClientConfig::new(Arc::new(quic));

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse()?)?;
    endpoint.set_default_client_config(client_config);
    let connection = endpoint.connect(addr.parse()?, "localhost")?.await?;

    // Control stream: handshake + snapshot.
    let (mut send, mut recv) = connection.open_bi().await?;
    send.write_all(
        &ClientMessage::Hello(v1::ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_session_id: uuid::Uuid::new_v4().to_string(),
            capabilities: Some(v1::Capabilities {
                bits: aqua_server::protocol::capability::PHASE_3A,
            }),
            client_name: "frame-client".into(),
        })
        .encode(),
    )
    .await?;

    // Read control messages in a background task so the control stream stays live.
    tokio::spawn(async move {
        loop {
            let mut len_buf = [0u8; 8];
            if recv.read_exact(&mut len_buf).await.is_err() {
                break;
            }
            let (tag, len) = frame::split_header(&len_buf);
            let mut payload = vec![0u8; len];
            if recv.read_exact(&mut payload).await.is_err() {
                break;
            }
            if let Some(ServerMessage::Snapshot(snapshot)) = ServerMessage::decode(tag, &payload) {
                eprintln!(
                    "control: snapshot windows={} surfaces={}",
                    snapshot.windows.len(),
                    snapshot.surfaces.len()
                );
            }
        }
    });

    // Accept server-opened unidirectional surface streams.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut frames = 0;
    loop {
        let accepted = tokio::time::timeout_at(deadline, connection.accept_uni()).await;
        let recv = match accepted {
            Ok(Ok(recv)) => recv,
            Ok(Err(_)) => break,
            Err(_) => break,
        };
        let mut recv = recv;
        let mut stream_frames = 0usize;
        while let Some((header, payload)) = read_frame_message(&mut recv).await {
            match header.kind {
                k if k == v1::SurfaceStreamKind::SurfaceStreamHello as u32 => {
                    println!(
                        "HELLO surface={} window={}",
                        header.surface_id, header.window_id
                    );
                }
                k if k == v1::SurfaceStreamKind::SurfaceStreamFrame as u32 => {
                    stream_frames += 1;
                    frames += 1;
                    let first_pixel = payload.get(0..4).unwrap_or(&[]);
                    println!(
                        "FRAME surface={} frame_id={} {}x{} stride={} format={} bytes={} damage={} first_px={:02x?}",
                        header.surface_id,
                        header.frame_id,
                        header.width,
                        header.height,
                        header.stride,
                        header.format,
                        payload.len(),
                        header.damage.len(),
                        first_pixel
                    );
                    if stream_frames >= 2 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }

    println!("total frames received: {frames}");
    connection.close(0u32.into(), b"done");
    Ok(())
}
