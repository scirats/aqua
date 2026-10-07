//! VA-API (AMD VCN) H.264/HEVC encoder behind the neutral [`VideoEncoder`] trait.
//!
//! The hardware encoder is driven through an `ffmpeg` subprocess because it is a
//! pragmatic, well-tested way to reach VA-API without hand-rolling the whole
//! `libva` encode API. The subprocess is an implementation detail: nothing but
//! neutral types ([`EncodedFrame`], Annex-B `Bytes`) crosses this module.
//!
//! ## Why this satisfies the phase contract
//!
//! For the **SHM** path the pixels are already CPU memory, so piping them to
//! ffmpeg is not a readback (it is an upload). The dmabuf path (no CPU readback)
//! is a later step that will import into a VA-API surface directly.
//!
//! ## Wire shape
//!
//! ffmpeg is asked for a raw Annex-B elementary stream with Access Unit
//! Delimiters inserted (`hevc_metadata=aud=insert` / `h264_metadata=aud=insert`)
//! and **no B-frames** (`-bf 0`), so every encoded picture is one access unit
//! that begins with an AUD NAL. That makes AU framing a pure function (see
//! [`tests`]), and a decoder never sees reordering.
//!
//! ## Keyframes / resize
//!
//! `ffmpeg` CLI has no clean "force IDR now" knob, so [`FfmpegVaapiSession`]
//! restarts the subprocess on `request_keyframe()` / `reconfigure()`. The first
//! access unit of a fresh process is an IRAP with VPS/SPS/PPS, which the caller
//! sends as `CONFIG` before the frame. Restart is rare (stream start, decoder
//! reset, dropped GOP, resize), so the cost is acceptable for this milestone.

use std::{
    io::{Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{channel, Receiver, TryRecvError},
    thread::JoinHandle,
};

use bytes::{Bytes, BytesMut};

use super::{
    EncodeError, EncodedFrame, GpuFrame, VideoCodec, VideoEncoder, VideoEncoderConfig,
    VideoEncoderSession,
};

/// Minimum encode dimensions of the AMD VCN on this class of APU (queried from
/// the driver; see the phase notes). Smaller windows must be padded upstream.
pub const MIN_WIDTH: u32 = 130;
pub const MIN_HEIGHT: u32 = 128;

// ---------------------------------------------------------------------------
// Annex-B parsing (pure)
// ---------------------------------------------------------------------------

fn start_code_len(data: &[u8], at: usize) -> Option<usize> {
    match data.get(at..at + 4) {
        Some([0, 0, 0, 1]) => Some(4),
        _ => match data.get(at..at + 3) {
            Some([0, 0, 1]) => Some(3),
            _ => None,
        },
    }
}

fn nal_type(codec: VideoCodec, nal: &[u8]) -> Option<u8> {
    let first = *nal.first()?;
    Some(match codec {
        VideoCodec::H264 => first & 0x1f,
        // HEVC nal_unit_type is bits 1..6 of the first byte.
        VideoCodec::Hevc | VideoCodec::Av1 => (first >> 1) & 0x3f,
    })
}

fn is_aud(codec: VideoCodec, nal: &[u8]) -> bool {
    matches!((codec, nal_type(codec, nal)), (VideoCodec::H264, Some(9)) | (VideoCodec::Hevc, Some(35)))
}

fn is_parameter_set(codec: VideoCodec, nal: &[u8]) -> bool {
    matches!(
        (codec, nal_type(codec, nal)),
        (VideoCodec::H264, Some(7..=8)) | (VideoCodec::Hevc, Some(32..=34))
    )
}

fn is_irap(codec: VideoCodec, nal: &[u8]) -> bool {
    match (codec, nal_type(codec, nal)) {
        // H.264: IDR (5). HEVC: BLA/IDR/CRA (16..=21).
        (VideoCodec::H264, Some(5)) => true,
        (VideoCodec::Hevc, Some(t)) if (16..=21).contains(&t) => true,
        _ => false,
    }
}

/// Start offsets of every AUD start code in `data`.
fn aud_offsets(codec: VideoCodec, data: &[u8]) -> Vec<usize> {
    let mut offsets = Vec::new();
    for (nal_start, start_code) in nal_spans(codec, data) {
        if is_aud(codec, &data[nal_start..]) {
            offsets.push(start_code);
        }
    }
    offsets
}

/// `(nal_start, start_code_offset)` for every NAL in an Annex-B buffer.
fn nal_spans(_codec: VideoCodec, data: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut i = 0;
    let mut current: Option<(usize, usize)> = None; // (nal_start, start_code_offset)
    while i < data.len() {
        if let Some(len) = start_code_len(data, i) {
            if let Some((nal_start, sc)) = current.take() {
                spans.push((nal_start, sc));
            }
            current = Some((i + len, i));
            i += len;
        } else {
            i += 1;
        }
    }
    if let Some((nal_start, sc)) = current {
        spans.push((nal_start, sc));
    }
    spans
}

/// Remove and return every complete access unit from `buf`, keeping the trailing
/// (incomplete) AU in `buf`. The first AU keeps any leading parameter-set
/// extradata that precedes the first AUD.
fn drain_aus(codec: VideoCodec, buf: &mut BytesMut) -> Vec<Bytes> {
    let auds = aud_offsets(codec, buf);
    if auds.len() < 2 {
        return Vec::new();
    }
    let end = *auds.last().unwrap();
    let head = buf.split_to(end).freeze();
    let mut out = Vec::new();
    // First AU spans from the very start (may include VPS/SPS/PPS) to the 2nd AUD.
    out.push(head.slice(0..auds[1]));
    for pair in auds.windows(2).skip(1) {
        out.push(head.slice(pair[0]..pair[1]));
    }
    out
}

/// Concatenate the parameter-set NALs (with their start codes) of one AU.
fn parameter_sets(codec: VideoCodec, au: &[u8]) -> Bytes {
    let mut out = BytesMut::new();
    for (nal_start, start_code) in nal_spans(codec, au) {
        if is_parameter_set(codec, &au[nal_start..]) {
            out.extend_from_slice(&au[start_code..nal_start]);
            // include NAL bytes up to the next start code
            let end = next_start_code(codec, au, nal_start).unwrap_or(au.len());
            out.extend_from_slice(&au[nal_start..end]);
        }
    }
    out.freeze()
}

fn next_start_code(_codec: VideoCodec, data: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i < data.len() {
        if start_code_len(data, i).is_some() {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn au_is_keyframe(codec: VideoCodec, au: &[u8]) -> bool {
    nal_spans(codec, au)
        .into_iter()
        .any(|(nal_start, _)| is_irap(codec, &au[nal_start..]))
}

// ---------------------------------------------------------------------------
// ffmpeg-backed encoder
// ---------------------------------------------------------------------------

/// Factory that spawns one ffmpeg/VA-API process per window video stream.
pub struct FfmpegVaapiEncoder {
    ffmpeg: PathBuf,
    device: String,
    lib_path: Option<String>,
}

impl FfmpegVaapiEncoder {
    /// Build from the environment: `AQUA_FFMPEG` (binary path, default
    /// `~/.local/ffmpeg-root/usr/bin/ffmpeg`) and `AQUA_VAAPI_DEVICE`
    /// (default `/dev/dri/renderD128`).
    pub fn from_env() -> Self {
        let ffmpeg = std::env::var("AQUA_FFMPEG")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                PathBuf::from(std::env::var("HOME").unwrap_or_default())
                    .join(".local/ffmpeg-root/usr/bin/ffmpeg")
            });
        let device =
            std::env::var("AQUA_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".to_string());
        let lib_path = ffmpeg
            .parent()
            .and_then(|bin| bin.parent())
            .map(|usr| usr.join("lib/x86_64-linux-gnu"))
            .filter(|p| p.is_dir())
            .map(|p| {
                format!("{}:/usr/lib/x86_64-linux-gnu", p.display())
            });
        Self {
            ffmpeg,
            device,
            lib_path,
        }
    }

    /// Whether the ffmpeg binary exists (the encoder is usable).
    pub fn is_available(&self) -> bool {
        self.ffmpeg.is_file()
    }
}

impl VideoEncoder for FfmpegVaapiEncoder {
    fn name(&self) -> &str {
        "ffmpeg-vaapi"
    }

    fn supported_codecs(&self) -> &[VideoCodec] {
        &[VideoCodec::Hevc, VideoCodec::H264]
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
        Ok(Box::new(FfmpegVaapiSession::new(
            config,
            self.ffmpeg.clone(),
            self.device.clone(),
            self.lib_path.clone(),
        )))
    }
}

struct FfmpegVaapiSession {
    config: VideoEncoderConfig,
    ffmpeg: PathBuf,
    device: String,
    lib_path: Option<String>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    aus: Option<Receiver<Bytes>>,
    reader: Option<JoinHandle<()>>,
    codec_config: Option<Bytes>,
    keyframe_requested: bool,
    frame_id: u64,
    pts_us: u64,
    window_id: String,
}

impl FfmpegVaapiSession {
    fn new(
        config: VideoEncoderConfig,
        ffmpeg: PathBuf,
        device: String,
        lib_path: Option<String>,
    ) -> Self {
        Self {
            config,
            ffmpeg,
            device,
            lib_path,
            child: None,
            stdin: None,
            aus: None,
            reader: None,
            codec_config: None,
            keyframe_requested: false,
            frame_id: 0,
            pts_us: 0,
            window_id: String::new(),
        }
    }

    fn codec_name(&self) -> &'static str {
        match self.config.codec {
            VideoCodec::H264 => "h264_vaapi",
            _ => "hevc_vaapi",
        }
    }

    fn profile(&self) -> &'static str {
        match self.config.codec {
            VideoCodec::H264 => "high",
            _ => "main",
        }
    }

    fn format(&self) -> &'static str {
        match self.config.codec {
            VideoCodec::H264 => "h264",
            _ => "hevc",
        }
    }

    fn bsf(&self) -> String {
        match self.config.codec {
            VideoCodec::H264 => "h264_metadata=aud=insert".to_string(),
            _ => "hevc_metadata=aud=insert".to_string(),
        }
    }

    fn stop_child(&mut self) {
        self.stdin = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.aus = None;
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }

    fn spawn(&mut self) -> Result<(), EncodeError> {
        let width = self.config.width;
        let height = self.config.height;
        let rate = self.config.frame_rate.max(1);
        let bitrate = self.config.bitrate_kbps.max(100);
        let gop = if self.config.gop == 0 {
            120
        } else {
            self.config.gop
        };
        let codec_name = self.codec_name();
        let profile = self.profile();
        let format = self.format();
        let bsf = self.bsf();

        let mut command = Command::new(&self.ffmpeg);
        command
            .arg("-hide_banner")
            .arg("-loglevel")
            .arg("error")
            .arg("-f")
            .arg("rawvideo")
            .arg("-pix_fmt")
            .arg("bgra")
            .arg("-s")
            .arg(format!("{width}x{height}"))
            .arg("-r")
            .arg(rate.to_string())
            .arg("-i")
            .arg("pipe:0")
            .arg("-vaapi_device")
            .arg(&self.device)
            .arg("-vf")
            .arg("format=nv12,hwupload")
            .arg("-c:v")
            .arg(codec_name)
            .arg("-profile:v")
            .arg(profile)
            .arg("-rc_mode")
            .arg("CBR")
            .arg("-b:v")
            .arg(format!("{bitrate}k"))
            .arg("-g")
            .arg(gop.to_string())
            .arg("-bf")
            .arg("0")
            .arg("-bsf:v")
            .arg(bsf)
            .arg("-f")
            .arg(format)
            .arg("pipe:1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(lib) = &self.lib_path {
            command.env("LD_LIBRARY_PATH", lib);
        }

        let mut child = command.spawn().map_err(|e| {
            EncodeError::Backend(format!("spawn {}: {e}", self.ffmpeg.display()))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EncodeError::Backend("ffmpeg stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EncodeError::Backend("ffmpeg stdout".into()))?;

        let (tx, rx) = channel::<Bytes>();
        let codec = self.config.codec;
        let reader = std::thread::Builder::new()
            .name("aqua-vaapi-reader".into())
            .spawn(move || {
                let mut reader = std::io::BufReader::new(stdout);
                let mut buf = BytesMut::new();
                let mut chunk = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            buf.extend_from_slice(&chunk[..n]);
                            for au in drain_aus(codec, &mut buf) {
                                if tx.send(au).is_err() {
                                    return;
                                }
                            }
                        }
                    }
                }
                if !buf.is_empty() && tx.send(buf.freeze()).is_err() {
                    // receiver gone; nothing to do
                }
            })
            .map_err(|e| EncodeError::Backend(format!("reader thread: {e}")))?;

        self.child = Some(child);
        self.stdin = Some(stdin);
        self.aus = Some(rx);
        self.reader = Some(reader);
        Ok(())
    }

    fn ensure_started(&mut self) -> Result<(), EncodeError> {
        if self.child.is_none() {
            self.spawn()?;
        }
        Ok(())
    }

    fn write_frame(&mut self, frame: &GpuFrame) -> Result<(), EncodeError> {
        let expected = (frame.width * frame.height * 4) as usize;
        let data = frame
            .data
            .as_ref()
            .ok_or_else(|| EncodeError::Backend("frame has no CPU pixels".into()))?;
        if data.len() < expected {
            return Err(EncodeError::Backend(format!(
                "frame too small: {} < {expected}",
                data.len()
            )));
        }
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| EncodeError::Backend("no ffmpeg stdin".into()))?;
        stdin
            .write_all(&data[..expected])
            .and_then(|_| stdin.flush())
            .map_err(|e| EncodeError::Backend(format!("write frame: {e}")))
    }

    /// Build an [`EncodedFrame`] from one access unit, learning the codec
    /// parameter sets the first time we see them.
    fn build_encoded(&mut self, window_id: &str, au: Bytes) -> EncodedFrame {
        if self.codec_config.is_none() {
            let ps = parameter_sets(self.config.codec, &au);
            if !ps.is_empty() {
                self.codec_config = Some(ps);
            }
        }
        let keyframe = au_is_keyframe(self.config.codec, &au);
        let frame = EncodedFrame {
            window_id: window_id.to_string(),
            frame_id: self.frame_id,
            keyframe,
            codec_config: false,
            pts_us: self.pts_us,
            data: au,
        };
        self.frame_id += 1;
        let interval = 1_000_000 / u64::from(self.config.frame_rate.max(1));
        self.pts_us += interval;
        frame
    }
}

impl VideoEncoderSession for FfmpegVaapiSession {
    fn config(&self) -> &VideoEncoderConfig {
        &self.config
    }

    fn encode(&mut self, frame: &GpuFrame) -> Result<Option<EncodedFrame>, EncodeError> {
        self.window_id = frame.window_id.clone();
        if self.keyframe_requested {
            self.stop_child();
            self.codec_config = None;
            self.keyframe_requested = false;
        }
        self.ensure_started()?;
        self.write_frame(frame)?;

        let result = match self.aus.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(au)) => Some(self.build_encoded(&frame.window_id, au)),
            Some(Err(TryRecvError::Empty)) | None => None,
            Some(Err(TryRecvError::Disconnected)) => {
                return Err(EncodeError::Backend("encoder pipe closed".into()));
            }
        };
        Ok(result)
    }

    fn take_codec_config(&mut self) -> Option<Bytes> {
        self.codec_config.take()
    }

    fn poll(&mut self) -> Option<EncodedFrame> {
        let window_id = self.window_id.clone();
        let au = self.aus.as_ref()?.try_recv().ok()?;
        Some(self.build_encoded(&window_id, au))
    }

    fn request_keyframe(&mut self) {
        self.keyframe_requested = true;
    }

    fn reconfigure(&mut self, width: u32, height: u32) -> Result<(), EncodeError> {
        self.config.width = width;
        self.config.height = height;
        // A new size needs a fresh encoder process and a fresh IDR.
        self.stop_child();
        self.codec_config = None;
        Ok(())
    }
}

impl Drop for FfmpegVaapiSession {
    fn drop(&mut self) {
        self.stop_child();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start(code: &[u8]) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 1];
        v.extend_from_slice(code);
        v
    }

    /// One AU = AUD + VPS + SPS + PPS + IDR slice (HEVC).
    fn hevc_au(idr: bool) -> Vec<u8> {
        let mut au = Vec::new();
        au.extend(start(&[0x46, 0x01])); // AUD (type 35)
        au.extend(start(&[0x40, 0x01, 0xaa])); // VPS (32)
        au.extend(start(&[0x42, 0x01, 0xbb])); // SPS (33)
        au.extend(start(&[0x44, 0x01, 0xcc])); // PPS (34)
        au.extend(start(&[if idr { 0x26 } else { 0x02 }, 0x01, 0xdd])); // IDR(19/20) or TRAIL_R(1)
        au
    }

    #[test]
    fn detects_hevc_access_units() {
        let mut stream = BytesMut::new();
        stream.extend_from_slice(&hevc_au(true));
        stream.extend_from_slice(&hevc_au(false));
        let aus = drain_aus(VideoCodec::Hevc, &mut stream);
        assert_eq!(aus.len(), 1, "second AU still incomplete");
        assert!(au_is_keyframe(VideoCodec::Hevc, &aus[0]));
        // last AU stays buffered
        assert!(!stream.is_empty());
    }

    #[test]
    fn leading_parameter_sets_are_part_of_first_au() {
        // extradata before the first AUD
        let mut stream = BytesMut::new();
        stream.extend(start(&[0x40, 0x01, 0xaa]));
        stream.extend(start(&[0x42, 0x01, 0xbb]));
        stream.extend(start(&[0x44, 0x01, 0xcc]));
        stream.extend_from_slice(&hevc_au(true));
        stream.extend_from_slice(&hevc_au(false));
        let aus = drain_aus(VideoCodec::Hevc, &mut stream);
        assert_eq!(aus.len(), 1);
        let ps = parameter_sets(VideoCodec::Hevc, &aus[0]);
        assert!(ps.starts_with(&[0, 0, 0, 1, 0x40]), "VPS kept");
        assert!(au_is_keyframe(VideoCodec::Hevc, &aus[0]));
    }

    #[test]
    fn h264_aud_and_idr() {
        let mut au = Vec::new();
        au.extend(start(&[0x09, 0xf0])); // AUD (type 9)
        au.extend(start(&[0x67, 0x01])); // SPS (7)
        au.extend(start(&[0x68, 0x01])); // PPS (8)
        au.extend(start(&[0x65, 0x01])); // IDR slice (5)
        assert!(au_is_keyframe(VideoCodec::H264, &au));
        let ps = parameter_sets(VideoCodec::H264, &au);
        assert!(ps.windows(4).any(|w| w == [0, 0, 0, 1]));
    }
}
