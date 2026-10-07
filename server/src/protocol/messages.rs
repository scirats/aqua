//! Tagged Aqua messages. Payloads are Protocol Buffers bodies from `aqua.v1`.

use bytes::Bytes;
use prost::Message;

use super::frame::encode_frame;
use super::v1;

// Server -> Client tags
pub const T_SERVER_HELLO: u32 = 1;
pub const T_SERVER_SNAPSHOT: u32 = 2;
pub const T_WINDOW_CREATED: u32 = 3;
pub const T_WINDOW_UPDATED: u32 = 4;
pub const T_WINDOW_TITLE_CHANGED: u32 = 5;
pub const T_WINDOW_APPLICATION_CHANGED: u32 = 6;
pub const T_WINDOW_STATE_CHANGED: u32 = 7;
pub const T_WINDOW_MAPPED: u32 = 8;
pub const T_WINDOW_CLOSED: u32 = 9;
pub const T_SURFACE_CREATED: u32 = 10;
pub const T_SURFACE_DESTROYED: u32 = 11;
pub const T_SURFACE_UPDATED: u32 = 13;
pub const T_PONG: u32 = 12;
pub const T_WINDOW_VIDEO_CONFIG: u32 = 14;

// Client -> Server tags
pub const T_CLIENT_HELLO: u32 = 40;
pub const T_VIEWPORT_CHANGED: u32 = 41;
pub const T_WINDOW_FOCUS_REQUESTED: u32 = 42;
pub const T_POINTER_MOVED: u32 = 43;
pub const T_POINTER_BUTTON: u32 = 44;
pub const T_POINTER_SCROLL: u32 = 45;
pub const T_KEY_EVENT: u32 = 46;
pub const T_TOUCH_EVENT: u32 = 47;
pub const T_PING: u32 = 48;
pub const T_FRAME_PRESENTED: u32 = 49;
pub const T_REQUEST_KEYFRAME: u32 = 50;

#[derive(Debug, Clone, PartialEq)]
pub enum ServerMessage {
    Hello(v1::ServerHello),
    Snapshot(v1::ServerSnapshot),
    WindowCreated(v1::WindowCreated),
    WindowUpdated(v1::WindowUpdated),
    WindowTitleChanged(v1::WindowTitleChanged),
    WindowApplicationChanged(v1::WindowApplicationChanged),
    WindowStateChanged(v1::WindowStateChanged),
    WindowMapped(v1::WindowMapped),
    WindowClosed(v1::WindowClosed),
    SurfaceCreated(v1::SurfaceCreated),
    SurfaceUpdated(v1::SurfaceUpdated),
    SurfaceDestroyed(v1::SurfaceDestroyed),
    WindowVideoConfig(v1::WindowVideoConfig),
    Pong(v1::Pong),
}

impl ServerMessage {
    pub fn tag(&self) -> u32 {
        match self {
            Self::Hello(_) => T_SERVER_HELLO,
            Self::Snapshot(_) => T_SERVER_SNAPSHOT,
            Self::WindowCreated(_) => T_WINDOW_CREATED,
            Self::WindowUpdated(_) => T_WINDOW_UPDATED,
            Self::WindowTitleChanged(_) => T_WINDOW_TITLE_CHANGED,
            Self::WindowApplicationChanged(_) => T_WINDOW_APPLICATION_CHANGED,
            Self::WindowStateChanged(_) => T_WINDOW_STATE_CHANGED,
            Self::WindowMapped(_) => T_WINDOW_MAPPED,
            Self::WindowClosed(_) => T_WINDOW_CLOSED,
            Self::SurfaceCreated(_) => T_SURFACE_CREATED,
            Self::SurfaceUpdated(_) => T_SURFACE_UPDATED,
            Self::SurfaceDestroyed(_) => T_SURFACE_DESTROYED,
            Self::WindowVideoConfig(_) => T_WINDOW_VIDEO_CONFIG,
            Self::Pong(_) => T_PONG,
        }
    }

    fn body(&self) -> Vec<u8> {
        match self {
            Self::Hello(m) => m.encode_to_vec(),
            Self::Snapshot(m) => m.encode_to_vec(),
            Self::WindowCreated(m) => m.encode_to_vec(),
            Self::WindowUpdated(m) => m.encode_to_vec(),
            Self::WindowTitleChanged(m) => m.encode_to_vec(),
            Self::WindowApplicationChanged(m) => m.encode_to_vec(),
            Self::WindowStateChanged(m) => m.encode_to_vec(),
            Self::WindowMapped(m) => m.encode_to_vec(),
            Self::WindowClosed(m) => m.encode_to_vec(),
            Self::SurfaceCreated(m) => m.encode_to_vec(),
            Self::SurfaceUpdated(m) => m.encode_to_vec(),
            Self::SurfaceDestroyed(m) => m.encode_to_vec(),
            Self::WindowVideoConfig(m) => m.encode_to_vec(),
            Self::Pong(m) => m.encode_to_vec(),
        }
    }

    /// Encode into a full frame.
    pub fn encode(&self) -> Bytes {
        encode_frame(self.tag(), &self.body())
    }

    /// Decode a frame payload by tag. Returns `None` for unknown tags
    /// (forward compatible: ignore and continue).
    pub fn decode(tag: u32, payload: &[u8]) -> Option<Self> {
        match tag {
            T_SERVER_HELLO => v1::ServerHello::decode(payload).ok().map(Self::Hello),
            T_SERVER_SNAPSHOT => v1::ServerSnapshot::decode(payload).ok().map(Self::Snapshot),
            T_WINDOW_CREATED => v1::WindowCreated::decode(payload)
                .ok()
                .map(Self::WindowCreated),
            T_WINDOW_UPDATED => v1::WindowUpdated::decode(payload)
                .ok()
                .map(Self::WindowUpdated),
            T_WINDOW_TITLE_CHANGED => v1::WindowTitleChanged::decode(payload)
                .ok()
                .map(Self::WindowTitleChanged),
            T_WINDOW_APPLICATION_CHANGED => v1::WindowApplicationChanged::decode(payload)
                .ok()
                .map(Self::WindowApplicationChanged),
            T_WINDOW_STATE_CHANGED => v1::WindowStateChanged::decode(payload)
                .ok()
                .map(Self::WindowStateChanged),
            T_WINDOW_MAPPED => v1::WindowMapped::decode(payload)
                .ok()
                .map(Self::WindowMapped),
            T_WINDOW_CLOSED => v1::WindowClosed::decode(payload)
                .ok()
                .map(Self::WindowClosed),
            T_SURFACE_CREATED => v1::SurfaceCreated::decode(payload)
                .ok()
                .map(Self::SurfaceCreated),
            T_SURFACE_UPDATED => v1::SurfaceUpdated::decode(payload)
                .ok()
                .map(Self::SurfaceUpdated),
            T_SURFACE_DESTROYED => v1::SurfaceDestroyed::decode(payload)
                .ok()
                .map(Self::SurfaceDestroyed),
            T_WINDOW_VIDEO_CONFIG => v1::WindowVideoConfig::decode(payload)
                .ok()
                .map(Self::WindowVideoConfig),
            T_PONG => v1::Pong::decode(payload).ok().map(Self::Pong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClientMessage {
    Hello(v1::ClientHello),
    ViewportChanged(v1::ViewportChanged),
    WindowFocusRequested(v1::WindowFocusRequested),
    PointerMoved(v1::PointerMoved),
    PointerButton(v1::PointerButton),
    PointerScroll(v1::PointerScroll),
    Key(v1::KeyEvent),
    Touch(v1::TouchEvent),
    Ping(v1::Ping),
    FramePresented(v1::FramePresented),
    RequestKeyframe(v1::RequestKeyframe),
}

impl ClientMessage {
    pub fn tag(&self) -> u32 {
        match self {
            Self::Hello(_) => T_CLIENT_HELLO,
            Self::ViewportChanged(_) => T_VIEWPORT_CHANGED,
            Self::WindowFocusRequested(_) => T_WINDOW_FOCUS_REQUESTED,
            Self::PointerMoved(_) => T_POINTER_MOVED,
            Self::PointerButton(_) => T_POINTER_BUTTON,
            Self::PointerScroll(_) => T_POINTER_SCROLL,
            Self::Key(_) => T_KEY_EVENT,
            Self::Touch(_) => T_TOUCH_EVENT,
            Self::Ping(_) => T_PING,
            Self::FramePresented(_) => T_FRAME_PRESENTED,
            Self::RequestKeyframe(_) => T_REQUEST_KEYFRAME,
        }
    }

    fn body(&self) -> Vec<u8> {
        match self {
            Self::Hello(m) => m.encode_to_vec(),
            Self::ViewportChanged(m) => m.encode_to_vec(),
            Self::WindowFocusRequested(m) => m.encode_to_vec(),
            Self::PointerMoved(m) => m.encode_to_vec(),
            Self::PointerButton(m) => m.encode_to_vec(),
            Self::PointerScroll(m) => m.encode_to_vec(),
            Self::Key(m) => m.encode_to_vec(),
            Self::Touch(m) => m.encode_to_vec(),
            Self::Ping(m) => m.encode_to_vec(),
            Self::FramePresented(m) => m.encode_to_vec(),
            Self::RequestKeyframe(m) => m.encode_to_vec(),
        }
    }

    pub fn encode(&self) -> Bytes {
        encode_frame(self.tag(), &self.body())
    }

    pub fn decode(tag: u32, payload: &[u8]) -> Option<Self> {
        match tag {
            T_CLIENT_HELLO => v1::ClientHello::decode(payload).ok().map(Self::Hello),
            T_VIEWPORT_CHANGED => v1::ViewportChanged::decode(payload)
                .ok()
                .map(Self::ViewportChanged),
            T_WINDOW_FOCUS_REQUESTED => v1::WindowFocusRequested::decode(payload)
                .ok()
                .map(Self::WindowFocusRequested),
            T_POINTER_MOVED => v1::PointerMoved::decode(payload)
                .ok()
                .map(Self::PointerMoved),
            T_POINTER_BUTTON => v1::PointerButton::decode(payload)
                .ok()
                .map(Self::PointerButton),
            T_POINTER_SCROLL => v1::PointerScroll::decode(payload)
                .ok()
                .map(Self::PointerScroll),
            T_KEY_EVENT => v1::KeyEvent::decode(payload).ok().map(Self::Key),
            T_TOUCH_EVENT => v1::TouchEvent::decode(payload).ok().map(Self::Touch),
            T_PING => v1::Ping::decode(payload).ok().map(Self::Ping),
            T_FRAME_PRESENTED => v1::FramePresented::decode(payload)
                .ok()
                .map(Self::FramePresented),
            T_REQUEST_KEYFRAME => v1::RequestKeyframe::decode(payload)
                .ok()
                .map(Self::RequestKeyframe),
            _ => None,
        }
    }
}
