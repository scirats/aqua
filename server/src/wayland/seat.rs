use smithay::{
    delegate_data_device, delegate_output, delegate_seat,
    input::{pointer::CursorImageStatus, Seat, SeatHandler, SeatState},
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    wayland::{
        output::OutputHandler,
        selection::{
            data_device::{
                set_data_device_focus, ClientDndGrabHandler, DataDeviceHandler, DataDeviceState,
                ServerDndGrabHandler,
            },
            SelectionHandler,
        },
    },
};

use super::state::AquaState;

impl SeatHandler for AquaState {
    type KeyboardFocus = WlSurface;
    type PointerFocus = WlSurface;
    type TouchFocus = WlSurface;

    fn seat_state(&mut self) -> &mut SeatState<AquaState> {
        &mut self.seat_state
    }

    fn cursor_image(&mut self, _seat: &Seat<Self>, _image: CursorImageStatus) {
        // `wl_pointer.set_cursor` surfaces arrive here. They are NOT windows and
        // are not transmitted in phase 2. See `docs/PHASE2-server.md` for the RemoteCursor plan.
    }

    fn focus_changed(&mut self, seat: &Seat<Self>, focused: Option<&WlSurface>) {
        let dh = self.display_handle.clone();
        let client = focused.and_then(|surface| dh.get_client(surface.id()).ok());
        set_data_device_focus(&dh, seat, client);
    }
}

delegate_seat!(AquaState);

// MARK: - Data device (clipboard / DnD groundwork)

impl SelectionHandler for AquaState {
    type SelectionUserData = ();
}

impl DataDeviceHandler for AquaState {
    fn data_device_state(&self) -> &DataDeviceState {
        &self.data_device_state
    }
}

impl ClientDndGrabHandler for AquaState {}
impl ServerDndGrabHandler for AquaState {}

delegate_data_device!(AquaState);

// MARK: - Output

impl OutputHandler for AquaState {}

delegate_output!(AquaState);
