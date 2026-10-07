//! Localhost transport tests: real QUIC (Quinn server <-> Quinn client), real
//! TLS, real framing and Protocol Buffers bodies. No Tailscale, no Wayland.

use std::{sync::Arc, time::Duration};

use aqua_server::net::tls;
use aqua_server::net::{NetworkConfig, NetworkServer, ServerEvent};
use aqua_server::protocol::{frame, v1, ClientMessage, ServerMessage, PROTOCOL_VERSION};

const CERT: &[u8] = include_bytes!("../certs/dev-cert.pem");
const KEY: &[u8] = include_bytes!("../certs/dev-key.pem");

fn config(port: u16) -> NetworkConfig {
    NetworkConfig {
        bind: format!("127.0.0.1:{port}").parse().unwrap(),
        cert_pem: CERT.to_vec(),
        key_pem: KEY.to_vec(),
    }
}

async fn read_message(recv: &mut quinn::RecvStream) -> Option<ServerMessage> {
    let mut header = [0u8; frame::HEADER_LEN];
    recv.read_exact(&mut header).await.ok()?;
    let (tag, len) = frame::split_header(&header);
    let mut payload = vec![0u8; len];
    recv.read_exact(&mut payload).await.ok()?;
    ServerMessage::decode(tag, &payload)
}

fn quinn_client_config(fingerprint: &str) -> quinn::ClientConfig {
    let rustls = tls::client_config(fingerprint).expect("client tls");
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls).expect("quic client");
    quinn::ClientConfig::new(Arc::new(quic))
}

#[test]
fn handshake_snapshot_and_command() {
    let (event_tx, event_rx) = std::sync::mpsc::channel();
    let server = NetworkServer::spawn(config(0), event_tx, aqua_server::net::FrameHub::new())
        .expect("server");
    let addr = server.local_addr;
    let fingerprint = server.identity.fingerprint_sha256.clone();
    assert!(!fingerprint.is_empty());

    // "core" thread: reply to the handshake and collect commands.
    let core = std::thread::spawn(move || {
        let mut commands = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match event_rx.recv_timeout(Duration::from_millis(200)) {
                Ok(ServerEvent::ClientConnected {
                    client_session_id,
                    outgoing,
                    ..
                }) => {
                    assert!(!client_session_id.is_empty());
                    let _ = outgoing.send(ServerMessage::Hello(v1::ServerHello {
                        protocol_version: PROTOCOL_VERSION,
                        server_session_id: "session-test".into(),
                        server_identity: "identity-test".into(),
                        capabilities: Some(v1::Capabilities { bits: 0b11110 }),
                        revision: 7,
                        error: String::new(),
                    }));
                    let _ = outgoing.send(ServerMessage::Snapshot(v1::ServerSnapshot {
                        revision: 7,
                        server_session_id: "session-test".into(),
                        surfaces: Vec::new(),
                        windows: vec![v1::WindowInfo {
                            window_id: "window-1".into(),
                            title: "simple-shm".into(),
                            ..Default::default()
                        }],
                    }));
                    let _ = outgoing.send(ServerMessage::WindowCreated(v1::WindowCreated {
                        revision: 8,
                        window: Some(v1::WindowInfo {
                            window_id: "window-2".into(),
                            title: "second".into(),
                            ..Default::default()
                        }),
                    }));
                    // keep the sender alive so the pump stays open
                    std::thread::sleep(Duration::from_millis(800));
                }
                Ok(ServerEvent::Command { command, .. }) => commands.push(command),
                Ok(ServerEvent::ClientDisconnected { .. }) => break,
                Err(_) => { /* timeout */ }
            }
        }
        commands
    });

    let rt = tokio::runtime::Runtime::new().unwrap();
    let received = rt.block_on(async move {
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(quinn_client_config(&fingerprint));
        let connection = endpoint
            .connect(addr, "localhost")
            .unwrap()
            .await
            .expect("connect");
        let (mut send, mut recv) = connection.open_bi().await.expect("open_bi");

        let hello = ClientMessage::Hello(v1::ClientHello {
            protocol_version: PROTOCOL_VERSION,
            client_session_id: "client-test".into(),
            capabilities: Some(v1::Capabilities { bits: 0b11110 }),
            client_name: "test".into(),
        });
        send.write_all(&hello.encode()).await.unwrap();

        let m1 = read_message(&mut recv).await.expect("hello reply");
        assert!(matches!(m1, ServerMessage::Hello(_)));
        if let ServerMessage::Hello(h) = &m1 {
            assert_eq!(h.protocol_version, PROTOCOL_VERSION);
            assert_eq!(h.server_session_id, "session-test");
            assert_eq!(h.revision, 7);
            assert!(h.error.is_empty());
        }

        let m2 = read_message(&mut recv).await.expect("snapshot");
        if let ServerMessage::Snapshot(s) = &m2 {
            assert_eq!(s.revision, 7);
            assert_eq!(s.windows.len(), 1);
            assert_eq!(s.windows[0].window_id, "window-1");
        } else {
            panic!("expected snapshot, got {m2:?}");
        }

        let m3 = read_message(&mut recv).await.expect("window created");
        if let ServerMessage::WindowCreated(w) = &m3 {
            assert_eq!(w.revision, 8);
            assert_eq!(w.window.as_ref().unwrap().window_id, "window-2");
        } else {
            panic!("expected window created, got {m3:?}");
        }

        // Send a command back.
        let viewport = ClientMessage::ViewportChanged(v1::ViewportChanged {
            window_id: "window-1".into(),
            width: 800,
            height: 600,
            scale: 2.0,
            is_final: true,
        });
        send.write_all(&viewport.encode()).await.unwrap();
        send.finish().ok();
        // Keep the connection alive so the command is actually transmitted
        // before the endpoint is dropped.
        tokio::time::sleep(Duration::from_millis(500)).await;
        "done"
    });
    assert_eq!(received, "done");

    let commands = core.join().unwrap();
    assert!(
        commands.iter().any(|c| matches!(
            c,
            aqua_server::net::ClientCommand::ViewportChanged { window_id, width: 800, height: 600, .. }
                if window_id == "window-1"
        )),
        "expected viewport command, got {commands:?}"
    );
}

#[test]
fn version_mismatch_is_rejected() {
    let (event_tx, _event_rx) = std::sync::mpsc::channel();
    let server = NetworkServer::spawn(config(0), event_tx, aqua_server::net::FrameHub::new())
        .expect("server");
    let addr = server.local_addr;
    let fingerprint = server.identity.fingerprint_sha256.clone();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let error = rt.block_on(async move {
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(quinn_client_config(&fingerprint));
        let connection = endpoint
            .connect(addr, "localhost")
            .unwrap()
            .await
            .expect("connect");
        let (mut send, mut recv) = connection.open_bi().await.expect("open_bi");

        let hello = ClientMessage::Hello(v1::ClientHello {
            protocol_version: 999,
            client_session_id: "client-test".into(),
            capabilities: None,
            client_name: "test".into(),
        });
        send.write_all(&hello.encode()).await.unwrap();

        match read_message(&mut recv).await {
            Some(ServerMessage::Hello(h)) => h.error,
            other => panic!("expected hello error, got {other:?}"),
        }
    });
    assert_eq!(error, "unsupported_protocol_version");
}
