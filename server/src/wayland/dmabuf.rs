//! `zwp_linux_dmabuf_v1` adapter (phase 3C).
//!
//! Smithay owns the protocol implementation; this module only:
//!
//! 1. translates the neutral `GpuBufferImporter` format list into Smithay
//!    `drm_fourcc` formats (**only** what Aqua can really import), and
//! 2. converts an imported Smithay `Dmabuf` into a neutral
//!    [`crate::gpu::GpuFrame`] (duplicating plane fds, no CPU readback) before
//!    handing it to the importer.
//!
//! The global is created in [`crate::wayland::AquaState::install_gpu`] and only
//! when the importer advertises formats. See `docs/GPU_PIPELINE.md`.

use smithay::{
    backend::allocator::{dmabuf::Dmabuf, Buffer, Format, Fourcc, Modifier},
    delegate_dmabuf,
    wayland::dmabuf::{DmabufGlobal, DmabufHandler, DmabufState, ImportNotifier},
};

use crate::gpu::{DmabufFormat, FrameSource, GpuBufferImporter, GpuFrame, PlaneFd, SyncState};

use super::state::AquaState;

/// Translate the importer's advertised `(fourcc, modifier)` pairs into Smithay
/// formats. Pairs whose fourcc the kernel does not recognize are dropped rather
/// than announced.
pub fn smithay_formats(importer: &dyn GpuBufferImporter) -> Vec<Format> {
    importer
        .supported_formats()
        .iter()
        .filter_map(|format| {
            let code = Fourcc::try_from(format.fourcc).ok()?;
            Some(Format {
                code,
                modifier: Modifier::from(format.modifier),
            })
        })
        .collect()
}

/// Convert a Smithay `Dmabuf` into a neutral [`GpuFrame`], duplicating plane fds.
///
/// `window_id`/`surface_id` are empty at import time (imports happen when the
/// client creates the `wl_buffer`, before it is attached to a surface); the
/// attach path fills them in.
pub fn gpu_frame_from_dmabuf(dmabuf: &Dmabuf) -> std::io::Result<GpuFrame> {
    let format = dmabuf.format();
    let modifier = u64::from(format.modifier);
    let source_format = DmabufFormat::new(format.code as u32, modifier);
    let offsets: Vec<u32> = dmabuf.offsets().collect();
    let strides: Vec<u32> = dmabuf.strides().collect();

    let mut planes = Vec::with_capacity(dmabuf.num_planes());
    for (index, handle) in dmabuf.handles().enumerate() {
        let fd = handle.try_clone_to_owned()?;
        planes.push(PlaneFd::new(
            fd,
            offsets.get(index).copied().unwrap_or(0),
            strides.get(index).copied().unwrap_or(0),
            modifier,
        ));
    }

    Ok(GpuFrame {
        window_id: String::new(),
        surface_id: String::new(),
        width: dmabuf.width(),
        height: dmabuf.height(),
        format: source_format,
        source: FrameSource::Dmabuf,
        // The concrete fence is backend-specific; the contract only requires
        // that the importer synchronize before consuming (see docs/VIDEO.md).
        sync: SyncState::Unknown,
        planes,
        data: None,
    })
}

impl DmabufHandler for AquaState {
    fn dmabuf_state(&mut self) -> &mut DmabufState {
        &mut self.dmabuf_state
    }

    fn dmabuf_imported(
        &mut self,
        _global: &DmabufGlobal,
        dmabuf: Dmabuf,
        notifier: ImportNotifier,
    ) {
        let frame = match gpu_frame_from_dmabuf(&dmabuf) {
            Ok(frame) => frame,
            Err(error) => {
                tracing::warn!(%error, "dmabuf: failed to duplicate plane fds");
                notifier.failed();
                return;
            }
        };

        tracing::debug!(
            target: "aqua::dmabuf",
            importer = self.gpu_importer.name(),
            width = frame.width,
            height = frame.height,
            fourcc = format_args!("{:#010x}", frame.format.fourcc),
            modifier = format_args!("{:#018x}", frame.format.modifier),
            planes = frame.plane_count(),
            "dmabuf.imported"
        );

        match self.gpu_importer.import(frame) {
            Ok(_imported) => {
                // Milestone 1: the buffer is accepted for GPU consumption. The
                // imported frame is handed to the per-window video encoder when
                // one is installed (see PHASE3C.md, Milestone 2/3).
                let _ = notifier.successful::<AquaState>();
            }
            Err(error) => {
                tracing::debug!(%error, "dmabuf: importer rejected buffer");
                notifier.failed();
            }
        }
    }
}

delegate_dmabuf!(AquaState);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::ImportError;

    struct FakeImporter {
        formats: Vec<DmabufFormat>,
    }

    impl GpuBufferImporter for FakeImporter {
        fn name(&self) -> &str {
            "fake"
        }
        fn supported_formats(&self) -> &[DmabufFormat] {
            &self.formats
        }
        fn import(&self, _frame: GpuFrame) -> Result<GpuFrame, ImportError> {
            Err(ImportError::Backend("fake".into()))
        }
    }

    #[test]
    fn known_fourcc_is_kept_and_unknown_is_dropped() {
        // XR24 = Xrgb8888 (0x34325258), linear modifier 0.
        let importer = FakeImporter {
            formats: vec![
                DmabufFormat::new(0x3432_5258, 0),
                // Not a valid fourcc: must be silently dropped, never announced.
                DmabufFormat::new(0xdead_beef, 0),
            ],
        };
        let formats = smithay_formats(&importer);
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].code, Fourcc::Xrgb8888);
        assert_eq!(u64::from(formats[0].modifier), 0);
    }

    #[test]
    fn no_formats_announces_nothing() {
        let importer = FakeImporter { formats: vec![] };
        assert!(smithay_formats(&importer).is_empty());
    }

    #[test]
    fn amd_tiling_modifier_is_preserved() {
        // AR24 with the real AMD GFX9+DCC modifier observed from weston-simple-egl.
        let modifier = 0x0200_0004_4051_ba01;
        let importer = FakeImporter {
            formats: vec![DmabufFormat::new(0x3432_5241, modifier)],
        };
        let formats = smithay_formats(&importer);
        assert_eq!(formats.len(), 1);
        assert_eq!(formats[0].code, Fourcc::Argb8888);
        assert_eq!(u64::from(formats[0].modifier), modifier);
    }
}
