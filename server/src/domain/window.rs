use super::ids::{RemoteApplicationId, RemoteWindowId};

/// High level state of a remote window.
///
/// # Wayland asymmetry (documented on purpose)
///
/// `xdg_toplevel` exposes `maximized` and `fullscreen` as *client states* that a
/// compositor can both request (via configure) and observe. `minimized` is **not**
/// part of `xdg_toplevel::State`: minimization is a compositor-internal concept
/// with no client-confirmed representation. We therefore model `Minimized` as a
/// compositor-side state only; it is never derived from a `xdg_toplevel` state
/// set and never sent as a confirmable state to the client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RemoteWindowState {
    #[default]
    Normal,
    Maximized,
    Fullscreen,
    /// Compositor-side only. See the type-level note.
    Minimized,
}

impl RemoteWindowState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Maximized => "maximized",
            Self::Fullscreen => "fullscreen",
            Self::Minimized => "minimized",
        }
    }
}

/// Neutral description of a remote window.
///
/// This is the unit of windowing, the conceptual equivalent of the Swift
/// `RemoteWindow` from phase 1. It maps 1:1 to an `xdg_toplevel` while it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteWindow {
    pub id: RemoteWindowId,
    pub application_id: Option<RemoteApplicationId>,
    pub title: Option<String>,
    pub min_size: Option<(i32, i32)>,
    pub max_size: Option<(i32, i32)>,
    pub state: RemoteWindowState,
    /// Whether a buffer is currently attached/committed by the client.
    pub mapped: bool,
}

impl RemoteWindow {
    pub fn new(id: RemoteWindowId, application_id: Option<RemoteApplicationId>) -> Self {
        Self {
            id,
            application_id,
            title: None,
            min_size: None,
            max_size: None,
            state: RemoteWindowState::Normal,
            mapped: false,
        }
    }
}
