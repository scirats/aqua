use smithay::{
    delegate_xdg_shell,
    reexports::{
        wayland_protocols::xdg::shell::server::xdg_toplevel,
        wayland_server::protocol::{wl_seat, wl_surface::WlSurface},
    },
    utils::Serial,
    wayland::shell::xdg::{
        PopupSurface, PositionerState, ToplevelSurface, XdgShellHandler, XdgShellState,
    },
};

use crate::domain::{RemoteApplicationId, RemoteWindowId, RemoteWindowState};

use super::state::{read_size_hints, read_toplevel_title_app_id, AquaState};

impl XdgShellHandler for AquaState {
    fn xdg_shell_state(&mut self) -> &mut XdgShellState {
        &mut self.xdg_shell_state
    }

    /// A new `xdg_toplevel` appeared. One window per toplevel, always a fresh id.
    fn new_toplevel(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);
        let client = self.client_key_for_surface(&wl_surface);

        let events = self.registry.create_toplevel(client, sid);
        self.register_toplevel(sid, surface.clone());
        self.emit(events);

        if !surface.is_initial_configure_sent() {
            surface.send_configure();
        }
        self.sync_toplevel_metadata(&wl_surface);
    }

    /// A new `xdg_popup` (menu/tooltip/dropdown). Belongs to a window, never a new one.
    fn new_popup(&mut self, surface: PopupSurface, _positioner: PositionerState) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);

        if let Some(parent) = surface.get_parent_surface() {
            let parent_id = self.surface_id(&parent);
            let client = self.client_key_for_surface(&wl_surface);
            let events = self.registry.register_popup(client, sid, parent_id);
            self.emit(events);
        } else {
            tracing::warn!(surface_id = %sid, "popup without parent surface");
        }

        let _ = surface.send_configure();
    }

    fn toplevel_destroyed(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);
        self.unregister_toplevel(sid);
        let events = self.registry.destroy_toplevel(sid);
        self.emit(events);
        self.reap_surface(sid);
    }

    fn popup_destroyed(&mut self, surface: PopupSurface) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);
        let events = self.registry.destroy_popup(sid);
        self.emit(events);
        self.reap_surface(sid);
    }

    fn title_changed(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        self.sync_toplevel_metadata(&wl_surface);
    }

    fn app_id_changed(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        self.sync_toplevel_metadata(&wl_surface);
    }

    fn parent_changed(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);
        tracing::debug!(surface_id = %sid, "window.parent_changed");
    }

    /// The client accepted a configure. This is the end of the configure/ack
    /// handshake; the committed geometry may carry updated size hints.
    fn ack_configure(
        &mut self,
        surface: WlSurface,
        _configure: smithay::wayland::shell::xdg::Configure,
    ) {
        let sid = self.surface_id(&surface);
        tracing::debug!(surface_id = %sid, "xdg_surface.ack_configure");
        let (min, max) = read_size_hints(&surface);
        let events = self.registry.update_size_hints(sid, min, max);
        self.emit(events);
    }

    fn maximize_request(&mut self, surface: ToplevelSurface) {
        self.apply_xdg_state(&surface, RemoteWindowState::Maximized);
    }

    fn unmaximize_request(&mut self, surface: ToplevelSurface) {
        self.apply_xdg_state(&surface, RemoteWindowState::Normal);
    }

    fn fullscreen_request(
        &mut self,
        surface: ToplevelSurface,
        _output: Option<smithay::reexports::wayland_server::protocol::wl_output::WlOutput>,
    ) {
        self.apply_xdg_state(&surface, RemoteWindowState::Fullscreen);
    }

    fn unfullscreen_request(&mut self, surface: ToplevelSurface) {
        self.apply_xdg_state(&surface, RemoteWindowState::Normal);
    }

    /// `set_minimized` is compositor-side in `xdg_toplevel`; there is no state to
    /// confirm back to the client. We only update our domain state.
    fn minimize_request(&mut self, surface: ToplevelSurface) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);
        let events = self
            .registry
            .update_state(sid, RemoteWindowState::Minimized);
        self.emit(events);
    }

    fn move_request(&mut self, _surface: ToplevelSurface, _seat: wl_seat::WlSeat, _serial: Serial) {
        // No compositor-side move without a real input device.
    }

    fn resize_request(
        &mut self,
        _surface: ToplevelSurface,
        _seat: wl_seat::WlSeat,
        _serial: Serial,
        _edges: xdg_toplevel::ResizeEdge,
    ) {
        // Interactive resize belongs to the iPad in later phases.
    }

    fn grab(&mut self, _surface: PopupSurface, _seat: wl_seat::WlSeat, _serial: Serial) {}

    fn reposition_request(
        &mut self,
        surface: PopupSurface,
        _positioner: PositionerState,
        token: u32,
    ) {
        let _ = surface.send_repositioned(token);
    }
}

delegate_xdg_shell!(AquaState);

impl AquaState {
    /// Read title/app_id/size hints from the surface and emit any changes.
    fn sync_toplevel_metadata(&mut self, surface: &WlSurface) {
        let sid = self.surface_id(surface);
        let (title, app_id) = read_toplevel_title_app_id(surface);
        let application_id = app_id
            .filter(|value| !value.is_empty())
            .map(RemoteApplicationId::new);

        let mut events = self.registry.update_app_id(sid, application_id);
        events.extend(self.registry.update_title(sid, title));
        let (min, max) = read_size_hints(surface);
        events.extend(self.registry.update_size_hints(sid, min, max));
        self.emit(events);
    }

    /// Map an Aqua state change into an xdg state set + configure, and update the
    /// domain.
    fn apply_xdg_state(&mut self, surface: &ToplevelSurface, state: RemoteWindowState) {
        let wl_surface = surface.wl_surface().clone();
        let sid = self.surface_id(&wl_surface);

        surface.with_pending_state(|pending| match state {
            RemoteWindowState::Maximized => {
                pending.states.set(xdg_toplevel::State::Maximized);
            }
            RemoteWindowState::Fullscreen => {
                pending.states.set(xdg_toplevel::State::Fullscreen);
            }
            RemoteWindowState::Normal => {
                pending.states.unset(xdg_toplevel::State::Maximized);
                pending.states.unset(xdg_toplevel::State::Fullscreen);
            }
            RemoteWindowState::Minimized => {}
        });

        let events = self.registry.update_state(sid, state);
        self.emit(events);

        if state != RemoteWindowState::Minimized {
            surface.send_configure();
        }
    }
}

/// Reserved for future focus routing from the iPad.
pub(crate) fn _reserved(_: &RemoteWindowId) {}
