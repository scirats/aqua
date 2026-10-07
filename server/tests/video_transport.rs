//! Localhost video-plane test: real QUIC, server-opened unidirectional stream
//! per window, HELLO -> CONFIG -> FRAME in wire order.

use std::{sync::Arc, time::Duration};

use bytes::Bytes;
use prost::Message;

use aqua_server::gpu::{EncodedFrame, VideoChroma, VideoCodec};
use aqua_server::net::tls;
use aqua_server::net::{NetworkConfig, NetworkServer, VideoConfig, VideoHub, VideoSink};
use aqua_server::protocol::v1;

const CERT: &[u8] = include_bytes!("../certs/dev-cert.pem");
const KEY: &[u8] = include_bytes!("../certs/dev-key.pem");

fn config(port: u16) -> NetworkConfig {
    NetworkConfig {
        bind: format!("127.0.0.1:{port}").parse().unwrap(),
        cert_pem: CERT.to_vec(),
        key_pem: KEY.to_vec(),
    }
}

fn quinn_client_config(fingerprint: &str) -> quinn::ClientConfig {
    let rustls = tls::client_config(fingerprint).expect("client tls");
    let quic = quinn::crypto::rustls::QuicClientConfig::try_from(rustls).expect("quic client");
    quinn::ClientConfig::new(Arc::new(quic))
}

async fn read_video_message(recv: &mut quinn::RecvStream) -> (v1::WindowVideoStreamHeader, Vec<u8>) {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.expect("header len");
    let header_len = u32::from_be_bytes(len) as usize;
    let mut header = vec![0u8; header_len];
    recv.read_exact(&mut header).await.expect("header");
    let decoded = v1::WindowVideoStreamHeader::decode(&header[..]).expect("decode header");
    let mut payload = vec![0u8; decoded.payload_len as usize];
    if !payload.is_empty() {
        recv.read_exact(&mut payload).await.expect("payload");
    }
    (decoded, payload)
}

#[test]
fn window_video_stream_hello_config_frames() {
    let (event_tx, _event_rx) = std::sync::mpsc::channel();
    let hub = VideoHub::with_capacity(8);
    let server = NetworkServer::spawn(
        config(0),
        event_tx,
        aqua_server::net::FrameHub::new(),
        hub.clone(),
    )
    .expect("server");
    let addr = server.local_addr;
    let fingerprint = server.identity.fingerprint_sha256.clone();

    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async move {
        let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().unwrap()).unwrap();
        endpoint.set_default_client_config(quinn_client_config(&fingerprint));
        let connection = endpoint
            .connect(addr, "localhost")
            .unwrap()
            .await
            .expect("connect");

        let codec_config = vec![0x00, 0x00, 0x00, 0x01, 0x40, 0x01];
        hub.submit_config(
            "window-1",
            VideoConfig {
                codec: VideoCodec::Hevc,
                chroma: VideoChroma::Nv12,
                width: 800,
                height: 600,
                codec_config: codec_config.clone(),
            },
        );

        let mut recv = tokio::time::timeout(Duration::from_secs(5), connection.accept_uni())
            .await
            .expect("accept_uni timeout")
            .expect("accept_uni");

        let (hello, _) = read_video_message(&mut recv).await;
        assert_eq!(
            hello.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamHello as u32
        );
        assert_eq!(hello.window_id, "window-1");
        assert_eq!(
            hello.stream_type,
            v1::DataStreamType::DataStreamWindowVideo as u32
        );

        let (config, payload) = read_video_message(&mut recv).await;
        assert_eq!(
            config.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamConfig as u32
        );
        assert!(config.codec_config);
        assert_eq!(config.codec, v1::VideoCodec::Hevc as u32);
        assert_eq!(config.chroma, v1::VideoChroma::Nv12 as u32);
        assert_eq!(config.width, 800);
        assert_eq!(payload, codec_config);

        hub.submit_frame(EncodedFrame {
            window_id: "window-1".into(),
            frame_id: 1,
            keyframe: true,
            codec_config: false,
            pts_us: 0,
            data: Bytes::from_static(&[0x00, 0x00, 0x00, 0x01, 0x26, 0x01]),
        });
        hub.submit_frame(EncodedFrame {
            window_id: "window-1".into(),
            frame_id: 2,
            keyframe: false,
            codec_config: false,
            pts_us: 16_666,
            data: Bytes::from_static(&[0x00, 0x00, 0x00, 0x01, 0x02, 0x01]),
        });

        let (f1, _) = read_video_message(&mut recv).await;
        assert_eq!(
            f1.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamFrame as u32
        );
        assert_eq!(f1.frame_id, 1);
        assert!(f1.keyframe);
        assert_eq!(f1.pts_us, 0);

        let (f2, _) = read_video_message(&mut recv).await;
        assert_eq!(
            f2.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamFrame as u32
        );
        assert_eq!(f2.frame_id, 2);
        assert!(!f2.keyframe);
        assert_eq!(f2.pts_us, 16_666);
    });
}
