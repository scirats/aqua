use std::fmt;

/// Stable identifier of a remote window for as long as its `xdg_toplevel` lives.
///
/// Never derived from the window title and never equal to a PID. A single client
/// process may own many `RemoteWindowId`s.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteWindowId(String);

impl RemoteWindowId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteWindowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifier of a remote application, currently mapped from `xdg_toplevel.app_id`.
///
/// Empty/unset app ids produce `None` at the window level; this type is only
/// constructed when the client actually reported one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteApplicationId(String);

impl RemoteApplicationId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RemoteApplicationId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque identifier for a single `wl_surface`.
///
/// The adapter assigns this; the domain never sees a `wl_surface`. It exists so
/// the registry can distinguish a toplevel's main surface from its subsurfaces
/// and popups without depending on Wayland.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RemoteSurfaceId(pub u64);

impl fmt::Display for RemoteSurfaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "surface-{}", self.0)
    }
}

/// Opaque identifier for a Wayland client connection.
///
/// A client can own multiple windows; on disconnect all of them are cleaned up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientKey(pub u64);

impl fmt::Display for ClientKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "client-{}", self.0)
    }
}
