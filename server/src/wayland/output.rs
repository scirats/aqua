use smithay::{
    output::{Mode, Output, PhysicalProperties, Scale, Subpixel},
    reexports::wayland_server::DisplayHandle,
    utils::Transform,
};

use super::state::AquaState;

/// Default virtual output size. Not final; see PHASE2.md.
pub const VIRTUAL_WIDTH: i32 = 1920;
pub const VIRTUAL_HEIGHT: i32 = 1080;
pub const VIRTUAL_REFRESH_MHZ: i32 = 60_000;

/// Create the logical output Aqua advertises to clients.
///
/// This is a pure protocol output: no DRM/KMS, no physical monitor, no dummy
/// HDMI. It exists so Wayland clients get a valid `wl_output`/`xdg_output` and a
/// scaling reference even on a machine without a display.
pub fn create_virtual_output(display_handle: &DisplayHandle) -> Output {
    let output = Output::new(
        "aqua-virtual-output".to_string(),
        PhysicalProperties {
            // ~344x194 mm, a plausible 15" 16:9 panel.
            size: (344, 194).into(),
            subpixel: Subpixel::Unknown,
            make: "Aqua".to_string(),
            model: "Virtual Output".to_string(),
        },
    );

    let mode = Mode {
        size: (VIRTUAL_WIDTH, VIRTUAL_HEIGHT).into(),
        refresh: VIRTUAL_REFRESH_MHZ,
    };
    output.change_current_state(
        Some(mode),
        Some(Transform::Normal),
        Some(Scale::Integer(1)),
        Some((0, 0).into()),
    );
    output.set_preferred(mode);
    output.create_global::<AquaState>(display_handle);

    output
}
