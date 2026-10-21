// Bounded, private Font RCS admission for the hardware-tessellation probe.
//
// This module intentionally stops before submission while the checked-in
// native artifact is unavailable. That keeps ordinary font rendering on the
// legacy coverage path and prevents an offline capture from being mistaken
// for on-device stencil/retirement evidence.

const FONT_CURVE_PATCH_BEGIN_CONTOUR: u32 = 1 << 0;
const FONT_CURVE_PATCH_END_CONTOUR: u32 = 1 << 1;
const FONT_CURVE_PATCH_REVERSED: u32 = 1 << 2;
const FONT_CURVE_PATCH_KNOWN_FLAGS: u32 =
    FONT_CURVE_PATCH_BEGIN_CONTOUR | FONT_CURVE_PATCH_END_CONTOUR | FONT_CURVE_PATCH_REVERSED;

const FONT_TESSELLATION_MAX_NATIVE_EXTENT: u32 = 2048;
const FONT_TESSELLATION_SAMPLE_AXIS: u32 = 4;
const FONT_TESSELLATION_MAX_PATCHES: usize = 4096;
const FONT_TESSELLATION_MAX_CONTOURS: u32 = 127;
const FONT_TESSELLATION_MAX_FACTOR: u32 = 64;
const FONT_TESSELLATION_MAX_TRANSIENT_BYTES: usize = 32 * 1024 * 1024;
const FONT_TESSELLATION_FIXED_TRANSIENT_BYTES: usize = 256 * 1024;
const FONT_TESSELLATION_TOLERANCE_PX: f32 = 0.125;
#[repr(C, align(16))]
#[derive(Copy, Clone, Debug, PartialEq)]
pub(crate) struct FontCurvePatchV1 {
    pub(crate) p0: [f32; 2],
    pub(crate) p1: [f32; 2],
    pub(crate) p2: [f32; 2],
    pub(crate) p3: [f32; 2],
    pub(crate) anchor: [f32; 2],
    pub(crate) contour_index: u32,
    pub(crate) flags: u32,
    pub(crate) reserved: [u32; 4],
}

const _: () = {
    assert!(core::mem::size_of::<FontCurvePatchV1>() == 64);
    assert!(core::mem::align_of::<FontCurvePatchV1>() == 16);
    assert!(core::mem::offset_of!(FontCurvePatchV1, p0) == 0);
    assert!(core::mem::offset_of!(FontCurvePatchV1, p1) == 8);
    assert!(core::mem::offset_of!(FontCurvePatchV1, p2) == 16);
    assert!(core::mem::offset_of!(FontCurvePatchV1, p3) == 24);
    assert!(core::mem::offset_of!(FontCurvePatchV1, anchor) == 32);
    assert!(core::mem::offset_of!(FontCurvePatchV1, contour_index) == 40);
    assert!(core::mem::offset_of!(FontCurvePatchV1, flags) == 44);
    assert!(core::mem::offset_of!(FontCurvePatchV1, reserved) == 48);
};











#[cfg(test)]
mod font_tessellation_tests {
    use super::*;

    fn patch() -> FontCurvePatchV1 {
        FontCurvePatchV1 {
            p0: [8.0, 32.0],
            p1: [8.0, 8.0],
            p2: [40.0, 8.0],
            p3: [40.0, 32.0],
            anchor: [24.0, 44.0],
            contour_index: 0,
            flags: FONT_CURVE_PATCH_BEGIN_CONTOUR | FONT_CURVE_PATCH_END_CONTOUR,
            reserved: [0; 4],
        }
    }

    #[test]
    fn source_only_probe_stops_before_submission() {
        assert_eq!(
            preflight_font_tessellation_probe(0xa780, 64, 64, &[patch()]),
            Err(FontTessellationProbePreflightError::ArtifactUnavailable)
        );
    }

    #[test]
    fn invalid_input_fails_before_artifact_gate() {
        let mut invalid = patch();
        invalid.reserved[0] = 1;
        assert_eq!(
            preflight_font_tessellation_probe(0xa780, 64, 64, &[invalid]),
            Err(FontTessellationProbePreflightError::InvalidPatch)
        );
        assert_eq!(
            preflight_font_tessellation_probe(0xa780, 0, 64, &[patch()]),
            Err(FontTessellationProbePreflightError::InvalidExtent)
        );
        let mut open = patch();
        open.flags &= !FONT_CURVE_PATCH_END_CONTOUR;
        assert_eq!(
            preflight_font_tessellation_probe(0xa780, 64, 64, &[open]),
            Err(FontTessellationProbePreflightError::InvalidContourOrder)
        );
    }
}
