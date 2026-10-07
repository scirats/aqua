//! Aqua video data plane: window video stream messages (phase 3C).
//!
//! One encoded stream per `RemoteWindow` (NOT per surface). Each stream is a
//! sequence of `[be32 header_len][WindowVideoStreamHeader][payload raw bytes]`
//! messages, mirroring the SHM surface-stream shape but carrying an inter-frame
//! bitstream instead of raw pixels.
//!
//! This module owns the wire contract only; the encoder itself lives behind
//! `crate::gpu::VideoEncoder`.

use bytes::Bytes;
use prost::Message;

use super::frame;
use super::v1;

/// First message on a window video stream: identifies the window.
pub fn hello_window_stream_message(window_id: &str) -> Bytes {
    let header = v1::WindowVideoStreamHeader {
        kind: v1::WindowVideoStreamKind::WindowVideoStreamHello as u32,
        window_id: window_id.to_string(),
        ..Default::default()
    };
    frame::encode_stream_message(&header.encode_to_vec(), &[])
}

/// A `CONFIG` message: codec/chroma/size and, optionally, a codec-configuration
/// payload (VPS/SPS/PPS) that carries no picture.
#[allow(clippy::too_many_arguments)]
pub fn config_stream_message(
    window_id: &str,
    codec: u32,
    chroma: u32,
    width: u32,
    height: u32,
    codec_config: bool,
    payload: &[u8],
) -> Bytes {
    let header = v1::WindowVideoStreamHeader {
        kind: v1::WindowVideoStreamKind::WindowVideoStreamConfig as u32,
        window_id: window_id.to_string(),
        codec,
        chroma,
        width,
        height,
        payload_len: payload.len() as u64,
        codec_config,
        ..Default::default()
    };
    frame::encode_stream_message(&header.encode_to_vec(), payload)
}

/// A `FRAME` message: one access unit.
#[allow(clippy::too_many_arguments)]
pub fn frame_stream_message(
    window_id: &str,
    codec: u32,
    chroma: u32,
    width: u32,
    height: u32,
    frame_id: u64,
    keyframe: bool,
    pts_us: u64,
    payload: &[u8],
) -> Bytes {
    let header = v1::WindowVideoStreamHeader {
        kind: v1::WindowVideoStreamKind::WindowVideoStreamFrame as u32,
        window_id: window_id.to_string(),
        codec,
        chroma,
        width,
        height,
        frame_id,
        keyframe,
        pts_us,
        payload_len: payload.len() as u64,
        codec_config: false,
    };
    frame::encode_stream_message(&header.encode_to_vec(), payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(bytes: &Bytes) -> v1::WindowVideoStreamHeader {
        let header_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        v1::WindowVideoStreamHeader::decode(&bytes[4..4 + header_len]).expect("decode header")
    }

    #[test]
    fn hello_identifies_window() {
        let bytes = hello_window_stream_message("window-7");
        let header = decode(&bytes);
        assert_eq!(
            header.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamHello as u32
        );
        assert_eq!(header.window_id, "window-7");
        assert_eq!(bytes.len(), 4 + header.encoded_len());
    }

    #[test]
    fn config_carries_codec_config_flag_and_payload() {
        let payload = [0xde, 0xad, 0xbe, 0xef];
        let bytes = config_stream_message(
            "window-1",
            v1::VideoCodec::Hevc as u32,
            v1::VideoChroma::Nv12 as u32,
            800,
            600,
            true,
            &payload,
        );
        let header = decode(&bytes);
        assert_eq!(
            header.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamConfig as u32
        );
        assert_eq!(header.codec, v1::VideoCodec::Hevc as u32);
        assert_eq!(header.chroma, v1::VideoChroma::Nv12 as u32);
        assert_eq!(header.width, 800);
        assert_eq!(header.height, 600);
        assert!(header.codec_config);
        assert_eq!(header.payload_len, 4);
        let payload_offset = 4 + header.encoded_len();
        assert_eq!(&bytes[payload_offset..], &payload);
    }

    #[test]
    fn frame_carries_access_unit_metadata() {
        let payload = vec![0xaa; 10];
        let bytes = frame_stream_message(
            "window-2",
            v1::VideoCodec::H264 as u32,
            v1::VideoChroma::Nv12 as u32,
            1024,
            768,
            42,
            true,
            1_000_000,
            &payload,
        );
        let header = decode(&bytes);
        assert_eq!(
            header.kind,
            v1::WindowVideoStreamKind::WindowVideoStreamFrame as u32
        );
        assert_eq!(header.frame_id, 42);
        assert!(header.keyframe);
        assert_eq!(header.pts_us, 1_000_000);
        assert!(!header.codec_config);
        assert_eq!(header.payload_len, 10);
        let payload_offset = 4 + header.encoded_len();
        assert_eq!(&bytes[payload_offset..], &payload[..]);
    }
}
