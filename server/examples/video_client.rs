//! Video-plane probe: connects to a running Aqua server, accepts the
//! server-opened unidirectional streams, and writes the window-video access
//! units to an Annex-B file for offline inspection (ffprobe/ffmpeg).
//!
//! Usage: cargo run --example video_client -- <addr> <fingerprint> <out.hevc> [seconds]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use prost::Message;

use aqua_server::net::tls;
use aqua_server::protocol::v1;

async fn read_message(recv: &mut quinn::RecvStream) -> Option<(v1::WindowVideoStreamHeader, Vec<u8>)> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.ok()?;
    let header_len = u32::from_be_bytes(len) as usize;
    let mut header = vec![0u8; header_len];
    recv.read_exact(&mut header).await.ok()?;
    let decoded = v1::WindowVideoStreamHeader::decode(&header[..]).ok()?;
    let mut payload = vec![0u8; decoded.payload_len as usize];
    if !payload.is_empty() {
        recv.read_exact(&mut payload).await.ok()?;
    }
    Some((decoded, payload))
}

fn client_config(fingerprint: &str) -> quinn::ClientConfig {
    let rustls = tls::client_config(fingerprint).expect("client tls");
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls).expect("quic");
    quinn::ClientConfig::new(Arc::new(quic))
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: video_client <addr> <fingerprint> <out.hevc> [seconds]");
        std::process::exit(2);
    }
    let addr = args[1].parse().expect("addr");
    let fingerprint = &args[2];
    let out_path = args[3].clone();
    let seconds: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(6);

    let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap()).unwrap();
    endpoint.set_default_client_config(client_config(fingerprint));
    let connection = endpoint.connect(addr, "aqua").unwrap().await.expect("connect");
    println!("connected to {addr}");

    let counters: Arc<Mutex<HashMap<String, u64>>> = Arc::new(Mutex::new(HashMap::new()));
    let deadline = Instant::now() + Duration::from_secs(seconds);

    loop {
        let accepted = tokio::time::timeout_at(deadline.into(), connection.accept_uni()).await;
        let mut recv = match accepted {
            Ok(Ok(recv)) => recv,
            _ => break,
        };
        let Some((hello, _)) = read_message(&mut recv).await else {
            continue;
        };
        if hello.stream_type != v1::DataStreamType::DataStreamWindowVideo as u32 {
            // SHM surface stream: drain it so it does not exhaust the connection
            // flow-control window, then ignore it.
            tokio::spawn(async move {
                let _ = recv.read_to_end(64 * 1024 * 1024).await;
            });
            continue;
        }
        if hello.kind != v1::WindowVideoStreamKind::WindowVideoStreamHello as u32 {
            continue;
        }
        let window_id = hello.window_id.clone();
        let out_path = out_path.clone();
        let counters = counters.clone();
        println!("video stream for {window_id}");
        tokio::spawn(async move {
            let mut file = std::fs::File::create(&out_path).expect("create out");
            use std::io::Write;
            while let Some((header, payload)) = read_message(&mut recv).await {
                match header.kind {
                    k if k == v1::WindowVideoStreamKind::WindowVideoStreamConfig as u32 => {
                        // Keep the parameter sets in-band so the capture decodes
                        // after an encoder restart.
                        let _ = file.write_all(&payload);
                        println!(
                            "CONFIG codec={} chroma={} {}x{} ps={} bytes",
                            header.codec,
                            header.chroma,
                            header.width,
                            header.height,
                            payload.len()
                        );
                    }
                    k if k == v1::WindowVideoStreamKind::WindowVideoStreamFrame as u32 => {
                        let _ = file.write_all(&payload);
                        let mut c = counters.lock().unwrap();
                        let entry = c.entry(window_id.clone()).or_insert(0);
                        *entry += 1;
                        if *entry == 1 || *entry % 30 == 0 {
                            println!(
                                "FRAME {} id={} key={} pts={} bytes={}",
                                window_id,
                                header.frame_id,
                                header.keyframe,
                                header.pts_us,
                                payload.len()
                            );
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    tokio::time::sleep(Duration::from_millis(200)).await;
    let counters = counters.lock().unwrap();
    println!("done; frames per window: {counters:?}");
}
