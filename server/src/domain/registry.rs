use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::event::{RemoteBufferInfo, RemoteEvent};
use super::ids::{ClientKey, RemoteApplicationId, RemoteSurfaceId, RemoteWindowId};
use super::surface::{RemoteSurface, RemoteSurfaceKind};
use super::viewport::RemoteViewport;
use super::window::{RemoteWindow, RemoteWindowState};

/// Pure state machine mapping surface/toplevel lifecycles to `RemoteWindow`s.
///
/// Contains no Wayland or Smithay types. Everything here is driven by
/// `RemoteSurfaceId`/`ClientKey` tokens supplied by the adapter, which makes the
/// whole window/surface lifecycle testable without a compositor.
#[derive(Debug, Default)]
pub struct WindowRegistry {
    windows: BTreeMap<RemoteWindowId, RemoteWindow>,
    /// Root surface of each `xdg_toplevel`.
    toplevel_surface: HashMap<RemoteSurfaceId, RemoteWindowId>,
    /// Every surface (toplevel main / subsurface / popup) -> owning window.
    surface_window: HashMap<RemoteSurfaceId, RemoteWindowId>,
    surface_kind: HashMap<RemoteSurfaceId, RemoteSurfaceKind>,
    surface_parent: HashMap<RemoteSurfaceId, RemoteSurfaceId>,
    surface_client: HashMap<RemoteSurfaceId, ClientKey>,
    surfaces: HashMap<RemoteSurfaceId, RemoteSurface>,
    client_windows: HashMap<ClientKey, BTreeSet<RemoteWindowId>>,
    window_client: HashMap<RemoteWindowId, ClientKey>,
    next_window: u64,
}

impl WindowRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    // MARK: - Queries

    pub fn window(&self, id: &RemoteWindowId) -> Option<&RemoteWindow> {
        self.windows.get(id)
    }

    pub fn windows(&self) -> impl Iterator<Item = &RemoteWindow> {
        self.windows.values()
    }

    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    pub fn surface_kind(&self, surface: RemoteSurfaceId) -> Option<RemoteSurfaceKind> {
        self.surface_kind.get(&surface).copied()
    }

    /// All surfaces currently registered as popups.
    pub fn popup_surfaces(&self) -> impl Iterator<Item = RemoteSurfaceId> + '_ {
        self.surface_kind
            .iter()
            .filter(|(_, kind)| **kind == RemoteSurfaceKind::Popup)
            .map(|(surface, _)| *surface)
    }

    pub fn window_for_surface(&self, surface: RemoteSurfaceId) -> Option<RemoteWindowId> {
        let mut current = surface;
        for _ in 0..64 {
            if let Some(window) = self.surface_window.get(&current) {
                return Some(window.clone());
            }
            current = *self.surface_parent.get(&current)?;
        }
        None
    }

    /// The root (toplevel) surface that owns `window`.
    pub fn root_surface_for_window(&self, window: &RemoteWindowId) -> Option<RemoteSurfaceId> {
        self.toplevel_surface
            .iter()
            .find(|(_, w)| *w == window)
            .map(|(s, _)| *s)
    }

    // MARK: - Toplevel lifecycle

    /// A new `xdg_toplevel` was created. Always allocates a fresh window id.
    pub fn create_toplevel(
        &mut self,
        client: ClientKey,
        surface: RemoteSurfaceId,
    ) -> Vec<RemoteEvent> {
        if self.toplevel_surface.contains_key(&surface) {
            return Vec::new();
        }
        self.next_window += 1;
        let id = RemoteWindowId::new(format!("window-{}", self.next_window));
        let window = RemoteWindow::new(id.clone(), None);

        self.windows.insert(id.clone(), window.clone());
        self.toplevel_surface.insert(surface, id.clone());
        self.surface_window.insert(surface, id.clone());
        self.surface_kind
            .insert(surface, RemoteSurfaceKind::Toplevel);
        self.surface_client.insert(surface, client);
        self.client_windows
            .entry(client)
            .or_default()
            .insert(id.clone());
        self.window_client.insert(id.clone(), client);

        let remote_surface =
            RemoteSurface::new(surface, id.clone(), None, RemoteSurfaceKind::Toplevel);
        self.surfaces.insert(surface, remote_surface.clone());

        vec![
            RemoteEvent::WindowCreated { window },
            RemoteEvent::SurfaceCreated {
                surface: remote_surface,
            },
        ]
    }

    pub fn update_title(
        &mut self,
        surface: RemoteSurfaceId,
        title: Option<String>,
    ) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        let Some(entry) = self.windows.get_mut(&window) else {
            return Vec::new();
        };
        if entry.title == title {
            return Vec::new();
        }
        entry.title = title.clone();
        vec![RemoteEvent::WindowTitleChanged { id: window, title }]
    }

    pub fn update_app_id(
        &mut self,
        surface: RemoteSurfaceId,
        application_id: Option<RemoteApplicationId>,
    ) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        let Some(entry) = self.windows.get_mut(&window) else {
            return Vec::new();
        };
        if entry.application_id == application_id {
            return Vec::new();
        }
        entry.application_id = application_id.clone();
        vec![RemoteEvent::WindowAppIdChanged {
            id: window,
            application_id,
        }]
    }

    pub fn update_size_hints(
        &mut self,
        surface: RemoteSurfaceId,
        min_size: Option<(i32, i32)>,
        max_size: Option<(i32, i32)>,
    ) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        let Some(entry) = self.windows.get_mut(&window) else {
            return Vec::new();
        };
        if entry.min_size == min_size && entry.max_size == max_size {
            return Vec::new();
        }
        entry.min_size = min_size;
        entry.max_size = max_size;
        vec![RemoteEvent::WindowGeometryChanged {
            id: window,
            min_size,
            max_size,
        }]
    }

    pub fn update_state(
        &mut self,
        surface: RemoteSurfaceId,
        state: RemoteWindowState,
    ) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        let Some(entry) = self.windows.get_mut(&window) else {
            return Vec::new();
        };
        if entry.state == state {
            return Vec::new();
        }
        entry.state = state;
        vec![RemoteEvent::WindowStateChanged { id: window, state }]
    }

    pub fn update_mapped(&mut self, surface: RemoteSurfaceId, mapped: bool) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        let Some(entry) = self.windows.get_mut(&window) else {
            return Vec::new();
        };
        if entry.mapped == mapped {
            return Vec::new();
        }
        entry.mapped = mapped;
        vec![RemoteEvent::WindowMapped { id: window, mapped }]
    }

    /// Close the window owning `surface` (its toplevel root). Idempotent.
    pub fn destroy_toplevel(&mut self, surface: RemoteSurfaceId) -> Vec<RemoteEvent> {
        let Some(window) = self.toplevel_surface.get(&surface).cloned() else {
            return Vec::new();
        };
        self.close_window(&window)
    }

    // MARK: - Surface tree lifecycle

    /// A `wl_subsurface` was observed. Never creates a window.
    pub fn register_subsurface(
        &mut self,
        client: ClientKey,
        surface: RemoteSurfaceId,
        parent: RemoteSurfaceId,
    ) -> Vec<RemoteEvent> {
        if self.surface_window.contains_key(&surface) {
            return Vec::new();
        }
        let Some(window) = self.window_for_surface(parent) else {
            return Vec::new();
        };
        self.surface_window.insert(surface, window.clone());
        self.surface_kind
            .insert(surface, RemoteSurfaceKind::Subsurface);
        self.surface_parent.insert(surface, parent);
        self.surface_client.insert(surface, client);
        let mut remote =
            RemoteSurface::new(surface, window, Some(parent), RemoteSurfaceKind::Subsurface);
        remote.z = self.child_count(parent);
        self.surfaces.insert(surface, remote.clone());
        vec![RemoteEvent::SurfaceCreated { surface: remote }]
    }

    /// An `xdg_popup` was observed. Never creates a window.
    pub fn register_popup(
        &mut self,
        client: ClientKey,
        surface: RemoteSurfaceId,
        parent: RemoteSurfaceId,
    ) -> Vec<RemoteEvent> {
        if self.surface_window.contains_key(&surface) {
            return Vec::new();
        }
        let Some(window) = self.window_for_surface(parent) else {
            return Vec::new();
        };
        self.surface_window.insert(surface, window.clone());
        self.surface_kind.insert(surface, RemoteSurfaceKind::Popup);
        self.surface_parent.insert(surface, parent);
        self.surface_client.insert(surface, client);
        let mut remote = RemoteSurface::new(
            surface,
            window.clone(),
            Some(parent),
            RemoteSurfaceKind::Popup,
        );
        remote.z = self.child_count(parent);
        self.surfaces.insert(surface, remote.clone());
        vec![
            RemoteEvent::PopupCreated {
                id: surface,
                window,
            },
            RemoteEvent::SurfaceCreated { surface: remote },
        ]
    }

    pub fn surface_committed(
        &mut self,
        surface: RemoteSurfaceId,
        buffer: Option<RemoteBufferInfo>,
    ) -> Vec<RemoteEvent> {
        let Some(window) = self.window_for_surface(surface) else {
            return Vec::new();
        };
        vec![RemoteEvent::SurfaceCommitted {
            id: surface,
            window,
            buffer,
        }]
    }

    pub fn destroy_popup(&mut self, surface: RemoteSurfaceId) -> Vec<RemoteEvent> {
        let Some(window) = self.surface_window.remove(&surface) else {
            return Vec::new();
        };
        self.surface_kind.remove(&surface);
        self.surface_parent.remove(&surface);
        self.surface_client.remove(&surface);
        self.surfaces.remove(&surface);
        vec![
            RemoteEvent::PopupDestroyed {
                id: surface,
                window: window.clone(),
            },
            RemoteEvent::SurfaceDestroyed {
                id: surface,
                window,
            },
        ]
    }

    // MARK: - Viewport / configure

    /// Records a viewport change that should become an `xdg_toplevel` configure.
    pub fn viewport_changed(
        &mut self,
        window: RemoteWindowId,
        viewport: RemoteViewport,
    ) -> Vec<RemoteEvent> {
        if !self.windows.contains_key(&window) {
            return Vec::new();
        }
        vec![RemoteEvent::ViewportChanged {
            id: window,
            viewport,
        }]
    }

    // MARK: - Client lifecycle

    /// All windows owned by a client are closed exactly once when it disconnects.
    pub fn client_disconnected(&mut self, client: ClientKey) -> Vec<RemoteEvent> {
        let windows: Vec<RemoteWindowId> = self
            .client_windows
            .remove(&client)
            .map(|set| set.into_iter().collect())
            .unwrap_or_default();
        let mut events = Vec::new();
        for window in windows {
            events.extend(self.close_window(&window));
        }
        events
    }

    // MARK: - Surface queries / geometry

    pub fn surface(&self, id: RemoteSurfaceId) -> Option<&RemoteSurface> {
        self.surfaces.get(&id)
    }

    /// All surfaces of a window, ordered root-first.
    pub fn window_surfaces(&self, window: &RemoteWindowId) -> Vec<RemoteSurface> {
        let mut surfaces: Vec<RemoteSurface> = self
            .surfaces
            .values()
            .filter(|surface| &surface.window == window)
            .cloned()
            .collect();
        surfaces.sort_by_key(|surface| (surface.parent.is_some(), surface.z, surface.id));
        surfaces
    }

    pub fn surface_count(&self) -> usize {
        self.surfaces.len()
    }

    /// Update a surface's geometry (position relative to parent, size, z) and
    /// emit `SurfaceUpdated` when anything changed.
    pub fn update_surface_geometry(
        &mut self,
        surface: RemoteSurfaceId,
        position: (i32, i32),
        size: (u32, u32),
        z: u32,
    ) -> Vec<RemoteEvent> {
        let Some(entry) = self.surfaces.get_mut(&surface) else {
            return Vec::new();
        };
        if entry.position == position && entry.size == size && entry.z == z {
            return Vec::new();
        }
        entry.position = position;
        entry.size = size;
        entry.z = z;
        let updated = entry.clone();
        vec![RemoteEvent::SurfaceUpdated { surface: updated }]
    }

    fn child_count(&self, parent: RemoteSurfaceId) -> u32 {
        self.surface_parent
            .values()
            .filter(|candidate| **candidate == parent)
            .count() as u32
    }

    // MARK: - Private

    fn close_window(&mut self, window: &RemoteWindowId) -> Vec<RemoteEvent> {
        // Removing the window first guarantees `WindowClosed` is emitted at most once.
        if self.windows.remove(window).is_none() {
            return Vec::new();
        }

        let surfaces: Vec<RemoteSurfaceId> = self
            .surface_window
            .iter()
            .filter(|(_, w)| *w == window)
            .map(|(s, _)| *s)
            .collect();

        let mut events = Vec::new();
        for surface in surfaces {
            self.surface_window.remove(&surface);
            self.surface_kind.remove(&surface);
            self.surface_parent.remove(&surface);
            self.surface_client.remove(&surface);
            self.surfaces.remove(&surface);
            self.toplevel_surface.remove(&surface);
            events.push(RemoteEvent::SurfaceDestroyed {
                id: surface,
                window: window.clone(),
            });
        }

        if let Some(client) = self.window_client.remove(window) {
            if let Some(set) = self.client_windows.get_mut(&client) {
                set.remove(window);
            }
        }

        events.push(RemoteEvent::WindowClosed { id: window.clone() });
        events
    }
}
