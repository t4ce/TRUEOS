#![allow(dead_code)]

//! Mechanical VD-to-SFC bring-up contract for Xe-LP media.
//!
//! Nothing in this module submits SFC work.  It establishes the checked plan,
//! command order, fixed packet headers, and scratch sizing that the eventual
//! AVC batch builder must consume.  Keeping this separate from the live AVC
//! encoder makes it impossible for a mere `sfc: true` topology declaration to
//! enable partially programmed hardware.

pub(crate) const UPSTREAM_INTEL_MEDIA_DRIVER_REPO: &str = "https://github.com/intel/media-driver";
pub(crate) const UPSTREAM_INTEL_MEDIA_DRIVER_COMMIT: &str = "a203cfc";
pub(crate) const UPSTREAM_SFC_PLATFORM: &str = "Xe_LPM_plus_base";

pub(crate) const SFC_MIN_OUTPUT_DIMENSION: u32 = 32;
pub(crate) const SFC_MAX_FRAME_DIMENSION: u32 = 16 * 1024;
pub(crate) const SFC_OUTPUT_PITCH_ALIGNMENT: u32 = 64;
pub(crate) const SFC_RESOURCE_ALIGNMENT: u64 = 4096;

pub(crate) const SFC_LOCK_DWORD_COUNT: usize = 2;
pub(crate) const SFC_STATE_DWORD_COUNT: usize = 61;
pub(crate) const SFC_AVS_STATE_DWORD_COUNT: usize = 4;
pub(crate) const SFC_IEF_STATE_DWORD_COUNT: usize = 24;
pub(crate) const SFC_FRAME_START_DWORD_COUNT: usize = 2;
pub(crate) const SFC_AVS_LUMA_COEFF_DWORD_COUNT: usize = 129;
pub(crate) const SFC_AVS_CHROMA_COEFF_DWORD_COUNT: usize = 65;

pub(crate) const SFC_LOCK_DW0: u32 = 0x7500_0000;
pub(crate) const SFC_STATE_DW0: u32 = 0x7501_003B;
pub(crate) const SFC_AVS_STATE_DW0: u32 = 0x7502_0002;
pub(crate) const SFC_IEF_STATE_DW0: u32 = 0x7503_0016;
pub(crate) const SFC_FRAME_START_DW0: u32 = 0x7504_0000;

/// CSC-only, one-to-one output does not emit polyphase coefficient tables.
pub(crate) const SFC_AVC_UI4_SAME_SIZE_COMMAND_DWORDS: usize = SFC_LOCK_DWORD_COUNT
    + SFC_STATE_DWORD_COUNT
    + SFC_AVS_STATE_DWORD_COUNT
    + SFC_IEF_STATE_DWORD_COUNT
    + SFC_FRAME_START_DWORD_COUNT;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SfcCommandKind {
    Lock,
    State,
    AvsState,
    AvsLumaCoefficients,
    AvsChromaCoefficients,
    IefState,
    FrameStart,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcCommandStep {
    pub kind: SfcCommandKind,
    pub dword_count: usize,
    pub upstream_file: &'static str,
    pub upstream_symbol: &'static str,
    pub trueos_gate: &'static str,
}

/// Exact command order used by Intel's VD-to-SFC path for same-size CSC.
/// This sequence is inserted after the second MFX_WAIT and before
/// MFX_SURFACE_STATE in the AVC picture packet.
pub(crate) const SFC_AVC_UI4_SAME_SIZE_RECIPE: &[SfcCommandStep] = &[
    SfcCommandStep {
        kind: SfcCommandKind::Lock,
        dword_count: SFC_LOCK_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcLock",
        trueos_gate: "single VDBOX decoder pipe, output-to-memory enabled",
    },
    SfcCommandStep {
        kind: SfcCommandKind::State,
        dword_count: SFC_STATE_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/hw/mhw_sfc_xe_lpm_plus_base_next_impl.h",
        upstream_symbol: "mhw::sfc::xe_lpm_plus_base_next::Impl::SetSFC_STATE",
        trueos_gate: "linear RGBA output and every required scratch address must be bound",
    },
    SfcCommandStep {
        kind: SfcCommandKind::AvsState,
        dword_count: SFC_AVS_STATE_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcAvsState",
        trueos_gate: "one-to-one progressive sampling state must be encoded",
    },
    SfcCommandStep {
        kind: SfcCommandKind::IefState,
        dword_count: SFC_IEF_STATE_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcIefState",
        trueos_gate: "BT.601 video-range NV12 to opaque UI4 RGBA CSC must match the CPU oracle",
    },
    SfcCommandStep {
        kind: SfcCommandKind::FrameStart,
        dword_count: SFC_FRAME_START_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcFrameStart",
        trueos_gate: "all state and output mappings must be complete before frame start",
    },
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcLockDwords {
    pub dwords: [u32; SFC_LOCK_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcFrameStartDwords {
    pub dwords: [u32; SFC_FRAME_START_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcAvsStateDwords {
    pub dwords: [u32; SFC_AVS_STATE_DWORD_COUNT],
}

/// Acquire SFC for an MFX decoder and enable its pre-scaled output to memory.
pub(crate) const fn encode_sfc_lock_vdbox_output_to_memory() -> SfcLockDwords {
    SfcLockDwords {
        dwords: [SFC_LOCK_DW0, 1 << 1],
    }
}

pub(crate) const fn encode_sfc_frame_start() -> SfcFrameStartDwords {
    SfcFrameStartDwords {
        dwords: [SFC_FRAME_START_DW0, 0],
    }
}

/// Intel's 5x5, one-to-one AVC AVS state. Chroma siting remains left/top and
/// coefficient-table packets are intentionally absent until scaling is added.
pub(crate) const fn encode_sfc_avs_state_avc_same_size() -> SfcAvsStateDwords {
    SfcAvsStateDwords {
        dwords: [
            SFC_AVS_STATE_DW0,
            (255 << 24) | (4 << 4) | 5,
            (7 << 16) | 20,
            0,
        ],
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcAvcInputFrame {
    pub coded_width: u32,
    pub coded_height: u32,
    pub visible_width: u32,
    pub visible_height: u32,
    pub progressive: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcRgbaOutputSurface {
    pub gpu_addr: u64,
    pub phys_addr: u64,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcScratchRequirements {
    pub avs_line_bytes: usize,
    pub avs_line_tile_bytes: usize,
    pub ief_line_bytes: usize,
    pub ief_line_tile_bytes: usize,
    pub sfd_line_bytes: usize,
    pub sfd_line_tile_bytes: usize,
    pub page_aligned_total_bytes: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcAvcUi4Plan {
    pub input: SfcAvcInputFrame,
    pub output: SfcRgbaOutputSurface,
    pub scratch: SfcScratchRequirements,
    pub command_dwords: usize,
    pub scaling_enabled: bool,
    pub csc_enabled: bool,
    pub avs_coefficients_required: bool,
    pub alpha_default_u10: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SfcPlanError {
    InvalidInputDimensions,
    UnsupportedPictureStructure,
    OutputDimensionsMismatch,
    OutputAddressUnaligned,
    OutputMappingInvalid,
    OutputPitchInvalid,
    OutputSurfaceTooSmall,
    ScratchSizeOverflow,
}

/// Validate the deliberately narrow first hardware milestone: progressive AVC
/// into a separate, linear, opaque RGBA UI4 buffer without scaling.
pub(crate) fn plan_avc_ui4_same_size(
    input: SfcAvcInputFrame,
    output: SfcRgbaOutputSurface,
) -> Result<SfcAvcUi4Plan, SfcPlanError> {
    if input.coded_width == 0
        || input.coded_height == 0
        || input.coded_width > SFC_MAX_FRAME_DIMENSION
        || input.coded_height > SFC_MAX_FRAME_DIMENSION
        || input.visible_width == 0
        || input.visible_height == 0
        || input.visible_width > input.coded_width
        || input.visible_height > input.coded_height
    {
        return Err(SfcPlanError::InvalidInputDimensions);
    }
    if !input.progressive {
        return Err(SfcPlanError::UnsupportedPictureStructure);
    }
    if output.width != input.visible_width
        || output.height != input.visible_height
        || output.width < SFC_MIN_OUTPUT_DIMENSION
        || output.height < SFC_MIN_OUTPUT_DIMENSION
        || output.width > SFC_MAX_FRAME_DIMENSION
        || output.height > SFC_MAX_FRAME_DIMENSION
    {
        return Err(SfcPlanError::OutputDimensionsMismatch);
    }
    if output.gpu_addr == 0
        || output.phys_addr == 0
        || !output.gpu_addr.is_multiple_of(SFC_RESOURCE_ALIGNMENT)
        || !output.phys_addr.is_multiple_of(SFC_RESOURCE_ALIGNMENT)
    {
        return Err(SfcPlanError::OutputAddressUnaligned);
    }
    if output.byte_len == 0
        || !output
            .byte_len
            .is_multiple_of(SFC_RESOURCE_ALIGNMENT as usize)
    {
        return Err(SfcPlanError::OutputMappingInvalid);
    }
    let min_pitch = output
        .width
        .checked_mul(4)
        .ok_or(SfcPlanError::OutputPitchInvalid)?;
    if output.pitch_bytes < min_pitch
        || !output
            .pitch_bytes
            .is_multiple_of(SFC_OUTPUT_PITCH_ALIGNMENT)
        || output.pitch_bytes.saturating_sub(1) > 0x7_FFFF
    {
        return Err(SfcPlanError::OutputPitchInvalid);
    }
    let required_output_bytes = (output.pitch_bytes as usize)
        .checked_mul(output.height as usize)
        .ok_or(SfcPlanError::OutputSurfaceTooSmall)?;
    if output.byte_len < required_output_bytes {
        return Err(SfcPlanError::OutputSurfaceTooSmall);
    }

    let scratch = sfc_vdbox_scratch_requirements(input.coded_width, output.width)
        .ok_or(SfcPlanError::ScratchSizeOverflow)?;
    Ok(SfcAvcUi4Plan {
        input,
        output,
        scratch,
        command_dwords: SFC_AVC_UI4_SAME_SIZE_COMMAND_DWORDS,
        scaling_enabled: false,
        csc_enabled: true,
        avs_coefficients_required: false,
        alpha_default_u10: 1023,
    })
}

/// Intel's single-pipe VD-to-SFC allocation formulas for 4-tap 8-bit AVC.
/// IEF line storage is zero in VDBOX mode; CSC still consumes IEF_STATE.
fn sfc_vdbox_scratch_requirements(
    input_width: u32,
    output_width: u32,
) -> Option<SfcScratchRequirements> {
    const CACHELINE: usize = 64;
    const TILE_EXTRA: usize = 1024 * CACHELINE;
    const AVS_4_TAP_8BIT_BYTES_PER_PIXEL: usize = 3 * CACHELINE / 8;

    let aligned_input_width = align_up(input_width as usize, 8)?;
    let avs_line_bytes = aligned_input_width.checked_mul(AVS_4_TAP_8BIT_BYTES_PER_PIXEL)?;
    let avs_line_tile_bytes = avs_line_bytes.checked_add(TILE_EXTRA)?;
    let sfd_line_bytes = (output_width as usize)
        .div_ceil(10)
        .checked_mul(CACHELINE)?
        .checked_mul(2)?;
    let sfd_line_tile_bytes = sfd_line_bytes.checked_add(TILE_EXTRA)?;
    let page_aligned_total_bytes = [
        avs_line_bytes,
        avs_line_tile_bytes,
        sfd_line_bytes,
        sfd_line_tile_bytes,
    ]
    .into_iter()
    .try_fold(0usize, |total, bytes| {
        align_up(bytes, SFC_RESOURCE_ALIGNMENT as usize).and_then(|bytes| total.checked_add(bytes))
    })?;

    Some(SfcScratchRequirements {
        avs_line_bytes,
        avs_line_tile_bytes,
        ief_line_bytes: 0,
        ief_line_tile_bytes: 0,
        sfd_line_bytes,
        sfd_line_tile_bytes,
        page_aligned_total_bytes,
    })
}

fn align_up(value: usize, alignment: usize) -> Option<usize> {
    if alignment == 0 || !alignment.is_power_of_two() {
        return None;
    }
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
}

pub(crate) const fn validate_sfc_foundation() -> bool {
    let lock = encode_sfc_lock_vdbox_output_to_memory();
    let avs = encode_sfc_avs_state_avc_same_size();
    let start = encode_sfc_frame_start();
    lock.dwords[0] == SFC_LOCK_DW0
        && lock.dwords[1] == 2
        && avs.dwords[0] == SFC_AVS_STATE_DW0
        && avs.dwords[1] == 0xFF00_0045
        && avs.dwords[2] == 0x0007_0014
        && avs.dwords[3] == 0
        && start.dwords[0] == SFC_FRAME_START_DW0
        && start.dwords[1] == 0
        && SFC_AVC_UI4_SAME_SIZE_COMMAND_DWORDS == 93
        && SFC_AVC_UI4_SAME_SIZE_RECIPE.len() == 5
}

const _: () = assert!(validate_sfc_foundation());
