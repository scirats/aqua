//! Wayland/Smithay adapter.
//!
//! This is the "WAYLAND WORLD" side of the boundary. It owns every Smithay type
//! and translates Wayland lifecycles into neutral [`crate::domain::RemoteEvent`]s.
//! Nothing here leaks into the domain or the future transport.

pub mod compositor;
pub mod dmabuf;
pub mod input;
pub mod output;
pub mod seat;
pub mod xdg_shell;

mod state;

pub use state::{
    client_state, install_display_source, AquaState, CalloopData, ClientState, ControlMessage,
};
