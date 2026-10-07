use std::time::Duration;

use smithay::{
    backend::input::{Axis, AxisSource, ButtonState, KeyState},
    desktop::utils::send_frames_surface_tree,
    input::{
        keyboard::{FilterResult, Keycode},
        pointer::{AxisFrame, ButtonEvent, MotionEvent},
    },
    reexports::wayland_server::{protocol::wl_surface::WlSurface, Resource},
    utils::{Point, SERIAL_COUNTER},
};

use crate::domain::{RemoteViewport, RemoteWindowId};

use super::state::{AquaState, ControlMessage};

impl AquaState {
    /// Handle a control message delivered by the Wayland client adapter.
    pub fn handle_control(&mut self, message: ControlMessage) {
        match message {
            ControlMessage::ClientDisconnected(client_id) => {
                self.handle_client_disconnected(client_id);
            }
        }
    }

    // MARK: - Viewport

    /// `resize <window> <w> <h>` -> `RemoteViewportChanged` -> xdg configure.
    pub fn resize_window(&mut self, window: &str, width: i32, height: i32) {
        let id = RemoteWindowId::new(window);
        if self.registry.window(&id).is_none() {
            tracing::warn!(window = %window, "resize ignored: unknown window");
            return;
        }

        let viewport = RemoteViewport::new(width, height, self.viewport_scale);
        let events = self.registry.viewport_changed(id.clone(), viewport);
        self.emit(events);

        let Some(toplevel) = self
            .registry
            .root_surface_for_window(&id)
            .and_then(|sid| self.toplevel(sid))
        else {
            tracing::warn!(window = %window, "resize: no toplevel surface");
            return;
        };

        toplevel.with_pending_state(|state| {
            state.size = Some((width, height).into());
        });
        toplevel.send_pending_configure();
        tracing::info!(
            target: "aqua::configure",
            window = %window,
            width,
            height,
            "xdg_toplevel.configure sent (awaiting ack_configure)"
        );
    }

    // MARK: - Input

    pub fn focus_window(&mut self, window: &str) {
        let id = RemoteWindowId::new(window);
        let Some(surface) = self.root_surface_of(&id) else {
            tracing::warn!(window = %window, "focus ignored: unknown window");
            return;
        };
        self.keyboard_focus = Some(id.clone());
        self.pointer_focus = Some(id.clone());

        let serial = SERIAL_COUNTER.next_serial();
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, Some(surface), serial);
        }
        tracing::info!(target: "aqua::input", window = %window, "keyboard focus set");
    }

    fn pointer_move(&mut self, x: f64, y: f64) {
        let Some(window) = self
            .pointer_focus
            .clone()
            .or_else(|| self.keyboard_focus.clone())
        else {
            return;
        };
        let Some(surface) = self.root_surface_of(&window) else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let location = Point::from((x, y));
        if let Some(pointer) = self.seat.get_pointer() {
            pointer.motion(
                self,
                Some((surface, location)),
                &MotionEvent {
                    location,
                    serial,
                    time: 0,
                },
            );
            pointer.frame(self);
        }
        tracing::info!(target: "aqua::input", window = %window, x, y, "pointer.motion");
    }

    fn scroll(&mut self, dx: f64, dy: f64) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let mut frame = AxisFrame::new(0).source(AxisSource::Wheel);
        if dy != 0.0 {
            frame = frame.value(Axis::Vertical, dy);
        }
        if dx != 0.0 {
            frame = frame.value(Axis::Horizontal, dx);
        }
        pointer.axis(self, frame);
        pointer.frame(self);
        tracing::info!(target: "aqua::input", dx, dy, "pointer.axis");
    }

    // MARK: - Remote input (from the iPad, over Aqua Protocol)

    pub(crate) fn inject_pointer_moved(&mut self, x: f64, y: f64) {
        self.pointer_move(x, y);
    }

    pub(crate) fn inject_pointer_scroll(&mut self, dx: f64, dy: f64) {
        self.scroll(dx, dy);
    }

    pub(crate) fn inject_pointer_button(&mut self, button: u32, pressed: bool) {
        let Some(pointer) = self.seat.get_pointer() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let state = if pressed {
            ButtonState::Pressed
        } else {
            ButtonState::Released
        };
        pointer.button(
            self,
            &ButtonEvent {
                serial,
                time: 0,
                button,
                state,
            },
        );
        pointer.frame(self);
        tracing::info!(target: "aqua::input", button, pressed, "pointer.button (remote)");
    }

    /// Inject a key from the iPad. `keycode` is a Linux evdev code (0 if the
    /// client only knows the character); otherwise it is resolved from
    /// `characters` using the US layout map.
    pub(crate) fn inject_key(&mut self, characters: &str, keycode: u32, pressed: bool) {
        let evdev = if keycode > 0 {
            keycode
        } else {
            match crate::control::resolve_keycode(characters) {
                Some(code) => code,
                None => {
                    tracing::debug!(characters, "remote key not mapped");
                    return;
                }
            }
        };
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let serial = SERIAL_COUNTER.next_serial();
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        keyboard.input(
            self,
            Keycode::new(evdev + 8),
            state,
            serial,
            0,
            |_, _, _| FilterResult::<()>::Forward,
        );
        tracing::info!(target: "aqua::input", characters, evdev, pressed, "keyboard.input (remote)");
    }

    // MARK: - Frame clock

    /// Drive `wl_surface.frame` callbacks. Without a renderer we throttle to a
    /// fixed clock and always deliver; this keeps clients from stalling.
    pub fn send_frames(&mut self) {
        // Video backpressure: a full video queue asks for a forced keyframe so the
        // decoder can resynchronize after dropped GOPs.
        if let Some(sink) = self.video_sink.clone() {
            for window_id in sink.take_keyframe_requests() {
                self.request_keyframe(&window_id, "video_backpressure");
            }
        }
        if self.registry.window_count() == 0 {
            return;
        }
        let time = self.start_time.elapsed();
        let output = self.output.clone();

        let windows: Vec<RemoteWindowId> = self.registry.windows().map(|w| w.id.clone()).collect();
        let mut surfaces: Vec<WlSurface> = windows
            .iter()
            .filter_map(|window| self.root_surface_of(window))
            .collect();

        // Popups own a separate surface tree.
        let popup_ids: Vec<_> = self.registry.popup_surfaces().collect();
        for id in popup_ids {
            if let Some(surface) = self.surface_handle(id) {
                surfaces.push(surface.clone());
            }
        }

        for surface in surfaces {
            if !surface.is_alive() {
                continue;
            }
            send_frames_surface_tree(&surface, &output, time, Some(Duration::ZERO), |_, _| {
                Some(output.clone())
            });
        }
    }
}
