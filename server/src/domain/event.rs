use super::ids::{RemoteApplicationId, RemoteSurfaceId, RemoteWindowId};
use super::surface::RemoteSurface;
use super::viewport::RemoteViewport;
use super::window::{RemoteWindow, RemoteWindowState};

/// Metadata about the buffer attached to a committed surface.
///
/// Phase 2 does not transmit pixels. We only describe what the client produced
/// so the future encoder phase knows what to expect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteBufferInfo {
    pub width: i32,
    pub height: i32,
    /// `true` for `wl_shm` buffers; `false` for anything else (e.g. dmabuf).
    pub shm: bool,
}

/// Neutral event stream produced by the domain.
///
/// The Wayland adapter emits these; a `RemoteEventSink` consumes them. Today the
/// sink logs with `tracing`; in phase 3 it will be a QUIC transport. The Wayland
/// code never needs to change when the sink changes.
#[derive(Debug, Clone, PartialEq)]
pub enum RemoteEvent {
    WindowCreated {
        window: RemoteWindow,
    },
    WindowTitleChanged {
        id: RemoteWindowId,
        title: Option<String>,
    },
    WindowAppIdChanged {
        id: RemoteWindowId,
        application_id: Option<RemoteApplicationId>,
    },
    WindowStateChanged {
        id: RemoteWindowId,
        state: RemoteWindowState,
    },
    WindowGeometryChanged {
        id: RemoteWindowId,
        min_size: Option<(i32, i32)>,
        max_size: Option<(i32, i32)>,
    },
    WindowMapped {
        id: RemoteWindowId,
        mapped: bool,
    },
    WindowClosed {
        id: RemoteWindowId,
    },
    SurfaceCreated {
        surface: RemoteSurface,
    },
    SurfaceUpdated {
        surface: RemoteSurface,
    },
    SurfaceCommitted {
        id: RemoteSurfaceId,
        window: RemoteWindowId,
        buffer: Option<RemoteBufferInfo>,
    },
    SurfaceDestroyed {
        id: RemoteSurfaceId,
        window: RemoteWindowId,
    },
    PopupCreated {
        id: RemoteSurfaceId,
        window: RemoteWindowId,
    },
    PopupDestroyed {
        id: RemoteSurfaceId,
        window: RemoteWindowId,
    },
    ViewportChanged {
        id: RemoteWindowId,
        viewport: RemoteViewport,
    },
}

impl RemoteEvent {
    /// Stable event name used for structured logging and tests.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::WindowCreated { .. } => "window.created",
            Self::WindowTitleChanged { .. } => "window.title_changed",
            Self::WindowAppIdChanged { .. } => "window.app_id_changed",
            Self::WindowStateChanged { .. } => "window.state_changed",
            Self::WindowGeometryChanged { .. } => "window.geometry_changed",
            Self::WindowMapped { .. } => "window.mapped",
            Self::WindowClosed { .. } => "window.closed",
            Self::SurfaceCreated { .. } => "surface.created",
            Self::SurfaceUpdated { .. } => "surface.updated",
            Self::SurfaceCommitted { .. } => "surface.commit",
            Self::SurfaceDestroyed { .. } => "surface.destroyed",
            Self::PopupCreated { .. } => "popup.created",
            Self::PopupDestroyed { .. } => "popup.destroyed",
            Self::ViewportChanged { .. } => "viewport.changed",
        }
    }

    pub fn window_id(&self) -> Option<&RemoteWindowId> {
        match self {
            Self::WindowCreated { window } => Some(&window.id),
            Self::WindowTitleChanged { id, .. }
            | Self::WindowAppIdChanged { id, .. }
            | Self::WindowStateChanged { id, .. }
            | Self::WindowGeometryChanged { id, .. }
            | Self::WindowMapped { id, .. }
            | Self::WindowClosed { id }
            | Self::ViewportChanged { id, .. } => Some(id),
            Self::SurfaceCreated { surface } => Some(&surface.window),
            Self::SurfaceUpdated { surface } => Some(&surface.window),
            Self::SurfaceCommitted { window, .. }
            | Self::SurfaceDestroyed { window, .. }
            | Self::PopupCreated { window, .. }
            | Self::PopupDestroyed { window, .. } => Some(window),
        }
    }
}
