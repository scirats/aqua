use std::{collections::HashMap, sync::Arc, time::Instant};

use std::os::unix::fs::MetadataExt;

use bytes::Bytes;

use smithay::{
    desktop::PopupManager,
    input::{Seat, SeatState},
    output::Output,
    reexports::{
        calloop::{
            channel::Sender, generic::Generic, EventLoop, Interest, LoopSignal, Mode, PostAction,
        },
        wayland_server::{
            backend::{ClientData, ClientId, DisconnectReason, ObjectId},
            protocol::{wl_buffer, wl_surface::WlSurface},
            Client, Display, DisplayHandle, Resource,
        },
    },
    wayland::{
        compositor::{
            get_parent, BufferAssignment, CompositorClientState, CompositorState, SurfaceAttributes,
        },
        dmabuf::{DmabufGlobal, DmabufState},
        output::OutputManagerState,
        selection::data_device::DataDeviceState,
        shell::xdg::{ToplevelSurface, XdgShellState, XdgToplevelSurfaceData},
        shm::ShmState,
    },
};

use tokio::sync::mpsc::UnboundedSender;

use crate::{
    domain::{
        ClientKey, RemoteBufferInfo, RemoteEvent, RemoteSurfaceId, RemoteWindowId, WindowRegistry,
    },
    events::{RemoteEventSink, TracingEventSink},
    gpu::{
        DmabufFormat, FrameSource, GpuBufferImporter, GpuFrame, NullGpuImporter, NullVideoEncoder,
        SyncState, VideoChroma, VideoCodec, VideoEncoder, VideoEncoderConfig, VideoEncoderSession,
    },
    net::{FrameSink, ServerEvent, VideoConfig, VideoSink},
    protocol::{adapters, data::SurfaceFrame, v1, ServerMessage},
};

use super::output::create_virtual_output;

/// Messages delivered to the event loop from outside the Wayland dispatch.
#[derive(Debug)]
pub enum ControlMessage {
    /// A line typed on stdin (demo control channel).
    Command(String),
    /// A Wayland client disconnected.
    ClientDisconnected(ClientId),
}

/// Per-client data stored inside the Wayland display.
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    pub control_tx: Sender<ControlMessage>,
}

impl ClientState {
    pub fn new(control_tx: Sender<ControlMessage>) -> Self {
        Self {
            compositor_state: CompositorClientState::default(),
            control_tx,
        }
    }
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}

    fn disconnected(&self, client_id: ClientId, _reason: DisconnectReason) {
        // Runs during dispatch with `&self`; hand the cleanup to the event loop.
        let _ = self
            .control_tx
            .send(ControlMessage::ClientDisconnected(client_id));
    }
}

/// Shared data threaded through the `calloop` event loop.
pub struct CalloopData {
    pub state: AquaState,
    pub display_handle: DisplayHandle,
}

/// The Aqua compositor state.
///
/// It owns Smithay state, the domain registry, and the adapter bookkeeping
/// (`wl_surface` -> `RemoteSurfaceId`, `Client` -> `ClientKey`).
pub struct AquaState {
    pub start_time: Instant,
    pub display_handle: DisplayHandle,
    pub loop_signal: LoopSignal,

    // Smithay state
    pub compositor_state: CompositorState,
    pub xdg_shell_state: XdgShellState,
    pub shm_state: ShmState,
    pub output_manager_state: OutputManagerState,
    pub seat_state: SeatState<AquaState>,
    pub data_device_state: DataDeviceState,
    pub popups: PopupManager,
    pub seat: Seat<AquaState>,
    pub output: Output,

    // Aqua domain
    pub registry: WindowRegistry,
    pub sink: Box<dyn RemoteEventSink>,

    // Adapter bookkeeping
    surfaces: HashMap<ObjectId, RemoteSurfaceId>,
    surface_handles: HashMap<RemoteSurfaceId, WlSurface>,
    client_keys: HashMap<ClientId, ClientKey>,
    toplevels: HashMap<RemoteSurfaceId, ToplevelSurface>,
    next_surface: u64,
    next_client: u64,

    pub keyboard_focus: Option<RemoteWindowId>,
    pub pointer_focus: Option<RemoteWindowId>,
    pub viewport_scale: f64,

    // Aqua Protocol v1 session state
    pub server_session_id: String,
    pub server_identity: String,
    pub capabilities: u32,
    revision: u64,
    network_clients: HashMap<u64, UnboundedSender<ServerMessage>>,
    /// Per connected client: whether it negotiated `SURFACE_VIDEO` in the
    /// handshake. Video is only produced while at least one client wants it.
    video_clients: HashMap<u64, bool>,
    pub(crate) frame_sink: Option<Arc<dyn FrameSink>>,
    pub(crate) video_sink: Option<Arc<dyn VideoSink>>,
    frame_ids: HashMap<RemoteSurfaceId, u64>,

    // Phase 3C: GPU import + video encoding (neutral boundary).
    pub dmabuf_state: DmabufState,
    pub dmabuf_global: Option<DmabufGlobal>,
    pub gpu_importer: Arc<dyn GpuBufferImporter>,
    pub video_encoder: Arc<dyn VideoEncoder>,
    pub video_sessions: HashMap<RemoteWindowId, Box<dyn VideoEncoderSession>>,
}

impl AquaState {
    pub fn new(event_loop: &mut EventLoop<CalloopData>, display_handle: &DisplayHandle) -> Self {
        let compositor_state = CompositorState::new::<Self>(display_handle);
        let xdg_shell_state = XdgShellState::new::<Self>(display_handle);
        let shm_state = ShmState::new::<Self>(
            display_handle,
            [
                smithay::reexports::wayland_server::protocol::wl_shm::Format::Argb8888,
                smithay::reexports::wayland_server::protocol::wl_shm::Format::Xrgb8888,
            ],
        );
        let output_manager_state = OutputManagerState::new_with_xdg_output::<Self>(display_handle);
        let data_device_state = DataDeviceState::new::<Self>(display_handle);

        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(display_handle, "aqua");
        // Keyboard + pointer are always present. Real devices arrive later from
        // the iPad through the mock input source.
        let _ = seat.add_keyboard(Default::default(), 200, 25);
        seat.add_pointer();

        let output = create_virtual_output(display_handle);
        let popups = PopupManager::default();

        Self {
            start_time: Instant::now(),
            display_handle: display_handle.clone(),
            loop_signal: event_loop.get_signal(),

            compositor_state,
            xdg_shell_state,
            shm_state,
            output_manager_state,
            seat_state,
            data_device_state,
            popups,
            seat,
            output,

            registry: WindowRegistry::new(),
            sink: Box::new(TracingEventSink),

            surfaces: HashMap::new(),
            surface_handles: HashMap::new(),
            client_keys: HashMap::new(),
            toplevels: HashMap::new(),
            next_surface: 0,
            next_client: 0,

            keyboard_focus: None,
            pointer_focus: None,
            viewport_scale: 1.0,

            server_session_id: String::new(),
            server_identity: String::new(),
            capabilities: crate::protocol::capability::advertised(false),
            revision: 0,
            network_clients: HashMap::new(),
            video_clients: HashMap::new(),
            frame_sink: None,
            video_sink: None,
            frame_ids: HashMap::new(),

            dmabuf_state: DmabufState::new(),
            dmabuf_global: None,
            gpu_importer: Arc::new(NullGpuImporter),
            video_encoder: Arc::new(NullVideoEncoder),
            video_sessions: HashMap::new(),
        }
    }

    /// Install the data-plane frame sink (the QUIC frame hub).
    pub fn set_frame_sink(&mut self, sink: Arc<dyn FrameSink>) {
        self.frame_sink = Some(sink);
    }

    /// Install the window-video data-plane sink (the QUIC video hub).
    pub fn set_video_sink(&mut self, sink: Arc<dyn VideoSink>) {
        self.video_sink = Some(sink);
    }

    /// Install the GPU importer and video encoder.
    ///
    /// This is the switch that turns phase 3C on: the `zwp_linux_dmabuf_v1`
    /// global is created **only if** the importer advertises at least one
    /// importable `(fourcc, modifier)` pair, and `SURFACE_VIDEO` is advertised
    /// **only if** the encoder supports at least one codec. With the default
    /// headless `NullGpuImporter`/`NullVideoEncoder` this is a no-op and Aqua
    /// keeps advertising SHM only (see `docs/GPU_PIPELINE.md`).
    pub fn install_gpu(
        &mut self,
        importer: Arc<dyn GpuBufferImporter>,
        encoder: Arc<dyn VideoEncoder>,
    ) {
        self.gpu_importer = importer;
        self.video_encoder = encoder;
        self.capabilities = crate::protocol::capability::advertised(
            !self.video_encoder.supported_codecs().is_empty(),
        );

        if self.gpu_importer.supported_formats().is_empty() {
            tracing::info!(
                importer = self.gpu_importer.name(),
                "gpu: no importable formats; zwp_linux_dmabuf_v1 not advertised"
            );
            self.dmabuf_global = None;
            return;
        }

        let formats = crate::wayland::dmabuf::smithay_formats(self.gpu_importer.as_ref());
        if formats.is_empty() {
            self.dmabuf_global = None;
            return;
        }
        let count = formats.len();
        // Announce a default feedback carrying the real DRM device, so EGL
        // clients (Mesa) can obtain the render node and allocate dmabufs.
        let device = std::env::var("AQUA_VAAPI_DEVICE")
            .unwrap_or_else(|_| "/dev/dri/renderD128".to_string());
        let plain_formats = formats.clone();
        let global = match std::fs::metadata(&device).map(|meta| meta.rdev()) {
            Ok(main_device) => {
                match smithay::wayland::dmabuf::DmabufFeedbackBuilder::new(main_device, formats)
                    .build()
                {
                    Ok(feedback) => self
                        .dmabuf_state
                        .create_global_with_default_feedback::<AquaState>(
                            &self.display_handle,
                            &feedback,
                        ),
                    Err(error) => {
                        tracing::warn!(%error, "dmabuf: feedback build failed; plain global");
                        self.dmabuf_state
                            .create_global::<AquaState>(&self.display_handle, plain_formats)
                    }
                }
            }
            Err(error) => {
                tracing::warn!(%error, device, "dmabuf: cannot stat render node; plain global");
                self.dmabuf_state
                    .create_global::<AquaState>(&self.display_handle, plain_formats)
            }
        };
        tracing::info!(
            importer = self.gpu_importer.name(),
            formats = count,
            encoder = self.video_encoder.name(),
            "gpu: zwp_linux_dmabuf_v1 global created"
        );
        self.dmabuf_global = Some(global);
    }

    /// Whether the video data plane is active (an encoder with codecs exists).
    pub fn has_video(&self) -> bool {
        !self.video_encoder.supported_codecs().is_empty()
    }

    /// Whether any connected client negotiated `SURFACE_VIDEO`. Video is only
    /// produced while someone consumes it (a cheap, non-design guard).
    pub(crate) fn has_video_consumer(&self) -> bool {
        self.video_clients.values().any(|wants| *wants)
    }

    /// Encode one committed SHM frame for its window and publish it to the video
    /// plane. Root toplevel only for this milestone (subsurfaces/popups later).
    pub(crate) fn encode_video_frame(&mut self, frame: &SurfaceFrame) {
        if frame.width < crate::gpu::MIN_WIDTH || frame.height < crate::gpu::MIN_HEIGHT {
            return; // below the VCN minimum; padded composition is a later step
        }
        // Cheap guard (not the hybrid decision): only produce video while at
        // least one client wants it and the window is mapped.
        if !self.has_video_consumer() {
            return;
        }
        if !self
            .registry
            .window(&RemoteWindowId::new(&frame.window_id))
            .map(|window| window.mapped)
            .unwrap_or(false)
        {
            return;
        }
        let Some(sink) = self.video_sink.clone() else {
            return;
        };
        let window_id = RemoteWindowId::new(&frame.window_id);
        let encoder = self.video_encoder.clone();
        let config = VideoEncoderConfig {
            codec: VideoCodec::Hevc,
            chroma: VideoChroma::Nv12,
            width: frame.width,
            height: frame.height,
            frame_rate: 60,
            bitrate_kbps: 12_000,
            gop: 120,
            low_latency: true,
        };

        // Repack rows to tightly packed BGRA (drop stride padding). wl_shm
        // ARGB8888/XRGB8888 are BGRA/BGRX in memory, which is what ffmpeg's
        // `bgra` input expects.
        let stride = frame.stride as usize;
        let row = frame.width as usize * 4;
        let needed = stride * frame.height.saturating_sub(1) as usize + row;
        if frame.data.len() < needed {
            return;
        }
        let mut pixels = Vec::with_capacity(row * frame.height as usize);
        for y in 0..frame.height as usize {
            let offset = y * stride;
            pixels.extend_from_slice(&frame.data[offset..offset + row]);
        }

        let gpu = GpuFrame {
            window_id: frame.window_id.clone(),
            surface_id: frame.surface_id.clone(),
            width: frame.width,
            height: frame.height,
            format: DmabufFormat::new(0x3432_5241, 0), // ARGB8888
            source: FrameSource::Shm,
            sync: SyncState::Ready,
            planes: Vec::new(),
            data: Some(Bytes::from(pixels)),
        };

        let (outcome, codec_config) = {
            use std::collections::hash_map::Entry;
            let session = match self.video_sessions.entry(window_id.clone()) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(vacant) => match encoder.create_session(config) {
                    Ok(session) => vacant.insert(session),
                    Err(error) => {
                        tracing::warn!(window = %frame.window_id, %error, "video.session_failed");
                        return;
                    }
                },
            };
            if session.config().width != frame.width || session.config().height != frame.height {
                let _ = session.reconfigure(frame.width, frame.height);
            }
            let outcome = session.encode(&gpu);
            (outcome, session.take_codec_config())
        };

        match outcome {
            Ok(Some(encoded)) => {
                if let Some(codec_config) = codec_config {
                    sink.submit_config(
                        &frame.window_id,
                        VideoConfig {
                            codec: VideoCodec::Hevc,
                            chroma: VideoChroma::Nv12,
                            width: frame.width,
                            height: frame.height,
                            codec_config: codec_config.to_vec(),
                        },
                    );
                }
                sink.submit_frame(encoded);
            }
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(window = %frame.window_id, %error, "video.encode_failed");
                self.video_sessions.remove(&window_id);
            }
        }
    }

    /// Monotonic per-surface frame id.
    pub(crate) fn next_frame_id(&mut self, surface: RemoteSurfaceId) -> u64 {
        let entry = self.frame_ids.entry(surface).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Configure the long-term identity and per-run session id (from the TLS
    /// identity loaded at startup).
    pub fn set_identity(&mut self, session_id: String, identity_fingerprint: String) {
        self.server_session_id = session_id;
        self.server_identity = identity_fingerprint;
    }

    // MARK: - Adapter bookkeeping

    /// Stable domain id for a `wl_surface`, allocated on first sight.
    pub(crate) fn surface_id(&mut self, surface: &WlSurface) -> RemoteSurfaceId {
        let oid = surface.id();
        if let Some(id) = self.surfaces.get(&oid) {
            return *id;
        }
        self.next_surface += 1;
        let id = RemoteSurfaceId(self.next_surface);
        self.surfaces.insert(oid, id);
        self.surface_handles.insert(id, surface.clone());
        id
    }

    pub(crate) fn surface_handle(&self, id: RemoteSurfaceId) -> Option<&WlSurface> {
        self.surface_handles.get(&id)
    }

    pub(crate) fn client_key(&mut self, client: &Client) -> ClientKey {
        let cid = client.id();
        if let Some(key) = self.client_keys.get(&cid) {
            return *key;
        }
        self.next_client += 1;
        let key = ClientKey(self.next_client);
        self.client_keys.insert(cid, key);
        key
    }

    pub(crate) fn client_key_for_surface(&mut self, surface: &WlSurface) -> ClientKey {
        match self.display_handle.get_client(surface.id()) {
            Ok(client) => self.client_key(&client),
            Err(_) => ClientKey(0),
        }
    }

    pub(crate) fn toplevel(&self, surface: RemoteSurfaceId) -> Option<ToplevelSurface> {
        self.toplevels.get(&surface).cloned()
    }

    pub(crate) fn register_toplevel(
        &mut self,
        surface: RemoteSurfaceId,
        toplevel: ToplevelSurface,
    ) {
        self.toplevels.insert(surface, toplevel);
    }

    pub(crate) fn unregister_toplevel(&mut self, surface: RemoteSurfaceId) {
        self.toplevels.remove(&surface);
    }

    /// Root `wl_surface` of the window, if any.
    pub(crate) fn root_surface_of(&self, window: &RemoteWindowId) -> Option<WlSurface> {
        let sid = self.registry.root_surface_for_window(window)?;
        self.surface_handles.get(&sid).cloned()
    }

    /// Push domain events to the configured sink and to connected clients.
    pub(crate) fn emit(&mut self, events: Vec<RemoteEvent>) {
        for event in &events {
            self.sink.handle(event);
        }
        self.broadcast(&events);
    }

    // MARK: - Aqua Protocol v1 session

    /// Apply a network event delivered by the QUIC transport.
    pub fn handle_server_event(&mut self, event: ServerEvent) {
        match event {
            ServerEvent::ClientConnected {
                client_id,
                client_session_id,
                client_capabilities,
                outgoing,
            } => {
                tracing::info!(
                    client_id,
                    client_session_id = %client_session_id,
                    "handshake.server session={} revision={}",
                    self.server_session_id,
                    self.revision
                );
                // ServerHello
                let _ = outgoing.send(ServerMessage::Hello(v1::ServerHello {
                    protocol_version: crate::protocol::PROTOCOL_VERSION,
                    server_session_id: self.server_session_id.clone(),
                    server_identity: self.server_identity.clone(),
                    capabilities: Some(v1::Capabilities {
                        bits: self.capabilities,
                    }),
                    revision: self.revision,
                    error: String::new(),
                }));
                // ServerSnapshot taken at the current revision
                let windows = self
                    .registry
                    .windows()
                    .map(adapters::window_info)
                    .collect::<Vec<_>>();
                let surface_infos = self
                    .registry
                    .windows()
                    .flat_map(|window| {
                        self.registry
                            .window_surfaces(&window.id)
                            .iter()
                            .map(adapters::surface_info)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                let _ = outgoing.send(ServerMessage::Snapshot(v1::ServerSnapshot {
                    revision: self.revision,
                    server_session_id: self.server_session_id.clone(),
                    windows,
                    surfaces: surface_infos,
                }));
                tracing::info!(client_id, revision = self.revision, "snapshot.sent");
                self.video_clients.insert(
                    client_id,
                    client_capabilities & crate::protocol::capability::SURFACE_VIDEO != 0,
                );
                self.network_clients.insert(client_id, outgoing);
            }
            ServerEvent::ClientDisconnected { client_id } => {
                self.network_clients.remove(&client_id);
                self.video_clients.remove(&client_id);
                tracing::info!(client_id, "connection.removed");
            }
            ServerEvent::Command { client_id, command } => {
                self.apply_client_command(client_id, command);
            }
        }
    }

    /// Translate a domain event into a wire message and fan it out.
    ///
    /// `surface.commit` and popup events are intentionally not broadcast: the
    /// client cannot use pixels yet and popup lifetimes are already covered by
    /// `surface.created/destroyed` (see PHASE3A.md).
    fn broadcast(&mut self, events: &[RemoteEvent]) {
        if self.network_clients.is_empty() {
            return;
        }
        for event in events {
            let message = match event {
                RemoteEvent::WindowCreated { window } => {
                    self.revision += 1;
                    ServerMessage::WindowCreated(v1::WindowCreated {
                        revision: self.revision,
                        window: Some(adapters::window_info(window)),
                    })
                }
                RemoteEvent::WindowTitleChanged { id, title } => {
                    self.revision += 1;
                    ServerMessage::WindowTitleChanged(v1::WindowTitleChanged {
                        revision: self.revision,
                        window_id: id.to_string(),
                        title: title.clone().unwrap_or_default(),
                    })
                }
                RemoteEvent::WindowAppIdChanged { id, application_id } => {
                    self.revision += 1;
                    ServerMessage::WindowApplicationChanged(v1::WindowApplicationChanged {
                        revision: self.revision,
                        window_id: id.to_string(),
                        application_id: application_id
                            .as_ref()
                            .map(|a| a.as_str().to_string())
                            .unwrap_or_default(),
                    })
                }
                RemoteEvent::WindowStateChanged { id, state } => {
                    self.revision += 1;
                    ServerMessage::WindowStateChanged(v1::WindowStateChanged {
                        revision: self.revision,
                        window_id: id.to_string(),
                        state: adapters::state_to_u32(*state),
                    })
                }
                RemoteEvent::WindowMapped { id, mapped } => {
                    self.revision += 1;
                    ServerMessage::WindowMapped(v1::WindowMapped {
                        revision: self.revision,
                        window_id: id.to_string(),
                        mapped: *mapped,
                    })
                }
                RemoteEvent::WindowGeometryChanged { id, .. } => {
                    let Some(window) = self.registry.window(id) else {
                        continue;
                    };
                    let info = adapters::window_info(window);
                    self.revision += 1;
                    ServerMessage::WindowUpdated(v1::WindowUpdated {
                        revision: self.revision,
                        window: Some(info),
                    })
                }
                RemoteEvent::WindowClosed { id } => {
                    if let Some(sink) = &self.video_sink {
                        sink.drop_window(&id.to_string());
                    }
                    self.revision += 1;
                    ServerMessage::WindowClosed(v1::WindowClosed {
                        revision: self.revision,
                        window_id: id.to_string(),
                    })
                }
                RemoteEvent::SurfaceCreated { surface } => {
                    self.revision += 1;
                    ServerMessage::SurfaceCreated(v1::SurfaceCreated {
                        revision: self.revision,
                        surface: Some(adapters::surface_info(surface)),
                    })
                }
                RemoteEvent::SurfaceUpdated { surface } => {
                    self.revision += 1;
                    ServerMessage::SurfaceUpdated(v1::SurfaceUpdated {
                        revision: self.revision,
                        surface: Some(adapters::surface_info(surface)),
                    })
                }
                RemoteEvent::SurfaceDestroyed { id, window } => {
                    if let Some(sink) = &self.frame_sink {
                        sink.drop_surface(&id.to_string());
                    }
                    self.frame_ids.remove(id);
                    self.revision += 1;
                    ServerMessage::SurfaceDestroyed(v1::SurfaceDestroyed {
                        revision: self.revision,
                        surface_id: id.to_string(),
                        window_id: window.to_string(),
                    })
                }
                // Not broadcast on the control stream in phase 3B.
                RemoteEvent::SurfaceCommitted { .. }
                | RemoteEvent::PopupCreated { .. }
                | RemoteEvent::PopupDestroyed { .. }
                | RemoteEvent::ViewportChanged { .. } => continue,
            };

            for outgoing in self.network_clients.values() {
                let _ = outgoing.send(message.clone());
            }
        }
    }

    fn apply_client_command(&mut self, _client_id: u64, command: crate::net::ClientCommand) {
        use crate::net::ClientCommand;
        match command {
            ClientCommand::ViewportChanged {
                window_id,
                width,
                height,
                is_final,
                ..
            } => {
                tracing::info!(
                    window = %window_id,
                    width,
                    height,
                    is_final,
                    "viewport.received"
                );
                self.resize_window(&window_id, width, height);
            }
            ClientCommand::Focus { window_id } => self.focus_window(&window_id),
            ClientCommand::PointerMoved { window_id, x, y } => {
                self.pointer_focus = Some(RemoteWindowId::new(&window_id));
                self.inject_pointer_moved(x, y);
            }
            ClientCommand::PointerButton {
                window_id,
                button,
                pressed,
                ..
            } => {
                self.pointer_focus = Some(RemoteWindowId::new(&window_id));
                self.inject_pointer_button(button, pressed);
            }
            ClientCommand::PointerScroll {
                window_id, dx, dy, ..
            } => {
                self.pointer_focus = Some(RemoteWindowId::new(&window_id));
                self.inject_pointer_scroll(dx, dy);
            }
            ClientCommand::Key {
                window_id,
                keycode,
                characters,
                pressed,
                ..
            } => {
                self.keyboard_focus = Some(RemoteWindowId::new(&window_id));
                self.inject_key(&characters, keycode, pressed);
            }
            ClientCommand::Touch { window_id, .. } => {
                // wl_touch is not served yet (phase 2 seat has keyboard+pointer).
                tracing::debug!(window = %window_id, "touch.received (not yet mapped to wl_touch)");
            }
            ClientCommand::FramePresented {
                surface_id,
                frame_id,
                presentation_time_us,
            } => {
                // Presentation feedback. Records the round trip for metrics and,
                // in a later step, will drive `wl_surface.frame` callbacks.
                tracing::debug!(
                    target: "aqua::present",
                    surface_id = %surface_id,
                    frame_id,
                    presentation_time_us,
                    "frame.presented"
                );
            }
            ClientCommand::RequestKeyframe { window_id, reason } => {
                self.request_keyframe(&window_id, &reason);
            }
        }
    }

    /// Ask the per-window video encoder for a keyframe (decoder reset, dropped
    /// GOP, resize, reconnection). No-op on the SHM-only path.
    pub(crate) fn request_keyframe(&mut self, window_id: &str, reason: &str) {
        let id = RemoteWindowId::new(window_id);
        if let Some(session) = self.video_sessions.get_mut(&id) {
            session.request_keyframe();
        }
        tracing::debug!(window = %window_id, %reason, "video.keyframe_requested");
    }

    /// Drop adapter bookkeeping for surfaces that no longer belong to a window.
    pub(crate) fn reap_surface(&mut self, surface: RemoteSurfaceId) {
        self.surface_handles.remove(&surface);
    }

    pub(crate) fn handle_client_disconnected(&mut self, client_id: ClientId) {
        let Some(key) = self.client_keys.remove(&client_id) else {
            return;
        };
        let events = self.registry.client_disconnected(key);
        // Reap adapter-side handles for surfaces that were closed.
        for event in &events {
            if let RemoteEvent::SurfaceDestroyed { id, .. } = event {
                self.reap_surface(*id);
                self.unregister_toplevel(*id);
            }
        }
        if !events.is_empty() {
            tracing::info!(client = %key, "client disconnected, closed windows");
        }
        self.emit(events);
    }
}

/// Read a toplevel's committed title and app id from its role attributes.
pub(crate) fn read_toplevel_title_app_id(surface: &WlSurface) -> (Option<String>, Option<String>) {
    smithay::wayland::compositor::with_states(surface, |states| {
        match states.data_map.get::<XdgToplevelSurfaceData>() {
            Some(attrs) => {
                let attrs = attrs.lock().unwrap();
                (attrs.title.clone(), attrs.app_id.clone())
            }
            None => (None, None),
        }
    })
}

/// `(min, max)` size hints, each `None` when the client left the axis unconstrained.
pub(crate) type SizeHints = (Option<(i32, i32)>, Option<(i32, i32)>);

/// Read `(min, max)` size hints committed by the client.
pub(crate) fn read_size_hints(surface: &WlSurface) -> SizeHints {
    smithay::wayland::compositor::with_states(surface, |states| {
        let mut cached = states
            .cached_state
            .get::<smithay::wayland::shell::xdg::SurfaceCachedState>();
        let current = cached.current();
        let min = nonzero_size(current.min_size);
        let max = nonzero_size(current.max_size);
        (min, max)
    })
}

fn nonzero_size(size: smithay::utils::Size<i32, smithay::utils::Logical>) -> Option<(i32, i32)> {
    if size.w > 0 || size.h > 0 {
        Some((size.w, size.h))
    } else {
        None
    }
}

/// Describe the currently attached buffer of a surface, if any.
pub(crate) fn read_buffer_info(surface: &WlSurface) -> Option<RemoteBufferInfo> {
    smithay::wayland::compositor::with_states(surface, |states| {
        let mut cached = states.cached_state.get::<SurfaceAttributes>();
        match &cached.current().buffer {
            Some(BufferAssignment::NewBuffer(buffer)) => Some(buffer_info(buffer)),
            _ => None,
        }
    })
}

fn buffer_info(buffer: &wl_buffer::WlBuffer) -> RemoteBufferInfo {
    match smithay::wayland::shm::with_buffer_contents(buffer, |_ptr, _len, data| RemoteBufferInfo {
        width: data.width,
        height: data.height,
        shm: true,
    }) {
        Ok(info) => info,
        Err(_) => RemoteBufferInfo {
            width: 0,
            height: 0,
            shm: false,
        },
    }
}

/// Climb the subsurface tree to the root surface of a window.
pub(crate) fn root_surface(surface: &WlSurface) -> WlSurface {
    let mut root = surface.clone();
    while let Some(parent) = get_parent(&root) {
        root = parent;
    }
    root
}

/// Install the Wayland display into the `calloop` loop.
pub fn install_display_source(event_loop: &EventLoop<CalloopData>, display: Display<AquaState>) {
    event_loop
        .handle()
        .insert_source(
            Generic::new(display, Interest::READ, Mode::Level),
            |_, display, data| {
                // Safety: the display is never dropped while the loop runs.
                unsafe {
                    display.get_mut().dispatch_clients(&mut data.state).unwrap();
                }
                data.state.display_handle.flush_clients().unwrap();
                Ok(PostAction::Continue)
            },
        )
        .expect("failed to insert the Wayland display source");
}

/// Helper to share a `Sender<ControlMessage>` with a `ClientState`.
pub type ControlSender = Sender<ControlMessage>;

/// Convenience: an `Arc<ClientState>` with a cloned sender.
pub fn client_state(control_tx: &ControlSender) -> Arc<ClientState> {
    Arc::new(ClientState::new(control_tx.clone()))
}
