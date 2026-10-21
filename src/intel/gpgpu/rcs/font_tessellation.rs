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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum FontTessellationProbePreflightError {
    ArtifactUnavailable,
    ArtifactDeviceMismatch,
    EmptyPatchSet,
    PatchLimit,
    InvalidExtent,
    InvalidPatch,
    InvalidContourOrder,
    WindingLimit,
    TessellationFactorLimit,
    WorkOverflow,
    TransientLimit,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct FontTessellationProbePlan {
    pub(crate) patch_count: u32,
    pub(crate) contour_count: u32,
    pub(crate) maximum_factor: u32,
    pub(crate) supersample_width: u32,
    pub(crate) supersample_height: u32,
    pub(crate) transient_bytes: usize,
}

fn font_patch_point_is_finite(point: [f32; 2]) -> bool {
    point[0].is_finite() && point[1].is_finite()
}

fn font_patch_factor(patch: &FontCurvePatchV1) -> Option<u32> {
    let second_difference = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        let x = a[0] - 2.0 * b[0] + c[0];
        let y = a[1] - 2.0 * b[1] + c[1];
        libm::sqrtf(x * x + y * y)
    };
    let curvature = libm::fmaxf(
        second_difference(patch.p0, patch.p1, patch.p2),
        second_difference(patch.p1, patch.p2, patch.p3),
    );
    if !curvature.is_finite() {
        return None;
    }
    let factor = libm::ceilf(libm::sqrtf(0.75 * curvature / FONT_TESSELLATION_TOLERANCE_PX));
    if !factor.is_finite() || factor < 0.0 || factor > u32::MAX as f32 {
        None
    } else {
        Some(libm::fmaxf(factor, 1.0) as u32)
    }
}

/// Validate a hand-authored P1 probe before any GPU resource becomes owned.
///
/// This function returns `ArtifactUnavailable` for the source-only artifact.
/// A later matched capture may pass admission, but the caller must still own
/// the FontKernel GPU lease and complete stencil, cover, and retirement before
/// publishing any result.
pub(crate) fn preflight_font_tessellation_probe(
    device_id: u16,
    native_width: u32,
    native_height: u32,
    patches: &[FontCurvePatchV1],
) -> Result<FontTessellationProbePlan, FontTessellationProbePreflightError> {
    if patches.is_empty() {
        return Err(FontTessellationProbePreflightError::EmptyPatchSet);
    }
    if patches.len() > FONT_TESSELLATION_MAX_PATCHES {
        return Err(FontTessellationProbePreflightError::PatchLimit);
    }
    // Until P2 contributes a stronger topology proof, one wedge is the
    // conservative maximum contribution at a sample. Keep signed winding in
    // [-127, 127] so an eight-bit wrapping stencil cannot alias nonzero to zero.
    if patches.len() > FONT_TESSELLATION_MAX_CONTOURS as usize {
        return Err(FontTessellationProbePreflightError::WindingLimit);
    }
    if native_width == 0
        || native_height == 0
        || native_width > FONT_TESSELLATION_MAX_NATIVE_EXTENT
        || native_height > FONT_TESSELLATION_MAX_NATIVE_EXTENT
    {
        return Err(FontTessellationProbePreflightError::InvalidExtent);
    }

    let mut previous_contour = None;
    let mut previous_ended = true;
    let mut contour_count = 0u32;
    let mut maximum_factor = 1u32;
    for patch in patches {
        if !font_patch_point_is_finite(patch.p0)
            || !font_patch_point_is_finite(patch.p1)
            || !font_patch_point_is_finite(patch.p2)
            || !font_patch_point_is_finite(patch.p3)
            || !font_patch_point_is_finite(patch.anchor)
            || patch.flags & !FONT_CURVE_PATCH_KNOWN_FLAGS != 0
            || patch.reserved != [0; 4]
        {
            return Err(FontTessellationProbePreflightError::InvalidPatch);
        }
        let begins = patch.flags & FONT_CURVE_PATCH_BEGIN_CONTOUR != 0;
        let ends = patch.flags & FONT_CURVE_PATCH_END_CONTOUR != 0;
        match previous_contour {
            Some(previous) if patch.contour_index < previous => {
                return Err(FontTessellationProbePreflightError::InvalidContourOrder);
            }
            Some(previous) if patch.contour_index == previous => {
                if previous_ended || begins {
                    return Err(FontTessellationProbePreflightError::InvalidContourOrder);
                }
            }
            _ => {
                if !previous_ended
                    || !begins
                    || patch.contour_index != previous_contour.map_or(0, |index| index + 1)
                {
                    return Err(FontTessellationProbePreflightError::InvalidContourOrder);
                }
                contour_count = contour_count
                    .checked_add(1)
                    .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
                previous_contour = Some(patch.contour_index);
            }
        }
        previous_ended = ends;
        let factor =
            font_patch_factor(patch).ok_or(FontTessellationProbePreflightError::InvalidPatch)?;
        if factor > FONT_TESSELLATION_MAX_FACTOR {
            return Err(FontTessellationProbePreflightError::TessellationFactorLimit);
        }
        maximum_factor = maximum_factor.max(factor);
    }
    if !previous_ended {
        return Err(FontTessellationProbePreflightError::InvalidContourOrder);
    }
    if contour_count > FONT_TESSELLATION_MAX_CONTOURS {
        return Err(FontTessellationProbePreflightError::WindingLimit);
    }

    let supersample_width = native_width
        .checked_mul(FONT_TESSELLATION_SAMPLE_AXIS)
        .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
    let supersample_height = native_height
        .checked_mul(FONT_TESSELLATION_SAMPLE_AXIS)
        .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
    let target_bytes = (supersample_width as usize)
        .checked_mul(supersample_height as usize)
        .and_then(|pixels| pixels.checked_mul(5))
        .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
    let patch_bytes = patches
        .len()
        .checked_mul(core::mem::size_of::<FontCurvePatchV1>())
        .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
    let transient_bytes = target_bytes
        .checked_add(patch_bytes)
        .and_then(|bytes| bytes.checked_add(FONT_TESSELLATION_FIXED_TRANSIENT_BYTES))
        .ok_or(FontTessellationProbePreflightError::WorkOverflow)?;
    if transient_bytes > FONT_TESSELLATION_MAX_TRANSIENT_BYTES {
        return Err(FontTessellationProbePreflightError::TransientLimit);
    }
    if !crate::intel::shader::font_patch::AVAILABLE {
        return Err(FontTessellationProbePreflightError::ArtifactUnavailable);
    }
    if crate::intel::shader::font_patch::COMPILED_DEVICE_ID != device_id {
        return Err(FontTessellationProbePreflightError::ArtifactDeviceMismatch);
    }
    Ok(FontTessellationProbePlan {
        patch_count: patches.len() as u32,
        contour_count,
        maximum_factor,
        supersample_width,
        supersample_height,
        transient_bytes,
    })
}

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
