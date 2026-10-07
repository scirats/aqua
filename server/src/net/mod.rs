//! QUIC transport for Aqua Protocol v1.
//!
//! The transport is deliberately isolated from the Wayland loop: it runs on a
//! dedicated Tokio runtime and communicates with calloop exclusively through
//! channels. See `docs/TRANSPORT.md`.

pub mod frames;
pub mod server;
pub mod tls;
pub mod video;

pub use frames::{FrameHub, FrameSink};
pub use server::{ClientCommand, NetworkConfig, NetworkServer, Outgoing, ServerEvent, TlsIdentity};
pub use video::{VideoConfig, VideoHub, VideoSink};
