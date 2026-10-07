//! Aqua server (phase 2).
//!
//! A real, headless Wayland server built on Smithay. It accepts real Wayland
//! clients, understands `xdg_toplevel`, and maps each one to a neutral
//! [`domain::RemoteWindow`]. It does not render, capture, or stream pixels, and
//! it does not speak to the iPad yet.

pub mod control;
pub mod domain;
pub mod events;
pub mod gpu;
pub mod net;
pub mod protocol;
pub mod wayland;

use std::net::SocketAddr;
use std::time::Duration;

use smithay::reexports::{
    calloop::{
        channel,
        timer::{TimeoutAction, Timer},
        EventLoop,
    },
    wayland_server::Display,
};
use smithay::wayland::socket::ListeningSocketSource;

use net::{FrameHub, NetworkConfig, NetworkServer, ServerEvent};
use wayland::{client_state, install_display_source, AquaState, CalloopData, ControlMessage};

/// Socket name clients connect to (`WAYLAND_DISPLAY=wayland-aqua`).
pub const SOCKET_NAME: &str = "wayland-aqua";

/// Development TLS identity (see `certs/` and `docs/TRANSPORT.md`).
const DEV_CERT_PEM: &[u8] = include_bytes!("../certs/dev-cert.pem");
const DEV_KEY_PEM: &[u8] = include_bytes!("../certs/dev-key.pem");

/// Default QUIC bind address. Loopback only: never expose publicly by default.
pub const DEFAULT_BIND: &str = "127.0.0.1:52420";

/// Run the Aqua Wayland server until the control channel asks it to stop.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    ensure_runtime_dir()?;

    let mut event_loop: EventLoop<CalloopData> = EventLoop::try_new()?;
    let display: Display<AquaState> = Display::new()?;
    let display_handle = display.handle();
    let (control_tx, control_rx) = channel::channel::<ControlMessage>();

    let state = AquaState::new(&mut event_loop, &display_handle);
    let mut data = CalloopData {
        state,
        display_handle: display_handle.clone(),
    };

    // --- QUIC transport (Aqua Protocol v1) -----------------------------------
    let bind: SocketAddr = std::env::var("AQUA_BIND")
        .unwrap_or_else(|_| DEFAULT_BIND.to_string())
        .parse()
        .map_err(|e| format!("invalid AQUA_BIND: {e}"))?;
    let (net_tx, net_rx) = channel::channel::<ServerEvent>();
    let frame_hub = FrameHub::new();
    let video_hub = net::VideoHub::new();
    data.state
        .set_frame_sink(frame_hub.clone() as std::sync::Arc<dyn net::FrameSink>);
    data.state
        .set_video_sink(video_hub.clone() as std::sync::Arc<dyn net::VideoSink>);
    let network = NetworkServer::spawn(
        NetworkConfig {
            bind,
            cert_pem: DEV_CERT_PEM.to_vec(),
            key_pem: DEV_KEY_PEM.to_vec(),
        },
        net_tx,
        frame_hub,
        video_hub,
    )?;
    let session_id = uuid::Uuid::new_v4().to_string();
    data.state.set_identity(
        session_id.clone(),
        network.identity.fingerprint_sha256.clone(),
    );

    // Phase 3C GPU/video switch. The default build has no GPU
    // (`docs/GPU_PIPELINE.md`), so the null importer/encoder keep Aqua on the
    // SHM-only path: no `zwp_linux_dmabuf_v1` global, no `SURFACE_VIDEO`.
    // A real RTX 2060 build substitutes a concrete importer/encoder here.
    data.state.install_gpu(
        std::sync::Arc::new(gpu::NullGpuImporter),
        std::sync::Arc::new(gpu::NullVideoEncoder),
    );
    event_loop
        .handle()
        .insert_source(net_rx, |event, _, data| {
            if let channel::Event::Msg(server_event) = event {
                data.state.handle_server_event(server_event);
                let _ = data.state.display_handle.flush_clients();
            }
        })
        .expect("failed to insert the network event source");

    // --- Wayland listening socket -------------------------------------------
    let socket = ListeningSocketSource::with_name(SOCKET_NAME)
        .or_else(|_| ListeningSocketSource::new_auto())?;
    let socket_name = socket.socket_name().to_os_string();
    {
        let tx = control_tx.clone();
        event_loop
            .handle()
            .insert_source(socket, move |stream, _, data| {
                if let Err(error) = data.display_handle.insert_client(stream, client_state(&tx)) {
                    tracing::warn!(%error, "failed to insert Wayland client");
                }
            })
            .expect("failed to insert the listening socket source");
    }

    // --- Wayland display -----------------------------------------------------
    install_display_source(&event_loop, display);

    // --- Control channel -----------------------------------------------------
    event_loop
        .handle()
        .insert_source(control_rx, |event, _, data| {
            if let channel::Event::Msg(message) = event {
                data.state.handle_control(message);
                // Input injection and configure requests queue protocol
                // messages; flush them immediately. The display source only
                // flushes when clients are readable, which is not the case
                // right after a synthetic input event.
                let _ = data.state.display_handle.flush_clients();
            }
        })
        .expect("failed to insert the control channel source");

    // --- stdin reader (demo control) -----------------------------------------
    {
        let tx = control_tx.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                match line {
                    Ok(line) => {
                        if tx.send(ControlMessage::Command(line)).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // --- Frame clock ---------------------------------------------------------
    let frame_interval = Duration::from_millis(16);
    let timer = Timer::from_duration(frame_interval);
    event_loop
        .handle()
        .insert_source(timer, move |_, _, data| {
            data.state.send_frames();
            let _ = data.state.display_handle.flush_clients();
            TimeoutAction::ToDuration(frame_interval)
        })
        .expect("failed to insert the frame timer");

    print_banner(&socket_name.to_string_lossy(), &network, &session_id);

    event_loop.run(None, &mut data, |_| {})?;
    Ok(())
}

fn init_tracing() {
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .try_init();
}

/// Wayland requires `XDG_RUNTIME_DIR`. On a bare TTY/SSH box it may be unset,
/// so create a private one. This is not a hack: it is the standard per-user
/// runtime directory the protocol expects.
fn ensure_runtime_dir() -> std::io::Result<()> {
    if std::env::var_os("XDG_RUNTIME_DIR").is_some() {
        return Ok(());
    }
    let dir = std::env::temp_dir().join(format!("aqua-runtime-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    std::env::set_var("XDG_RUNTIME_DIR", &dir);
    eprintln!(
        "warning: XDG_RUNTIME_DIR was unset; created {}",
        dir.display()
    );
    Ok(())
}

fn print_banner(socket_name: &str, network: &NetworkServer, session_id: &str) {
    println!("Aqua Wayland server");
    println!("socket = {socket_name}");
    println!();
    println!("Aqua QUIC transport (protocol v1)");
    println!("  address     = {}", network.local_addr);
    println!("  alpn        = aqua/1");
    println!("  session     = {session_id}");
    println!("  server      = {}", network.identity.fingerprint_sha256);
    println!("  set AQUA_BIND=<ip:port> to change (default {DEFAULT_BIND})");
    println!();
    println!("Run a client with:");
    println!("    WAYLAND_DISPLAY={socket_name} <wayland-client>");
    println!();
    println!("Control channel (stdin):");
    println!("    list");
    println!("    resize <window-id> <width> <height>");
    println!("    focus <window-id>");
    println!("    pointer move <x> <y>");
    println!("    pointer button <left|right|middle> <down|up>");
    println!("    key <char> <down|up>");
    println!("    quit");
}
