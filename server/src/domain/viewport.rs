/// Viewport of a remote window, in logical units.
///
/// Mirrors the Swift `RemoteViewport` from phase 1. This is what the iPad will
/// eventually send as a resize request; on the Linux side it becomes an
/// `xdg_toplevel` configure request.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RemoteViewport {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
}

impl RemoteViewport {
    pub fn new(width: i32, height: i32, scale: f64) -> Self {
        Self {
            width,
            height,
            scale,
        }
    }
}
