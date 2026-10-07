use tracing::{debug, info};

use super::sink::RemoteEventSink;
use crate::domain::RemoteEvent;

/// Structured `tracing` sink. This is the phase 2 consumer.
///
/// Replace with a QUIC sink in phase 3; the Wayland adapter never changes.
#[derive(Debug, Default)]
pub struct TracingEventSink;

impl RemoteEventSink for TracingEventSink {
    fn handle(&mut self, event: &RemoteEvent) {
        match event {
            RemoteEvent::WindowCreated { window } => info!(
                target: "aqua::event",
                id = %window.id,
                app_id = ?window.application_id,
                title = ?window.title,
                "window.created"
            ),
            RemoteEvent::WindowTitleChanged { id, title } => info!(
                target: "aqua::event",
                id = %id,
                title = ?title,
                "window.title_changed"
            ),
            RemoteEvent::WindowAppIdChanged { id, application_id } => info!(
                target: "aqua::event",
                id = %id,
                app_id = ?application_id,
                "window.app_id_changed"
            ),
            RemoteEvent::WindowStateChanged { id, state } => info!(
                target: "aqua::event",
                id = %id,
                state = state.as_str(),
                "window.state_changed"
            ),
            RemoteEvent::WindowGeometryChanged {
                id,
                min_size,
                max_size,
            } => debug!(
                target: "aqua::event",
                id = %id,
                min = ?min_size,
                max = ?max_size,
                "window.geometry_changed"
            ),
            RemoteEvent::WindowMapped { id, mapped } => debug!(
                target: "aqua::event",
                id = %id,
                mapped = mapped,
                "window.mapped"
            ),
            RemoteEvent::WindowClosed { id } => info!(
                target: "aqua::event",
                id = %id,
                "window.closed"
            ),
            RemoteEvent::SurfaceCreated { surface } => debug!(
                target: "aqua::event",
                surface_id = %surface.id,
                window_id = %surface.window,
                kind = surface.kind.as_str(),
                parent = ?surface.parent,
                "surface.created"
            ),
            RemoteEvent::SurfaceUpdated { surface } => debug!(
                target: "aqua::event",
                surface_id = %surface.id,
                window_id = %surface.window,
                x = surface.position.0,
                y = surface.position.1,
                width = surface.size.0,
                height = surface.size.1,
                z = surface.z,
                "surface.updated"
            ),
            RemoteEvent::SurfaceCommitted { id, window, buffer } => debug!(
                target: "aqua::event",
                surface_id = %id,
                window_id = %window,
                width = buffer.map(|b| b.width),
                height = buffer.map(|b| b.height),
                shm = buffer.map(|b| b.shm),
                "surface.commit"
            ),
            RemoteEvent::SurfaceDestroyed { id, window } => debug!(
                target: "aqua::event",
                surface_id = %id,
                window_id = %window,
                "surface.destroyed"
            ),
            RemoteEvent::PopupCreated { id, window } => info!(
                target: "aqua::event",
                popup_id = %id,
                window_id = %window,
                "popup.created"
            ),
            RemoteEvent::PopupDestroyed { id, window } => info!(
                target: "aqua::event",
                popup_id = %id,
                window_id = %window,
                "popup.destroyed"
            ),
            RemoteEvent::ViewportChanged { id, viewport } => info!(
                target: "aqua::event",
                id = %id,
                width = viewport.width,
                height = viewport.height,
                scale = viewport.scale,
                "viewport.changed"
            ),
        }
    }
}
