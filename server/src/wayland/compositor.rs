use smithay::{
    backend::renderer::utils::on_commit_buffer_handler,
    delegate_compositor, delegate_shm,
    reexports::wayland_server::protocol::{wl_buffer, wl_surface::WlSurface},
    wayland::{
        buffer::BufferHandler,
        compositor::{
            get_parent, with_states, BufferAssignment, CompositorClientState, CompositorHandler,
            CompositorState, Damage, SubsurfaceCachedState, SurfaceAttributes,
        },
        shm::{ShmHandler, ShmState},
    },
};

use crate::{
    domain::{RemoteSurfaceId, RemoteSurfaceKind},
    gpu::GpuFrame,
    protocol::data::{self, DamageRect, SurfaceFrame},
};

use super::state::{read_buffer_info, read_size_hints, root_surface, AquaState, ClientState};

impl CompositorHandler for AquaState {
    fn compositor_state(&mut self) -> &mut CompositorState {
        &mut self.compositor_state
    }

    fn client_compositor_state<'a>(
        &self,
        client: &'a smithay::reexports::wayland_server::Client,
    ) -> &'a CompositorClientState {
        &client.get_data::<ClientState>().unwrap().compositor_state
    }

    fn commit(&mut self, surface: &WlSurface) {
        // Capture the committed buffer *before* `on_commit_buffer_handler`,
        // which consumes (`take`s) `SurfaceAttributes.buffer`. SHM is copied;
        // dmabuf is only inspected (never CPU-mapped) on the normal path.
        let captured = capture_surface_buffer(surface);
        let dmabuf = capture_dmabuf_info(surface);
        on_commit_buffer_handler::<Self>(surface);
        handle_commit(self, surface, captured, dmabuf);
    }
}

impl BufferHandler for AquaState {
    fn buffer_destroyed(&mut self, _buffer: &wl_buffer::WlBuffer) {}
}

impl ShmHandler for AquaState {
    fn shm_state(&self) -> &ShmState {
        &self.shm_state
    }
}

delegate_compositor!(AquaState);
delegate_shm!(AquaState);

/// Translate a `wl_surface` commit into neutral domain events and, for SHM
/// surfaces, a data-plane frame.
fn handle_commit(
    state: &mut AquaState,
    surface: &WlSurface,
    captured: Option<Captured>,
    dmabuf: Option<DmabufInfo>,
) {
    let sid = state.surface_id(surface);

    // GPU path: record the dmabuf metadata (format/modifier/planes/dimensions).
    // The actual import happened when the client created the `wl_buffer` (see
    // `wayland::dmabuf`); here we only observe the attach. No CPU readback.
    let has_dmabuf = dmabuf.is_some();
    if let Some(info) = &dmabuf {
        tracing::debug!(
            target: "aqua::frame",
            surface_id = %sid,
            width = info.width,
            height = info.height,
            fourcc = format_args!("{:#010x}", info.fourcc),
            modifier = format_args!("{:#018x}", info.modifier),
            planes = info.planes,
            "frame.dmabuf_attached"
        );
    }

    // A surface we have never seen, but which has a parent, is a subsurface.
    if state.registry.surface_kind(sid).is_none() {
        if let Some(parent) = get_parent(surface) {
            let parent_id = state.surface_id(&parent);
            let client = state.client_key_for_surface(surface);
            let events = state.registry.register_subsurface(client, sid, parent_id);
            state.emit(events);
        } else {
            // Unknown root surface (e.g. a cursor): not a window candidate yet.
            let _ = root_surface(surface);
        }
    }

    let buffer = read_buffer_info(surface);
    let mapped = buffer.is_some();
    let events = state.registry.surface_committed(sid, buffer);
    state.emit(events);

    if state.registry.surface_kind(sid).is_some() {
        let size = captured
            .as_ref()
            .map(|frame| (frame.width, frame.height))
            .unwrap_or((0, 0));
        let position = surface_position(surface);
        let z = state.registry.surface(sid).map(|s| s.z).unwrap_or(0);
        let events = state
            .registry
            .update_surface_geometry(sid, position, size, z);
        state.emit(events);
    }

    if let Some(captured) = captured {
        if let Some(sink) = state.frame_sink.clone() {
            let frame_id = state.next_frame_id(sid);
            let window_id = state
                .registry
                .window_for_surface(sid)
                .map(|window| window.to_string())
                .unwrap_or_default();
            let frame = SurfaceFrame {
                surface_id: sid.to_string(),
                window_id,
                frame_id,
                width: captured.width,
                height: captured.height,
                stride: captured.stride,
                format: captured.format,
                damage: captured.damage,
                data: captured.data,
            };
            tracing::debug!(
                target: "aqua::frame",
                surface_id = %sid,
                frame_id,
                width = frame.width,
                height = frame.height,
                bytes = frame.payload_len(),
                "frame.captured"
            );
            // Video plane (phase 3C): encode the root toplevel commit too.
            if state.has_video()
                && matches!(
                    state.registry.surface_kind(sid),
                    Some(RemoteSurfaceKind::Toplevel)
                )
            {
                state.encode_video_frame(&frame);
            }
            sink.submit_frame(frame);
        }
    }

    // GPU path: encode this dmabuf commit too (dormant until an encoder
    // advertises `supports_dmabuf()`), never touching CPU pixels.
    if has_dmabuf
        && matches!(
            state.registry.surface_kind(sid),
            Some(RemoteSurfaceKind::Toplevel)
        )
        && state.video_supports_dmabuf()
    {
        if let Some(gpu) = dmabuf_frame(state, surface, sid) {
            state.encode_video_gpu_frame(gpu);
        }
    }

    match state.registry.surface_kind(sid) {
        Some(RemoteSurfaceKind::Toplevel) => {
            let events = state.registry.update_mapped(sid, mapped);
            state.emit(events);

            // Keep size hints fresh: there is no dedicated xdg handler for
            // set_min_size/set_max_size, so we read them on commit.
            let (min, max) = read_size_hints(surface);
            let events = state.registry.update_size_hints(sid, min, max);
            state.emit(events);

            if let Some(toplevel) = state.toplevel(sid) {
                if !toplevel.is_initial_configure_sent() {
                    toplevel.send_configure();
                }
            }
        }
        Some(RemoteSurfaceKind::Popup) => {}
        _ => {}
    }

    // Let clients know their surface is on the virtual output.
    state.output.enter(surface);
}

/// A committed SHM buffer copied into owned memory.
struct Captured {
    width: u32,
    height: u32,
    stride: u32,
    format: u32,
    damage: Vec<DamageRect>,
    data: Vec<u8>,
}

fn capture_surface_buffer(surface: &WlSurface) -> Option<Captured> {
    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<SurfaceAttributes>();
        let attrs = cached.current();
        let buffer = match attrs.buffer.as_ref() {
            Some(BufferAssignment::NewBuffer(buffer)) => buffer.clone(),
            Some(BufferAssignment::Removed) => {
                tracing::trace!(target: "aqua::frame", "capture: buffer removed");
                return None;
            }
            None => {
                tracing::trace!(target: "aqua::frame", "capture: no buffer attached");
                return None;
            }
        };
        let damage: Vec<DamageRect> = attrs.damage.iter().filter_map(damage_rect).collect();
        let captured = capture_buffer(&buffer);
        tracing::trace!(target: "aqua::frame", ok = captured.is_some(), "capture: buffer read");
        let (width, height, stride, format, data) = captured?;
        Some(Captured {
            width,
            height,
            stride,
            format,
            damage,
            data,
        })
    })
}

fn capture_buffer(buffer: &wl_buffer::WlBuffer) -> Option<(u32, u32, u32, u32, Vec<u8>)> {
    let captured = smithay::wayland::shm::with_buffer_contents(buffer, |ptr, len, buffer_data| {
        use smithay::reexports::wayland_server::protocol::wl_shm::Format;
        tracing::trace!(
            target: "aqua::frame",
            format = ?buffer_data.format,
            width = buffer_data.width,
            height = buffer_data.height,
            stride = buffer_data.stride,
            len,
            "capture: shm buffer"
        );
        let format = match buffer_data.format {
            Format::Argb8888 => data::FORMAT_ARGB8888,
            Format::Xrgb8888 => data::FORMAT_XRGB8888,
            _ => return None,
        };
        let width = buffer_data.width.max(0) as u32;
        let height = buffer_data.height.max(0) as u32;
        let stride = buffer_data.stride.max(0) as u32;
        let expected = data::validate_shm(width, height, stride, len).ok()?;
        // Safety: `ptr` is valid for `expected` bytes for the duration of this
        // callback and `expected <= len` was validated. We copy immediately.
        let bytes = unsafe { std::slice::from_raw_parts(ptr, expected) }.to_vec();
        Some((width, height, stride, format, bytes))
    });
    if let Err(error) = &captured {
        tracing::trace!(target: "aqua::frame", ?error, "capture: with_buffer_contents error");
    }
    captured.ok().flatten()
}

/// Metadata of a committed `linux-dmabuf` buffer. Observed for logging and for
/// the (future) handoff to the video encoder; the fds themselves are duplicated
/// only inside the dmabuf import path, never here.
struct DmabufInfo {
    width: u32,
    height: u32,
    fourcc: u32,
    modifier: u64,
    planes: usize,
}

/// Build a neutral [`GpuFrame`] from the committed dmabuf of `surface`,
/// duplicating the plane fds (no CPU readback). Only called when a dmabuf-aware
/// encoder is installed.
fn dmabuf_frame(state: &AquaState, surface: &WlSurface, sid: RemoteSurfaceId) -> Option<GpuFrame> {
    let window_id = state.registry.window_for_surface(sid)?.to_string();
    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<SurfaceAttributes>();
        let buffer = match cached.current().buffer.as_ref() {
            Some(BufferAssignment::NewBuffer(buffer)) => buffer,
            _ => return None,
        };
        let dmabuf = smithay::wayland::dmabuf::get_dmabuf(buffer).ok()?;
        let mut frame = crate::wayland::dmabuf::gpu_frame_from_dmabuf(dmabuf).ok()?;
        frame.window_id = window_id.clone();
        frame.surface_id = sid.to_string();
        Some(frame)
    })
}

fn capture_dmabuf_info(surface: &WlSurface) -> Option<DmabufInfo> {    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<SurfaceAttributes>();
        let attrs = cached.current();
        let buffer = match attrs.buffer.as_ref() {
            Some(BufferAssignment::NewBuffer(buffer)) => buffer,
            _ => return None,
        };
        let dmabuf = smithay::wayland::dmabuf::get_dmabuf(buffer).ok()?;
        use smithay::backend::allocator::Buffer as _;
        let format = dmabuf.format();
        Some(DmabufInfo {
            width: dmabuf.width(),
            height: dmabuf.height(),
            fourcc: format.code as u32,
            modifier: u64::from(format.modifier),
            planes: dmabuf.num_planes(),
        })
    })
}

fn damage_rect(damage: &Damage) -> Option<DamageRect> {
    Some(match damage {
        Damage::Surface(rect) => DamageRect {
            x: rect.loc.x,
            y: rect.loc.y,
            width: rect.size.w.max(0) as u32,
            height: rect.size.h.max(0) as u32,
        },
        Damage::Buffer(rect) => DamageRect {
            x: rect.loc.x,
            y: rect.loc.y,
            width: rect.size.w.max(0) as u32,
            height: rect.size.h.max(0) as u32,
        },
    })
}

fn surface_position(surface: &WlSurface) -> (i32, i32) {
    with_states(surface, |states| {
        let mut cached = states.cached_state.get::<SubsurfaceCachedState>();
        let location = cached.current().location;
        (location.x, location.y)
    })
}
