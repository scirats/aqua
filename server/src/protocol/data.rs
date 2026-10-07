//! Aqua data plane: owned SHM frames and their surface-stream encoding.
//!
//! Frames never become `RemoteEvent`s and are never put inside the control
//! stream. Each `RemoteSurface` gets its own unidirectional QUIC stream carrying
//! a sequence of `[be32 header_len][SurfaceStreamHeader][payload_len raw bytes]`
//! messages (see `protocol/aqua.proto`).

use bytes::Bytes;
use prost::Message;

use super::frame;
use super::v1;

/// Wayland `wl_shm` DRM-style formats supported in phase 3B.
pub const FORMAT_ARGB8888: u32 = 1;
pub const FORMAT_XRGB8888: u32 = 2;

/// Safety bounds. The protocol never trusts remote dimensions.
pub const MAX_DIMENSION: u32 = 8192;
pub const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

/// A damage rectangle in buffer coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// An owned copy of a committed SHM surface buffer.
///
/// The bytes are copied out of the client's mmap while the buffer is pinned, so
/// there are no borrowed references to client memory.
#[derive(Debug, Clone)]
pub struct SurfaceFrame {
    pub surface_id: String,
    pub window_id: String,
    pub frame_id: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub format: u32,
    pub damage: Vec<DamageRect>,
    pub data: Vec<u8>,
}

impl SurfaceFrame {
    pub fn payload_len(&self) -> usize {
        self.data.len()
    }
}

/// Validate a set of SHM buffer dimensions. Returns the expected byte length.
///
/// Rejects zero sizes, absurd dimensions and `stride * height` overflow, and
/// enforces the global frame-size limit (defence against a buggy/malicious
/// Wayland client).
pub fn validate_shm(
    width: u32,
    height: u32,
    stride: u32,
    buffer_len: usize,
) -> Result<usize, FrameValidationError> {
    if width == 0 || height == 0 {
        return Err(FrameValidationError::ZeroSize);
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(FrameValidationError::TooLarge);
    }
    let expected = (stride as usize)
        .checked_mul(height as usize)
        .ok_or(FrameValidationError::Overflow)?;
    if (stride as u64) < (width as u64) * 4 {
        return Err(FrameValidationError::StrideTooSmall);
    }
    if expected > MAX_FRAME_BYTES {
        return Err(FrameValidationError::TooLarge);
    }
    if expected > buffer_len {
        return Err(FrameValidationError::ShortBuffer);
    }
    Ok(expected)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameValidationError {
    ZeroSize,
    TooLarge,
    Overflow,
    StrideTooSmall,
    ShortBuffer,
}

impl std::fmt::Display for FrameValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ZeroSize => "zero-sized buffer",
            Self::TooLarge => "buffer too large",
            Self::Overflow => "stride*height overflow",
            Self::StrideTooSmall => "stride smaller than width*4",
            Self::ShortBuffer => "buffer shorter than stride*height",
        })
    }
}

/// First message on a surface stream: identifies the surface.
pub fn hello_stream_message(surface_id: &str, window_id: &str) -> Bytes {
    let header = v1::SurfaceStreamHeader {
        kind: v1::SurfaceStreamKind::SurfaceStreamHello as u32,
        surface_id: surface_id.to_string(),
        window_id: window_id.to_string(),
        ..Default::default()
    };
    frame::encode_stream_message(&header.encode_to_vec(), &[])
}

/// A frame message: header + raw pixels.
pub fn frame_stream_message(frame_data: &SurfaceFrame) -> Bytes {
    let header = v1::SurfaceStreamHeader {
        kind: v1::SurfaceStreamKind::SurfaceStreamFrame as u32,
        surface_id: frame_data.surface_id.clone(),
        window_id: frame_data.window_id.clone(),
        frame_id: frame_data.frame_id,
        width: frame_data.width,
        height: frame_data.height,
        stride: frame_data.stride,
        format: frame_data.format,
        payload_len: frame_data.data.len() as u64,
        damage: frame_data
            .damage
            .iter()
            .map(|d| v1::DamageRect {
                x: d.x,
                y: d.y,
                width: d.width,
                height: d.height,
            })
            .collect(),
    };
    frame::encode_stream_message(&header.encode_to_vec(), &frame_data.data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_accepts_valid_geometry() {
        assert_eq!(validate_shm(4, 2, 16, 32), Ok(32));
        // Extra trailing space is allowed (stride padding).
        assert_eq!(validate_shm(3, 2, 16, 64), Ok(32));
    }

    #[test]
    fn validate_rejects_zero_size() {
        assert_eq!(
            validate_shm(0, 2, 16, 32),
            Err(FrameValidationError::ZeroSize)
        );
        assert_eq!(
            validate_shm(4, 0, 16, 32),
            Err(FrameValidationError::ZeroSize)
        );
    }

    #[test]
    fn validate_rejects_small_stride() {
        assert_eq!(
            validate_shm(4, 2, 8, 64),
            Err(FrameValidationError::StrideTooSmall)
        );
    }

    #[test]
    fn validate_rejects_short_buffer() {
        assert_eq!(
            validate_shm(4, 2, 16, 8),
            Err(FrameValidationError::ShortBuffer)
        );
    }

    #[test]
    fn validate_rejects_huge_frame() {
        // 8192x8192 at 4 bpp = 256 MiB > MAX_FRAME_BYTES.
        assert_eq!(
            validate_shm(8192, 8192, 8192 * 4, 8192 * 4 * 8192),
            Err(FrameValidationError::TooLarge)
        );
    }

    #[test]
    fn validate_rejects_dimension_over_max() {
        assert_eq!(
            validate_shm(MAX_DIMENSION + 1, 1, (MAX_DIMENSION + 1) * 4, 1),
            Err(FrameValidationError::TooLarge)
        );
    }

    #[test]
    fn frame_stream_message_round_trips_header() {
        let surface = SurfaceFrame {
            surface_id: "surface-2".into(),
            window_id: "window-1".into(),
            frame_id: 7,
            width: 4,
            height: 2,
            stride: 16,
            format: FORMAT_XRGB8888,
            damage: vec![DamageRect {
                x: 1,
                y: 2,
                width: 3,
                height: 4,
            }],
            data: vec![0xab; 32],
        };
        let bytes = frame_stream_message(&surface);
        assert!(bytes.len() > 4 + 32);
        let header_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let header =
            v1::SurfaceStreamHeader::decode(&bytes[4..4 + header_len]).expect("decode header");
        assert_eq!(
            header.kind,
            v1::SurfaceStreamKind::SurfaceStreamFrame as u32
        );
        assert_eq!(header.surface_id, "surface-2");
        assert_eq!(header.window_id, "window-1");
        assert_eq!(header.frame_id, 7);
        assert_eq!(header.width, 4);
        assert_eq!(header.height, 2);
        assert_eq!(header.stride, 16);
        assert_eq!(header.format, FORMAT_XRGB8888);
        assert_eq!(header.payload_len, 32);
        assert_eq!(header.damage.len(), 1);
        // Raw payload follows the header unchanged.
        let payload = &bytes[4 + header_len..];
        assert_eq!(payload.len(), 32);
        assert!(payload.iter().all(|b| *b == 0xab));
    }

    #[test]
    fn hello_stream_message_identifies_surface() {
        let bytes = hello_stream_message("surface-1", "window-1");
        let header_len = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
        let header =
            v1::SurfaceStreamHeader::decode(&bytes[4..4 + header_len]).expect("decode header");
        assert_eq!(
            header.kind,
            v1::SurfaceStreamKind::SurfaceStreamHello as u32
        );
        assert_eq!(header.surface_id, "surface-1");
        assert_eq!(header.window_id, "window-1");
        assert_eq!(bytes.len(), 4 + header_len);
    }
}
