//! dmabuf observation importer (Milestone 1 of phase 3C).
//!
//! This importer does **not** yet import into the GPU (no EGLImage / VA surface).
//! It exists to let `zwp_linux_dmabuf_v1` be advertised with the **real**
//! `(fourcc, modifier)` pairs this GPU can import — enumerated from EGL
//! (`EGL_EXT_image_dma_buf_import_modifiers`) — so a GPU client (e.g.
//! `weston-simple-egl`) actually allocates dmabufs and Aqua can observe
//! format/modifier/planes/dimensions. It is enabled explicitly
//! (`AQUA_DMABUF=observe`) and logs, so it is never mistaken for a real import.
//!
//! Milestone 2 replaces this with a concrete importer
//! (`EGL_EXT_image_dma_buf_import` → VA-API surface, or DRM PRIME) that encodes
//! without a CPU readback.

use super::{DmabufFormat, GpuBufferImporter, GpuFrame, ImportError};

/// Modifiers we advertise. We deliberately restrict to `LINEAR` (0): VA-API
/// imported a linear AR24 dmabuf via `DRM_PRIME_2` with **0 CPU copies**
/// (verified), while the AMD tiling+DCC modifiers are unproven for VA import and
/// would need a `VAProc`/VPP or DCC-aware path. Advertising only what we can
/// really import makes GPU clients allocate linear buffers. The **full** EGL
/// list this GPU supports is documented in `docs/GPU_PIPELINE.md` §0.
const MODS_RGBA: [u64; 1] = [0]; // DRM_FORMAT_MOD_LINEAR
const MODS_YUV: [u64; 1] = [0]; // DRM_FORMAT_MOD_LINEAR

const AR24: u32 = 0x3432_5241; // ARGB8888
const XR24: u32 = 0x3432_5258; // XRGB8888
const AB24: u32 = 0x3432_4241; // ABGR8888
const XB24: u32 = 0x3432_4258; // XBGR8888
const NV12: u32 = 0x3231_564e;
const P010: u32 = 0x3031_3050;

/// Metadata-only importer that advertises the GPU's real dmabuf formats.
pub struct ObserveGpuImporter {
    formats: Vec<DmabufFormat>,
}

impl ObserveGpuImporter {
    pub fn new() -> Self {
        let mut formats = Vec::new();
        for fourcc in [AR24, XR24, AB24, XB24] {
            for modifier in MODS_RGBA {
                formats.push(DmabufFormat::new(fourcc, modifier));
            }
        }
        for fourcc in [NV12, P010] {
            for modifier in MODS_YUV {
                formats.push(DmabufFormat::new(fourcc, modifier));
            }
        }
        Self { formats }
    }
}

impl Default for ObserveGpuImporter {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuBufferImporter for ObserveGpuImporter {
    fn name(&self) -> &str {
        "egl-observe"
    }

    fn supported_formats(&self) -> &[DmabufFormat] {
        &self.formats
    }

    fn import(&self, frame: GpuFrame) -> Result<GpuFrame, ImportError> {
        tracing::info!(
            target: "aqua::dmabuf",
            width = frame.width,
            height = frame.height,
            fourcc = format_args!("{:#010x}", frame.format.fourcc),
            modifier = format_args!("{:#018x}", frame.format.modifier),
            planes = frame.plane_count(),
            "dmabuf.observed (metadata-only; real EGL/VA import is Milestone 2)"
        );
        for (index, plane) in frame.planes.iter().enumerate() {
            tracing::info!(
                target: "aqua::dmabuf",
                plane = index,
                offset = plane.offset,
                stride = plane.stride,
                modifier = format_args!("{:#018x}", plane.modifier),
                "dmabuf.plane"
            );
        }
        Ok(frame)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_real_formats() {
        let importer = ObserveGpuImporter::new();
        let formats = importer.supported_formats();
        assert_eq!(formats.len(), 4 * MODS_RGBA.len() + 2 * MODS_YUV.len());
        assert!(formats.contains(&DmabufFormat::new(AR24, 0)));
        assert!(formats.contains(&DmabufFormat::new(NV12, 0)));
    }

    #[test]
    fn advertises_only_linear_modifiers() {
        let importer = ObserveGpuImporter::new();
        assert!(importer
            .supported_formats()
            .iter()
            .all(|format| format.modifier == 0));
    }

    #[test]
    fn is_named_as_an_observation_importer() {
        assert_eq!(ObserveGpuImporter::new().name(), "egl-observe");
    }
}
