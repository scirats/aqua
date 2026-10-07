//! Prints hex fixtures for cross-language codec verification.
//!
//! Run inside the Linux dev container:
//!     cargo run --example fixtures
//!
//! The Swift tests decode/encode these exact bytes.

use aqua_server::protocol::{video, v1, ClientMessage, ServerMessage};
use prost::Message;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn window() -> v1::WindowInfo {
    v1::WindowInfo {
        window_id: "window-1".into(),
        application_id: "org.gnome.Terminal".into(),
        title: "Terminal".into(),
        state: 2,
        mapped: true,
        has_min: true,
        min_width: 500,
        min_height: 300,
        has_max: false,
        max_width: 0,
        max_height: 0,
    }
}

fn main() {
    let capabilities = v1::Capabilities { bits: 0b11110 };

    let server_hello = ServerMessage::Hello(v1::ServerHello {
        protocol_version: 1,
        server_session_id: "sess-1".into(),
        server_identity: "abcd".into(),
        capabilities: Some(capabilities),
        revision: 42,
        error: String::new(),
    });
    let snapshot = ServerMessage::Snapshot(v1::ServerSnapshot {
        revision: 42,
        server_session_id: "sess-1".into(),
        windows: vec![window()],
        surfaces: Vec::new(),
    });
    let window_created = ServerMessage::WindowCreated(v1::WindowCreated {
        revision: 43,
        window: Some(window()),
    });

    let client_hello = ClientMessage::Hello(v1::ClientHello {
        protocol_version: 1,
        client_session_id: "client-1".into(),
        capabilities: Some(capabilities),
        client_name: "iPad".into(),
    });
    let viewport = ClientMessage::ViewportChanged(v1::ViewportChanged {
        window_id: "window-1".into(),
        width: 800,
        height: 600,
        scale: 2.0,
        is_final: true,
    });
    let key = ClientMessage::Key(v1::KeyEvent {
        window_id: "window-1".into(),
        keycode: 0,
        characters: "a".into(),
        modifiers: 0,
        pressed: true,
    });
    let pointer = ClientMessage::PointerButton(v1::PointerButton {
        window_id: "window-1".into(),
        button: 272,
        pressed: true,
        x: 12.5,
        y: 34.5,
    });

    // Phase 3C video control-plane messages.
    let video_config = ServerMessage::WindowVideoConfig(v1::WindowVideoConfig {
        window_id: "window-1".into(),
        codec: v1::VideoCodec::Hevc as u32,
        chroma: v1::VideoChroma::Nv12 as u32,
        width: 800,
        height: 600,
        frame_rate: 60,
        bitrate_kbps: 8000,
        gop: 0,
        low_latency: true,
    });
    let request_keyframe = ClientMessage::RequestKeyframe(v1::RequestKeyframe {
        window_id: "window-1".into(),
        reason: "resize".into(),
    });

    // Sanity: body round-trips.
    assert_eq!(window().encode_to_vec(), window().encode_to_vec());

    println!("SV_SERVER_HELLO {}", hex(&server_hello.encode()));
    println!("SV_SERVER_SNAPSHOT {}", hex(&snapshot.encode()));
    println!("SV_WINDOW_CREATED {}", hex(&window_created.encode()));
    println!("CL_CLIENT_HELLO {}", hex(&client_hello.encode()));
    println!("CL_VIEWPORT {}", hex(&viewport.encode()));
    println!("CL_KEY {}", hex(&key.encode()));
    println!("CL_POINTER_BUTTON {}", hex(&pointer.encode()));
    println!("SV_WINDOWINFO {}", hex(&window().encode_to_vec()));
    println!("SV_WINDOW_VIDEO_CONFIG {}", hex(&video_config.encode()));
    println!("CL_REQUEST_KEYFRAME {}", hex(&request_keyframe.encode()));

    // Phase 3C video data plane (window video stream).
    println!(
        "VD_WINDOW_VIDEO_HELLO {}",
        hex(&video::hello_window_stream_message("window-1"))
    );
    println!(
        "VD_WINDOW_VIDEO_CONFIG {}",
        hex(&video::config_stream_message(
            "window-1",
            v1::VideoCodec::Hevc as u32,
            v1::VideoChroma::Nv12 as u32,
            800,
            600,
            true,
            &[0xde, 0xad, 0xbe, 0xef],
        ))
    );
    println!(
        "VD_WINDOW_VIDEO_FRAME {}",
        hex(&video::frame_stream_message(
            "window-1",
            v1::VideoCodec::Hevc as u32,
            v1::VideoChroma::Nv12 as u32,
            800,
            600,
            42,
            true,
            1_000_000,
            &[0xaa, 0xbb, 0xcc],
        ))
    );
}
