//! VA-API dmabuf encoder driven by the `aqua-va-encode` sidecar.
//!
//! The sidecar (built from the C probe: `DRM_PRIME_2` import + VPP ARGB→NV12 +
//! in-process libavcodec `hevc_vaapi`/`h264_vaapi`) listens on a Unix socket and
//! receives, per frame, the **dmabuf plane fds** via `SCM_RIGHTS` plus their
//! layout. It returns Annex-B access units. No pixel ever crosses the CPU.
//!
//! ## Wire protocol (little-endian)
//!
//! The server **binds and listens** on `$AQUA_VA_SOCKET` (or a unique tmp path),
//! spawns the sidecar with that env var, and the sidecar connects.
//!
//! Request (header + plane descriptors, fds via `SCM_RIGHTS`):
//!
//! ```text
//! u32 magic=0x4156_4131 | u8 kind(1=FRAME,2=FLUSH,3=SHUTDOWN) | u8 codec(1=h264,2=hevc)
//! u16 rsv | u32 fourcc | u32 width | u32 height | u64 frame_id | u64 pts_us
//! u8 keyframe | u8 num_planes | u16 rsv2
//! [u32 offset, u32 stride, u64 modifier] * num_planes
//! ```
//!
//! Response (header + payload, no fds):
//!
//! ```text
//! u32 magic=0x4156_4131 | u8 kind(10=CONFIG,11=FRAME,12=ERROR,13=FLUSHED)
//! u8 codec | u16 rsv | u32 width | u32 height | u64 frame_id
//! u8 keyframe | u8[3] rsv | u32 payload_len | payload[...] (Annex-B)
//! ```
//!
//! `CONFIG` payload is the parameter-set-only Annex-B (VPS/SPS/PPS). `FRAME`
//! payload is one access unit. `ERROR` payload is a UTF-8 message.

use std::{
    io::{Read, Write},
    os::fd::{AsRawFd, RawFd},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{channel, Receiver, TryRecvError},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use bytes::Bytes;

use super::{
    EncodeError, EncodedFrame, GpuFrame, VideoCodec, VideoEncoder, VideoEncoderConfig,
    VideoEncoderSession,
};

pub const MAGIC: u32 = 0x4156_4131;
pub const KIND_FRAME: u8 = 1;
#[allow(dead_code)] // part of the documented protocol
pub const KIND_FLUSH: u8 = 2;
pub const KIND_SHUTDOWN: u8 = 3;
pub const KIND_CONFIG: u8 = 10;
pub const KIND_RESPONSE_FRAME: u8 = 11;
pub const KIND_ERROR: u8 = 12;
#[allow(dead_code)] // part of the documented protocol
pub const KIND_FLUSHED: u8 = 13;

const REQ_HEADER_LEN: usize = 40;
const RESP_HEADER_LEN: usize = 32;

/// Encode a request header plus one descriptor per plane (16 bytes each).
#[allow(clippy::too_many_arguments)]
pub fn encode_request(
    kind: u8,
    codec: u8,
    fourcc: u32,
    width: u32,
    height: u32,
    frame_id: u64,
    pts_us: u64,
    keyframe: bool,
    planes: &[(u32, u32, u64)],
) -> Vec<u8> {
    let mut out = Vec::with_capacity(REQ_HEADER_LEN + planes.len() * 16);
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.push(kind);
    out.push(codec);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&fourcc.to_le_bytes());
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(&frame_id.to_le_bytes());
    out.extend_from_slice(&pts_us.to_le_bytes());
    out.push(u8::from(keyframe));
    out.push(planes.len() as u8);
    out.extend_from_slice(&0u16.to_le_bytes());
    for (offset, stride, modifier) in planes {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&stride.to_le_bytes());
        out.extend_from_slice(&modifier.to_le_bytes());
    }
    out
}

/// A decoded sidecar response.
#[derive(Debug)]
#[allow(dead_code)] // codec/width/height are part of the protocol, not all read yet
pub struct Response {
    pub kind: u8,
    pub codec: u8,
    pub width: u32,
    pub height: u32,
    pub frame_id: u64,
    pub keyframe: bool,
    pub payload: Bytes,
}

/// Decode a response header and return it with its payload.
pub fn decode_response(buf: &[u8]) -> Option<Response> {
    if buf.len() < RESP_HEADER_LEN {
        return None;
    }
    let magic = u32::from_le_bytes(buf[0..4].try_into().ok()?);
    if magic != MAGIC {
        return None;
    }
    let kind = buf[4];
    let codec = buf[5];
    let width = u32::from_le_bytes(buf[8..12].try_into().ok()?);
    let height = u32::from_le_bytes(buf[12..16].try_into().ok()?);
    let frame_id = u64::from_le_bytes(buf[16..24].try_into().ok()?);
    let keyframe = buf[24] != 0;
    let payload_len = u32::from_le_bytes(buf[28..32].try_into().ok()?) as usize;
    if buf.len() < RESP_HEADER_LEN + payload_len {
        return None;
    }
    Some(Response {
        kind,
        codec,
        width,
        height,
        frame_id,
        keyframe,
        payload: Bytes::copy_from_slice(&buf[RESP_HEADER_LEN..RESP_HEADER_LEN + payload_len]),
    })
}

fn send_with_fds(stream: &UnixStream, data: &[u8], fds: &[RawFd]) -> std::io::Result<()> {
    let mut iov = libc::iovec {
        iov_base: data.as_ptr() as *mut libc::c_void,
        iov_len: data.len(),
    };
    let cmsg_space = unsafe { libc::CMSG_SPACE((fds.len() * 4) as u32) as usize };
    let mut cmsg_buf = vec![0u8; cmsg_space];
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    if !fds.is_empty() {
        msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = cmsg_space;
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN((fds.len() * 4) as u32) as usize;
            std::ptr::copy_nonoverlapping(
                fds.as_ptr() as *const u8,
                libc::CMSG_DATA(cmsg),
                fds.len() * 4,
            );
        }
    }
    let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &msg, 0) };
    if sent < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

/// Factory for the `aqua-va-encode` sidecar encoder.
pub struct VaSidecarEncoder {
    binary: PathBuf,
}

impl VaSidecarEncoder {
    /// Binary path from `AQUA_VA_ENCODE`, defaulting to a per-user location.
    pub fn from_env() -> Self {
        let binary = std::env::var("AQUA_VA_ENCODE").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".aqua-p2p/aqua-va-encode")
        });
        Self { binary }
    }

    pub fn is_available(&self) -> bool {
        self.binary.is_file()
    }

    fn binary(&self) -> &Path {
        &self.binary
    }
}

impl VideoEncoder for VaSidecarEncoder {
    fn name(&self) -> &str {
        "va-sidecar"
    }

    fn supported_codecs(&self) -> &[VideoCodec] {
        &[VideoCodec::Hevc, VideoCodec::H264]
    }

    fn supports_dmabuf(&self) -> bool {
        true
    }

    fn create_session(
        &self,
        config: VideoEncoderConfig,
    ) -> Result<Box<dyn VideoEncoderSession>, EncodeError> {
        if !self.is_available() {
            return Err(EncodeError::NoEncoder);
        }
        if !matches!(config.codec, VideoCodec::Hevc | VideoCodec::H264) {
            return Err(EncodeError::UnsupportedCodec(config.codec));
        }
        VaSidecarSession::spawn(self.binary(), config).map(|s| Box::new(s) as _)
    }
}

struct VaSidecarSession {
    config: VideoEncoderConfig,
    child: Option<Child>,
    write_stream: UnixStream,
    responses: Receiver<Response>,
    reader: Option<JoinHandle<()>>,
    socket_path: PathBuf,
    codec_config: Option<Bytes>,
    force_keyframe: bool,
    frame_id: u64,
    pts_us: u64,
    window_id: String,
}

impl VaSidecarSession {
    fn spawn(binary: &Path, config: VideoEncoderConfig) -> Result<Self, EncodeError> {
        let socket_path = std::env::temp_dir().join(format!(
            "aqua-va-{}-{}.sock",
            std::process::id(),
            next_socket_id()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| EncodeError::Backend(format!("bind {}: {e}", socket_path.display())))?;

        let mut child = Command::new(binary)
            .env("AQUA_VA_SOCKET", &socket_path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| EncodeError::Backend(format!("spawn {}: {e}", binary.display())))?;

        let stream = accept_with_timeout(&listener, Duration::from_secs(5)).map_err(|e| {
            let _ = child.kill();
            EncodeError::Backend(format!("sidecar did not connect: {e}"))
        })?;
        let _ = std::fs::remove_file(&socket_path);

        let read_stream = stream
            .try_clone()
            .map_err(|e| EncodeError::Backend(format!("clone socket: {e}")))?;
        let (tx, rx) = channel::<Response>();
        let reader = std::thread::Builder::new()
            .name("aqua-va-reader".into())
            .spawn(move || read_responses(read_stream, tx))
            .map_err(|e| EncodeError::Backend(format!("reader thread: {e}")))?;

        Ok(Self {
            config,
            child: Some(child),
            write_stream: stream,
            responses: rx,
            reader: Some(reader),
            socket_path,
            codec_config: None,
            force_keyframe: false,
            frame_id: 0,
            pts_us: 0,
            window_id: String::new(),
        })
    }

    fn send_header(&mut self, kind: u8, keyframe: bool) -> Result<(), EncodeError> {
        let request = encode_request(
            kind,
            self.config.codec.to_wire() as u8,
            0,
            self.config.width,
            self.config.height,
            self.frame_id,
            self.pts_us,
            keyframe,
            &[],
        );
        self.write_stream
            .write_all(&request)
            .map_err(|e| EncodeError::Backend(format!("write: {e}")))
    }
}

impl VideoEncoderSession for VaSidecarSession {
    fn config(&self) -> &VideoEncoderConfig {
        &self.config
    }

    fn encode(&mut self, frame: &GpuFrame) -> Result<Option<EncodedFrame>, EncodeError> {
        self.window_id = frame.window_id.clone();
        if frame.planes.is_empty() {
            return Err(EncodeError::Backend("dmabuf frame has no planes".into()));
        }
        let keyframe = self.force_keyframe;
        self.force_keyframe = false;

        let planes: Vec<(u32, u32, u64)> = frame
            .planes
            .iter()
            .map(|p| (p.offset, p.stride, p.modifier))
            .collect();
        let fds: Vec<RawFd> = frame.planes.iter().map(|p| p.fd.as_raw_fd()).collect();
        let request = encode_request(
            KIND_FRAME,
            self.config.codec.to_wire() as u8,
            frame.format.fourcc,
            frame.width,
            frame.height,
            self.frame_id,
            self.pts_us,
            keyframe,
            &planes,
        );
        send_with_fds(&self.write_stream, &request, &fds)
            .map_err(|e| EncodeError::Backend(format!("sendmsg: {e}")))?;
        self.frame_id += 1;
        self.pts_us += 1_000_000 / u64::from(self.config.frame_rate.max(1));

        // Drain responses; keep a CONFIG, return the first FRAME (or None).
        loop {
            match self.responses.try_recv() {
                Ok(response) if response.kind == KIND_CONFIG => {
                    tracing::debug!(target: "aqua::dmabuf", len = response.payload.len(), "video.sidecar_response config (encode)");
                    self.codec_config = Some(response.payload);
                }
                Ok(response) if response.kind == KIND_RESPONSE_FRAME => {
                    tracing::debug!(target: "aqua::dmabuf", frame_id = response.frame_id, key = response.keyframe, "video.sidecar_response frame (encode)");
                    return Ok(Some(EncodedFrame {
                        window_id: frame.window_id.clone(),
                        frame_id: response.frame_id,
                        keyframe: response.keyframe,
                        codec_config: false,
                        pts_us: self.pts_us.saturating_sub(1_000_000),
                        data: response.payload,
                    }));
                }
                Ok(response) if response.kind == KIND_ERROR => {
                    return Err(EncodeError::Backend(
                        String::from_utf8_lossy(&response.payload).into_owned(),
                    ));
                }
                Ok(response) => {
                    tracing::debug!(target: "aqua::dmabuf", kind = response.kind, "video.sidecar_response other");
                }
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => {
                    return Err(EncodeError::Backend("sidecar channel closed".into()));
                }
            }
        }
    }

    fn take_codec_config(&mut self) -> Option<Bytes> {
        self.codec_config.take()
    }

    fn poll(&mut self) -> Option<EncodedFrame> {
        loop {
            match self.responses.try_recv() {
                Ok(response) if response.kind == KIND_CONFIG => {
                    tracing::debug!(target: "aqua::dmabuf", len = response.payload.len(), "video.sidecar_response config");
                    self.codec_config = Some(response.payload);
                }
                Ok(response) if response.kind == KIND_RESPONSE_FRAME => {
                    tracing::debug!(target: "aqua::dmabuf", frame_id = response.frame_id, key = response.keyframe, len = response.payload.len(), "video.sidecar_response frame");
                    return Some(EncodedFrame {
                        window_id: self.window_id.clone(),
                        frame_id: response.frame_id,
                        keyframe: response.keyframe,
                        codec_config: false,
                        pts_us: self.pts_us,
                        data: response.payload,
                    });
                }
                Ok(response) if response.kind == KIND_ERROR => {
                    tracing::warn!(
                        window = %self.window_id,
                        error = %String::from_utf8_lossy(&response.payload),
                        "video.sidecar_error"
                    );
                }
                Ok(_) => {}
                Err(_) => return None,
            }
        }
    }

    fn request_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    fn reconfigure(&mut self, width: u32, height: u32) -> Result<(), EncodeError> {
        // The sidecar reads width/height on every request and reconfigures.
        self.config.width = width;
        self.config.height = height;
        self.codec_config = None;
        Ok(())
    }
}

impl Drop for VaSidecarSession {
    fn drop(&mut self) {
        let _ = self.send_header(KIND_SHUTDOWN, false);
        let _ = self.write_stream.flush();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

fn read_responses(mut stream: UnixStream, tx: std::sync::mpsc::Sender<Response>) {
    loop {
        let mut header = [0u8; RESP_HEADER_LEN];
        if stream.read_exact(&mut header).is_err() {
            return;
        }
        let payload_len =
            u32::from_le_bytes(header[28..32].try_into().expect("4 bytes")) as usize;
        let mut payload = vec![0u8; payload_len];
        if stream.read_exact(&mut payload).is_err() {
            return;
        }
        let mut full = Vec::with_capacity(RESP_HEADER_LEN + payload_len);
        full.extend_from_slice(&header);
        full.extend_from_slice(&payload);
        if let Some(response) = decode_response(&full) {
            if tx.send(response).is_err() {
                return;
            }
        }
    }
}

fn accept_with_timeout(listener: &UnixListener, timeout: Duration) -> std::io::Result<UnixStream> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false)?;
                return Ok(stream);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "timeout",
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(error) => return Err(error),
        }
    }
}

fn next_socket_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trips_plane_layout() {
        let bytes = encode_request(
            KIND_FRAME,
            VideoCodec::Hevc.to_wire() as u8,
            0x3432_5241, // AR24
            800,
            600,
            42,
            1_000_000,
            true,
            &[(0, 3200, 0), (262_144, 512, 0x0200_0004_4051_ba01)],
        );
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), MAGIC);
        assert_eq!(bytes[4], KIND_FRAME);
        assert_eq!(bytes[5], 2);
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            0x3432_5241
        );
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 800);
        assert_eq!(u64::from_le_bytes(bytes[20..28].try_into().unwrap()), 42);
        assert_eq!(bytes[36], 1); // keyframe
        assert_eq!(bytes[37], 2); // two planes
        assert_eq!(bytes.len(), REQ_HEADER_LEN + 32);
    }

    #[test]
    fn response_decodes_payload() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&MAGIC.to_le_bytes());
        buf.push(KIND_RESPONSE_FRAME);
        buf.push(2);
        buf.extend_from_slice(&0u16.to_le_bytes());
        buf.extend_from_slice(&250u32.to_le_bytes());
        buf.extend_from_slice(&250u32.to_le_bytes());
        buf.extend_from_slice(&7u64.to_le_bytes());
        buf.push(1);
        buf.extend_from_slice(&[0, 0, 0]);
        buf.extend_from_slice(&3u32.to_le_bytes());
        buf.extend_from_slice(&[0xaa, 0xbb, 0xcc]);
        let response = decode_response(&buf).expect("decode");
        assert_eq!(response.kind, KIND_RESPONSE_FRAME);
        assert_eq!(response.frame_id, 7);
        assert!(response.keyframe);
        assert_eq!(&response.payload[..], &[0xaa, 0xbb, 0xcc]);
    }
}
