//! Aqua domain model.
//!
//! This module is intentionally free of any Wayland/Smithay types. It is the
//! "AQUA WORLD" side of the boundary:
//!
//! ```text
//!   WAYLAND WORLD              AQUA WORLD
//!   xdg_toplevel   ─adapter─▶  RemoteWindow
//!   wl_surface     ─adapter─▶  RemoteSurface
//!   wl_seat        ─adapter─▶  RemoteInputEvent
//! ```
//!
//! The registry (see [`registry::WindowRegistry`]) is a pure state machine and
//! is unit-tested without ever starting a compositor.

mod event;
mod ids;
mod registry;
mod surface;
mod viewport;
mod window;

pub use event::{RemoteBufferInfo, RemoteEvent};
pub use ids::{ClientKey, RemoteApplicationId, RemoteSurfaceId, RemoteWindowId};
pub use registry::WindowRegistry;
pub use surface::{RemoteSurface, RemoteSurfaceKind};
pub use viewport::RemoteViewport;
pub use window::{RemoteWindow, RemoteWindowState};
