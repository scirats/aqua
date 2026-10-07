use bytes::{BufMut, Bytes, BytesMut};
use std::fmt;

/// Fixed frame header size: `be32(type_tag)` + `be32(payload_len)`.
pub const HEADER_LEN: usize = 8;

/// Maximum accepted payload length (sanity bound, 8 MiB).
pub const MAX_PAYLOAD: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameError {
    /// Not enough bytes yet to decode a full frame.
    Incomplete,
    /// Declared payload length exceeds [`MAX_PAYLOAD`].
    TooLarge,
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Incomplete => f.write_str("incomplete frame"),
            Self::TooLarge => f.write_str("frame payload too large"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Encode a frame: header + payload.
pub fn encode_frame(type_tag: u32, payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(HEADER_LEN + payload.len());
    buf.put_u32(type_tag);
    buf.put_u32(payload.len() as u32);
    buf.extend_from_slice(payload);
    buf.freeze()
}

/// Try to decode a frame header from the start of `buf`.
///
/// Returns the type tag and payload length. The caller then consumes
/// `HEADER_LEN + len` bytes.
pub fn decode_header(buf: &[u8]) -> Result<(u32, usize), FrameError> {
    if buf.len() < HEADER_LEN {
        return Err(FrameError::Incomplete);
    }
    let tag = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let len = u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]]) as usize;
    if len > MAX_PAYLOAD {
        return Err(FrameError::TooLarge);
    }
    Ok((tag, len))
}

/// Split an 8-byte header into `(tag, len)`.
pub fn split_header(header: &[u8; HEADER_LEN]) -> (u32, usize) {
    let tag = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let len = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
    (tag, len)
}

/// Maximum accepted data-plane (surface stream) protobuf header size.
pub const MAX_STREAM_HEADER: usize = 64 * 1024;

/// Data-plane message for surface streams:
///
/// ```text
/// [be32 header_len][SurfaceStreamHeader protobuf][payload_len raw bytes]
/// ```
///
/// `payload` is raw pixel data (no protobuf framing for the bytes themselves).
pub fn encode_stream_message(header: &[u8], payload: &[u8]) -> Bytes {
    let mut buf = BytesMut::with_capacity(4 + header.len() + payload.len());
    buf.put_u32(header.len() as u32);
    buf.extend_from_slice(header);
    buf.extend_from_slice(payload);
    buf.freeze()
}

/// Read a `be32` length prefix from the first 4 bytes.
pub fn split_stream_header_length(buf: &[u8; 4]) -> usize {
    u32::from_be_bytes(*buf) as usize
}
