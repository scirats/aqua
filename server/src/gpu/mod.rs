//! Neutral GPU / video abstraction boundary (phase 3C).
//!
//! This module is the **only** place allowed to know about GPU import and video
//! encoders. It deliberately exposes plain OS/neutral types (`OwnedFd`, fourcc
//! codes, modifiers, byte buffers) so that none of it leaks into
//! [`crate::domain`] (`RemoteWindow` / `RemoteSurface`) or the Aqua protocol.
//!
//! The default build has **no GPU** (see `docs/GPU_PIPELINE.md`), so it installs
//! [`NullGpuImporter`] and [`NullVideoEncoder`]: no dmabuf formats are
//! advertised and no video capability is negotiated. A real implementation on
//! the target RTX 2060 box drops in behind the same traits.
//!
//! Design rules (from the phase):
//! - a `GpuFrame` is produced by importing a dmabuf; importing must not perform
//!   a CPU readback on the normal path;
//! - the encoder consumes `GpuFrame`s and yields `EncodedFrame`s;
//! - inter-frame codecs require keyframe awareness, so the session exposes
//!   `request_keyframe` rather than "drop any frame".

use std::os::fd::OwnedFd;

use bytes::Bytes;

/// A dmabuf `(fourcc, modifier)` pair that an importer can consume.
///
/// `fourcc` is the DRM fourcc code (e.g. `AR24`/`XR24`/`NV12`) and `modifier`
/// the DRM format modifier (`0` == `DRM_FORMAT_MOD_LINEAR`, `0x00ffffffffffffff`
/// == `DRM_FORMAT_MOD_INVALID`). Kept as raw integers to avoid re-exporting a
/// third-party type across the boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DmabufFormat {
    pub fourcc: u32,
    pub modifier: u64,
}

impl DmabufFormat {
    pub const fn new(fourcc: u32, modifier: u64) -> Self {
        Self { fourcc, modifier }
    }
}

/// One plane of a dmabuf: a duplicated fd plus its layout.
#[derive(Debug)]
pub struct PlaneFd {
    /// Duplicated file descriptor. Owning it keeps the buffer alive.
    pub fd: OwnedFd,
    pub offset: u32,
    pub stride: u32,
    pub modifier: u64,
}

impl PlaneFd {
    pub fn new(fd: OwnedFd, offset: u32, stride: u32, modifier: u64) -> Self {
        Self {
            fd,
            offset,
            stride,
            modifier,
        }
    }
}

/// Where the pixels of a frame came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameSource {
    /// GPU-native buffer (`linux-dmabuf`).
    Dmabuf,
    /// CPU `wl_shm` buffer (fallback / debugging).
    Shm,
}

/// The synchronization state attached to a buffer.
///
/// A dmabuf being valid on the wire does **not** mean it is ready to read; the
/// producer may still be writing. The concrete fence type (implicit `dma_fence`
/// vs explicit `linux-drm-syncobj`) is backend-specific and intentionally not
/// modelled until a real importer needs it. The point of this type is that the
/// contract requires *some* proof of readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncState {
    /// No fence information; the importer/encoder must synchronize itself.
    #[default]
    Unknown,
    /// The buffer is complete and may be consumed immediately.
    Ready,
}

/// A GPU-imported frame, expressed in neutral terms.
///
/// This is metadata plus owned plane fds. It carries no EGL/CUDA/Vulkan/Smithay
/// type; a real importer maps it internally.
#[derive(Debug)]
pub struct GpuFrame {
    pub window_id: String,
    pub surface_id: String,
    pub width: u32,
    pub height: u32,
    pub format: DmabufFormat,
    pub source: FrameSource,
    pub sync: SyncState,
    pub planes: Vec<PlaneFd>,
    /// CPU pixels for a [`FrameSource::Shm`] frame: tightly packed ARGB
    /// (`width * 4` bytes per row, `width * height * 4` total). `None` for dmabuf
    /// frames, whose pixels live in `planes`.
    pub data: Option<Bytes>,
}

impl GpuFrame {
    pub fn plane_count(&self) -> usize {
        self.planes.len()
    }

    /// Duplicate the plane fds, producing an independently owned frame.
    pub fn try_clone(&self) -> std::io::Result<GpuFrame> {
        let planes = self
            .planes
            .iter()
            .map(|plane| {
                Ok(PlaneFd::new(
                    plane.fd.try_clone()?,
                    plane.offset,
                    plane.stride,
                    plane.modifier,
                ))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(GpuFrame {
            window_id: self.window_id.clone(),
            surface_id: self.surface_id.clone(),
            width: self.width,
            height: self.height,
            format: self.format,
            source: self.source,
            sync: self.sync,
            planes,
            data: self.data.clone(),
        })
    }
}

/// Why an import failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// No importer is configured (headless / no GPU).
    NoImporter,
    /// The `(format, modifier)` pair is not supported.
    UnsupportedFormat,
    /// A backend-specific failure (driver, out of memory, ...).
    Backend(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoImporter => f.write_str("no GPU importer configured"),
            Self::UnsupportedFormat => f.write_str("unsupported dmabuf format/modifier"),
            Self::Backend(message) => write!(f, "import backend error: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Imports dmabufs into the GPU so the encoder can consume them without a CPU
/// readback.
pub trait GpuBufferImporter: Send + Sync {
    /// Short backend name for logs (e.g. `"egl-cuda"`).
    fn name(&self) -> &str;

    /// The `(fourcc, modifier)` pairs this importer can actually consume. An
    /// empty list means "no dmabuf support": the `zwp_linux_dmabuf_v1` global is
    /// not created and nothing is advertised.
    fn supported_formats(&self) -> &[DmabufFormat];

    /// Import a frame. On success the returned frame is ready to hand to a
    /// [`VideoEncoderSession`]. Must not perform a CPU readback.
    fn import(&self, frame: GpuFrame) -> Result<GpuFrame, ImportError>;
}

/// Video codecs Aqua can negotiate (mirrors `aqua.v1.VideoCodec`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
}

impl VideoCodec {
    /// Wire value for `aqua.v1.VideoCodec`.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::H264 => 1,
            Self::Hevc => 2,
            Self::Av1 => 3,
        }
    }
}

/// Decoder-facing chroma layout (mirrors `aqua.v1.VideoChroma`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoChroma {
    Nv12,
    P010,
}

impl VideoChroma {
    pub fn to_wire(self) -> u32 {
        match self {
            Self::Nv12 => 1,
            Self::P010 => 2,
        }
    }
}

/// Encoder session configuration. Mirrors `aqua.v1.WindowVideoConfig`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoEncoderConfig {
    pub codec: VideoCodec,
    pub chroma: VideoChroma,
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    pub bitrate_kbps: u32,
    pub gop: u32,
    pub low_latency: bool,
}

/// One encoded access unit.
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    pub window_id: String,
    pub frame_id: u64,
    pub keyframe: bool,
    /// `true` when the payload is codec configuration only (VPS/SPS/PPS), not a
    /// picture.
    pub codec_config: bool,
    pub pts_us: u64,
    pub data: Bytes,
}

/// Why encoding failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// No hardware/software encoder is available.
    NoEncoder,
    /// The codec is not supported by this encoder.
    UnsupportedCodec(VideoCodec),
    /// A backend-specific failure.
    Backend(String),
}

impl std::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEncoder => f.write_str("no video encoder configured"),
            Self::UnsupportedCodec(codec) => write!(f, "unsupported codec: {codec:?}"),
            Self::Backend(message) => write!(f, "encoder backend error: {message}"),
        }
    }
}

impl std::error::Error for EncodeError {}

/// A live encoder for one `RemoteWindow` video stream.
///
/// The session is deliberately stateful because inter-frame codecs require:
/// - keyframe awareness (`request_keyframe`),
/// - reconfiguration on resize (`reconfigure`).
pub trait VideoEncoderSession: Send {
    fn config(&self) -> &VideoEncoderConfig;

    /// Encode one frame. Returns `Ok(None)` when the hardware encoder is still
    /// buffering and has not produced an access unit yet (encode delay): the
    /// caller simply calls again with the next frame. This keeps the core thread
    /// non-blocking.
    fn encode(&mut self, frame: &GpuFrame) -> Result<Option<EncodedFrame>, EncodeError>;

    /// Codec parameter sets (VPS/SPS/PPS in Annex-B) produced by the encoder,
    /// to be sent as a `CONFIG` message before the next frame. Taken once.
    fn take_codec_config(&mut self) -> Option<Bytes> {
        None
    }

    /// Ask for a keyframe on the next encode (decoder reset, dropped GOP, resize,
    /// reconnection).
    fn request_keyframe(&mut self);

    /// Reconfigure for a new size without destroying the session.
    fn reconfigure(&mut self, width: u32, height: u32) -> Result<(), EncodeError>;
}

/// Factory for encoder sessions.
pub trait VideoEncoder: Send + Sync {
    fn name(&self) -> &str;

    /// Codecs this encoder can actually produce by hardware. The negotiated
    /// capability uses this list; an empty list means no video.
    fn supported_codecs(&self) -> &[VideoCodec];

    /// Whether this encoder can consume `FrameSource::Dmabuf` frames (import to a
    /// VA surface / VPP) without a CPU readback. Defaults to `false`: an encoder
    /// that only handles the SHM path must not be handed dmabuf frames.
    fn supports_dmabuf(&self) -> bool {
        false
    }

    fn create_session(
        &self,
        config: VideoEncoderConfig,
    ) -> Result<Box<dyn VideoEncoderSession>, EncodeError>;
}

mod null;
mod observe;
mod sidecar;
mod vaapi;

pub use null::{NullGpuImporter, NullVideoEncoder};
pub use observe::ObserveGpuImporter;
pub use sidecar::VaSidecarEncoder;
pub use vaapi::{FfmpegVaapiEncoder, MIN_HEIGHT, MIN_WIDTH};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_importer_advertises_no_formats_and_rejects_import() {
        let importer = NullGpuImporter;
        assert!(importer.supported_formats().is_empty());
        let frame = GpuFrame {
            window_id: "window-1".into(),
            surface_id: "surface-1".into(),
            width: 4,
            height: 2,
            format: DmabufFormat::new(0x34325241, 0),
            source: FrameSource::Dmabuf,
            sync: SyncState::Ready,
            planes: Vec::new(),
            data: None,
        };
        assert!(matches!(
            importer.import(frame),
            Err(ImportError::NoImporter)
        ));
    }

    #[test]
    fn null_encoder_has_no_codecs_and_no_sessions() {
        let encoder = NullVideoEncoder;
        assert!(encoder.supported_codecs().is_empty());
        let config = VideoEncoderConfig {
            codec: VideoCodec::Hevc,
            chroma: VideoChroma::Nv12,
            width: 800,
            height: 600,
            frame_rate: 60,
            bitrate_kbps: 8000,
            gop: 0,
            low_latency: true,
        };
        assert_eq!(
            encoder.create_session(config).err(),
            Some(EncodeError::NoEncoder)
        );
    }

    #[test]
    fn codec_and_chroma_wire_values_match_proto() {
        assert_eq!(VideoCodec::H264.to_wire(), 1);
        assert_eq!(VideoCodec::Hevc.to_wire(), 2);
        assert_eq!(VideoCodec::Av1.to_wire(), 3);
        assert_eq!(VideoChroma::Nv12.to_wire(), 1);
        assert_eq!(VideoChroma::P010.to_wire(), 2);
    }
}
