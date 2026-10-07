//! Aqua Protocol v1.
//!
//! Wire format:
//!
//! ```text
//! frame := be32(type_tag) be32(payload_len) payload
//! payload := Protocol Buffers encoded body (schema: protocol/aqua.proto)
//! ```
//!
//! The type tag lives in the frame header, not the protobuf body, so an unknown
//! message type can be skipped safely by length (forward compatibility).

pub mod adapters;
pub mod data;
pub mod frame;
pub mod messages;
pub mod video;

/// Generated Protocol Buffers types from `protocol/aqua.proto` (package `aqua.v1`).
pub mod v1 {
    #![allow(clippy::all)]
    include!(concat!(env!("OUT_DIR"), "/aqua.v1.rs"));
}

pub use frame::{encode_frame, FrameError, HEADER_LEN};
pub use messages::{ClientMessage, ServerMessage};

/// Semantic protocol version carried in the handshake.
pub const PROTOCOL_VERSION: u32 = 1;

/// QUIC ALPN identifier.
pub const ALPN: &[u8] = b"aqua/1";

/// Capability bits supported by this build.
pub mod capability {
    pub const WINDOWS: u32 = 1 << 1;
    pub const POINTER: u32 = 1 << 2;
    pub const KEYBOARD: u32 = 1 << 3;
    pub const TOUCH: u32 = 1 << 4;
    pub const CLIPBOARD: u32 = 1 << 5;
    pub const DRAG_DROP: u32 = 1 << 6;
    pub const SURFACE_VIDEO: u32 = 1 << 7;
    pub const SURFACE_SHM: u32 = 1 << 8;
    pub const CURSOR: u32 = 1 << 9;

    /// Capabilities Aqua negotiates in phase 3A.
    pub const PHASE_3A: u32 = WINDOWS | POINTER | KEYBOARD | TOUCH;

    /// Phase 3B adds the raw SHM surface data plane.
    pub const PHASE_3B: u32 = PHASE_3A | SURFACE_SHM;

    /// Capabilities advertised by the server for a given build.
    ///
    /// `SURFACE_SHM` is always available (the 3B path is kept). `SURFACE_VIDEO`
    /// is advertised **only** when a real video encoder is installed, so the
    /// server never claims a capability it cannot deliver.
    pub fn advertised(has_video: bool) -> u32 {
        if has_video {
            PHASE_3B | SURFACE_VIDEO
        } else {
            PHASE_3B
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn shm_is_always_advertised() {
            assert_ne!(advertised(false) & SURFACE_SHM, 0);
            assert_ne!(advertised(true) & SURFACE_SHM, 0);
        }

        #[test]
        fn video_is_only_advertised_with_an_encoder() {
            assert_eq!(advertised(false) & SURFACE_VIDEO, 0);
            assert_ne!(advertised(true) & SURFACE_VIDEO, 0);
        }
    }
}
