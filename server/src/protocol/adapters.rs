//! Conversions between the Aqua domain model and the wire DTOs (`aqua.v1`).
//!
//! Kept separate on purpose: the domain never depends on generated types, and
//! the wire never depends on Smithay.

use super::v1;
use crate::domain::{RemoteSurface, RemoteSurfaceKind, RemoteWindow, RemoteWindowState};

pub fn state_to_u32(state: RemoteWindowState) -> u32 {
    match state {
        RemoteWindowState::Normal => 0,
        RemoteWindowState::Maximized => 1,
        RemoteWindowState::Fullscreen => 2,
        RemoteWindowState::Minimized => 3,
    }
}

pub fn u32_to_state(value: u32) -> RemoteWindowState {
    match value {
        1 => RemoteWindowState::Maximized,
        2 => RemoteWindowState::Fullscreen,
        3 => RemoteWindowState::Minimized,
        _ => RemoteWindowState::Normal,
    }
}

pub fn surface_kind_to_u32(kind: RemoteSurfaceKind) -> u32 {
    match kind {
        RemoteSurfaceKind::Toplevel => 1,
        RemoteSurfaceKind::Subsurface => 2,
        RemoteSurfaceKind::Popup => 3,
    }
}

pub fn u32_to_surface_kind(value: u32) -> RemoteSurfaceKind {
    match value {
        2 => RemoteSurfaceKind::Subsurface,
        3 => RemoteSurfaceKind::Popup,
        _ => RemoteSurfaceKind::Toplevel,
    }
}

/// Domain `RemoteSurface` -> wire `SurfaceInfo`.
pub fn surface_info(surface: &RemoteSurface) -> v1::SurfaceInfo {
    v1::SurfaceInfo {
        surface_id: surface.id.to_string(),
        window_id: surface.window.to_string(),
        parent_surface_id: surface.parent.map(|p| p.to_string()).unwrap_or_default(),
        role: surface_kind_to_u32(surface.kind),
        x: surface.position.0,
        y: surface.position.1,
        width: surface.size.0,
        height: surface.size.1,
        z: surface.z,
    }
}

/// Domain `RemoteWindow` -> wire `WindowInfo`.
pub fn window_info(window: &RemoteWindow) -> v1::WindowInfo {
    v1::WindowInfo {
        window_id: window.id.to_string(),
        application_id: window
            .application_id
            .as_ref()
            .map(|id| id.as_str().to_string())
            .unwrap_or_default(),
        title: window.title.clone().unwrap_or_default(),
        state: state_to_u32(window.state),
        mapped: window.mapped,
        has_min: window.min_size.is_some(),
        min_width: window.min_size.map(|s| s.0).unwrap_or(0),
        min_height: window.min_size.map(|s| s.1).unwrap_or(0),
        has_max: window.max_size.is_some(),
        max_width: window.max_size.map(|s| s.0).unwrap_or(0),
        max_height: window.max_size.map(|s| s.1).unwrap_or(0),
    }
}
