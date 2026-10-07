//! Headless implementations of the GPU/video traits.
//!
//! These are what the default build installs when there is no GPU (see
//! `docs/GPU_PIPELINE.md`). They advertise nothing, so Aqua never claims dmabuf
//! or video support it cannot deliver.

use super::{
    EncodeError, GpuBufferImporter, GpuFrame, ImportError, VideoCodec, VideoEncoder,
    VideoEncoderConfig, VideoEncoderSession,
};

/// An importer for a machine with no GPU: imports nothing, supports no formats.
pub struct NullGpuImporter;

impl GpuBufferImporter for NullGpuImporter {
    fn name(&self) -> &str {
        "null"
    }

    fn supported_formats(&self) -> &[super::DmabufFormat] {
        &[]
    }

    fn import(&self, _frame: GpuFrame) -> Result<GpuFrame, ImportError> {
        Err(ImportError::NoImporter)
    }
}

/// An encoder for a machine with no hardware encoder: supports no codecs.
pub struct NullVideoEncoder;

impl VideoEncoder for NullVideoEncoder {
    fn name(&self) -> &str {
        "null"
    }

    fn supported_codecs(&self) -> &[VideoCodec] {
        &[]
    }

    fn create_session(
        &self,
        _config: VideoEncoderConfig,
    ) -> Result<Box<dyn VideoEncoderSession>, EncodeError> {
        Err(EncodeError::NoEncoder)
    }
}
