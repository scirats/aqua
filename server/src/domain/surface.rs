use super::ids::{RemoteSurfaceId, RemoteWindowId};

/// What a `wl_surface` represents inside a window's surface tree.
///
/// This preserves the critical distinction that `wl_surface != RemoteWindow`:
/// a single window owns a main surface plus any number of subsurfaces and
/// popups.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemoteSurfaceKind {
    /// The main surface of an `xdg_toplevel`.
    Toplevel,
    /// A `wl_subsurface` (part of the same window, never its own window).
    Subsurface,
    /// An `xdg_popup` (menu, tooltip, dropdown, ...); belongs to a window.
    Popup,
}

impl RemoteSurfaceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Toplevel => "toplevel",
            Self::Subsurface => "subsurface",
            Self::Popup => "popup",
        }
    }
}

/// A single surface inside a window's tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteSurface {
    pub id: RemoteSurfaceId,
    pub window: RemoteWindowId,
    pub parent: Option<RemoteSurfaceId>,
    pub kind: RemoteSurfaceKind,
    /// Position relative to the parent surface, in logical units.
    pub position: (i32, i32),
    /// Surface size in logical units (from the last committed buffer).
    pub size: (u32, u32),
    /// Stacking order among siblings (0 == bottom).
    pub z: u32,
}

impl RemoteSurface {
    pub fn new(
        id: RemoteSurfaceId,
        window: RemoteWindowId,
        parent: Option<RemoteSurfaceId>,
        kind: RemoteSurfaceKind,
    ) -> Self {
        Self {
            id,
            window,
            parent,
            kind,
            position: (0, 0),
            size: (0, 0),
            z: 0,
        }
    }
}
