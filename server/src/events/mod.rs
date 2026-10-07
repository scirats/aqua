//! Event sinks.
//!
//! The domain emits [`RemoteEvent`]s; a sink consumes them. Phase 2 ships a
//! `tracing` sink. Phase 3 will add a QUIC sink without touching the Wayland
//! adapter.

mod sink;
mod tracing_sink;

pub use sink::{RecordingSink, RemoteEventSink};
pub use tracing_sink::TracingEventSink;
