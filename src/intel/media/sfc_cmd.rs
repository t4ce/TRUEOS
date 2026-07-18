
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
pub(crate) const SFC_MIN_SCALING_RATIO_NUMERATOR: u32 = 1;
pub(crate) const SFC_MIN_SCALING_RATIO_DENOMINATOR: u32 = 8;
pub(crate) const SFC_MAX_SCALING_RATIO: u32 = 8;
const SFC_SCALING_FACTOR_ONE: u64 = 1 << 19;
const SFC_MAX_GPU_ADDRESS: u64 = (1 << 48) - 1;

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

/// CSC-only, coded-size output does not emit polyphase coefficient tables.
pub(crate) const SFC_AVC_UI4_BASE_COMMAND_DWORDS: usize = SFC_LOCK_DWORD_COUNT
    + SFC_STATE_DWORD_COUNT
    + SFC_AVS_STATE_DWORD_COUNT
    + SFC_IEF_STATE_DWORD_COUNT
    + SFC_FRAME_START_DWORD_COUNT;
pub(crate) const SFC_AVC_UI4_SCALING_COMMAND_DWORDS: usize = SFC_AVC_UI4_BASE_COMMAND_DWORDS
    + SFC_AVS_LUMA_COEFF_DWORD_COUNT
    + SFC_AVS_CHROMA_COEFF_DWORD_COUNT;

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

/// Exact Intel command order when coded padding or requested resizing makes
/// the output extent differ from the VDBOX input extent.
pub(crate) const SFC_AVC_UI4_SCALING_RECIPE: &[SfcCommandStep] = &[
    SFC_AVC_UI4_SAME_SIZE_RECIPE[0],
    SFC_AVC_UI4_SAME_SIZE_RECIPE[1],
    SFC_AVC_UI4_SAME_SIZE_RECIPE[2],
    SfcCommandStep {
        kind: SfcCommandKind::AvsLumaCoefficients,
        dword_count: SFC_AVS_LUMA_COEFF_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcAvsLumaTable",
        trueos_gate: "the 5x5 polyphase luma table must match the planned X/Y ratios",
    },
    SfcCommandStep {
        kind: SfcCommandKind::AvsChromaCoefficients,
        dword_count: SFC_AVS_CHROMA_COEFF_DWORD_COUNT,
        upstream_file: "media_softlet/agnostic/common/vp/hal/packet/vp_render_sfc_base.cpp",
        upstream_symbol: "SfcRenderBase::SendSfcCmd/AddSfcAvsChromaTable",
        trueos_gate: "the NV12 left/center chroma siting table must match the CPU oracle",
    },
    SFC_AVC_UI4_SAME_SIZE_RECIPE[3],
    SFC_AVC_UI4_SAME_SIZE_RECIPE[4],
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcStateDwords {
    pub dwords: [u32; SFC_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcIefStateDwords {
    pub dwords: [u32; SFC_IEF_STATE_DWORD_COUNT],
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

/// Intel selects left/center chroma siting for scaled 4:2:0 input. Exact-size
/// VD output leaves both siting fields at zero and omits coefficient tables.
pub(crate) const fn encode_sfc_avs_state_avc(scaling_enabled: bool) -> SfcAvsStateDwords {
    let mut state = encode_sfc_avs_state_avc_same_size();
    if scaling_enabled {
        state.dwords[3] = 4;
    }
    state
}

/// IEF-disabled CSC state matching TRUEOS's integer limited-range BT.601
/// oracle: 298/256 luma, 409/256 red V, -100/256 green U,
/// -208/256 green V, and 516/256 blue U. The hardware coefficients are S2.10.
pub(crate) const fn encode_sfc_ief_state_trueos_bt601_limited() -> SfcIefStateDwords {
    let coefficients = [1192i16, 0, 1636, 1192, -400, -832, 1192, 2064, 0];
    let mut dwords = [
        SFC_IEF_STATE_DW0,
        0x0000_8040,
        0x0001_D100,
        0x039F_4F65,
        0x9A6E_4000,
        0x006C_9180,
        0xFFFE_2F2E,
        0x0000_0CE4,
        0xD82E_0640,
        0x8285_ECEC,
        0xFFFB_8282,
        0,
        0xFA11_7000,
        0xA38F_EC96,
        0x0100_8CC8,
        0x003A_6871,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    dwords[16] =
        1 | (signed_field(coefficients[0], 13) << 3) | (signed_field(coefficients[1], 13) << 16);
    dwords[17] = signed_field(coefficients[2], 13) | (signed_field(coefficients[3], 13) << 13);
    dwords[18] = signed_field(coefficients[4], 13) | (signed_field(coefficients[5], 13) << 13);
    dwords[19] = signed_field(coefficients[6], 13) | (signed_field(coefficients[7], 13) << 13);
    dwords[20] = signed_field(coefficients[8], 13);
    dwords[21] = signed_field(-16 * 4, 11);
    dwords[22] = signed_field(-128 * 4, 11);
    dwords[23] = signed_field(-128 * 4, 11);
    SfcIefStateDwords { dwords }
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
pub(crate) struct SfcGpuBuffer {
    pub gpu_addr: u64,
    pub byte_len: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SfcScratchBindings {
    pub avs_line: SfcGpuBuffer,
    pub avs_line_tile: SfcGpuBuffer,
    pub sfd_line: SfcGpuBuffer,
    pub sfd_line_tile: SfcGpuBuffer,
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
    UnsupportedScalingRatio,
    ScratchSizeOverflow,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum SfcPacketError {
    InvalidPlan,
    ResourceAddressInvalid,
    ResourceTooSmall,
    ResourceOverlap,
}

/// Validate the deliberately narrow first hardware milestone: progressive AVC
/// into a separate, linear, opaque RGBA UI4 buffer at the visible extent.
/// Coded padding is not a free crop in VD mode: a 1920x1088 coded frame going
/// to a 1920x1080 UI4 target is a scaling job and needs both AVS tables.
pub(crate) fn plan_avc_ui4_visible_output(
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
    let min_scale_numerator = u64::from(SFC_MIN_SCALING_RATIO_NUMERATOR);
    let min_scale_denominator = u64::from(SFC_MIN_SCALING_RATIO_DENOMINATOR);
    let max_scale = u64::from(SFC_MAX_SCALING_RATIO);
    if u64::from(output.width) * min_scale_denominator
        < u64::from(input.coded_width) * min_scale_numerator
        || u64::from(output.height) * min_scale_denominator
            < u64::from(input.coded_height) * min_scale_numerator
        || u64::from(output.width) > u64::from(input.coded_width) * max_scale
        || u64::from(output.height) > u64::from(input.coded_height) * max_scale
    {
        return Err(SfcPlanError::UnsupportedScalingRatio);
    }

    let scratch = sfc_vdbox_scratch_requirements(input.coded_width, output.width)
        .ok_or(SfcPlanError::ScratchSizeOverflow)?;
    let scaling_enabled = input.coded_width != output.width || input.coded_height != output.height;
    Ok(SfcAvcUi4Plan {
        input,
        output,
        scratch,
        command_dwords: if scaling_enabled {
            SFC_AVC_UI4_SCALING_COMMAND_DWORDS
        } else {
            SFC_AVC_UI4_BASE_COMMAND_DWORDS
        },
        scaling_enabled,
        csc_enabled: true,
        avs_coefficients_required: scaling_enabled,
        alpha_default_u10: 1023,
    })
}

/// Encode the complete 61-DWORD state for the checked single-pipe AVC plan.
/// This remains an offline builder: no caller appends it to a live batch yet.
pub(crate) fn encode_sfc_state_avc_ui4(
    plan: SfcAvcUi4Plan,
    scratch: SfcScratchBindings,
) -> Result<SfcStateDwords, SfcPacketError> {
    if plan_avc_ui4_visible_output(plan.input, plan.output).ok() != Some(plan) {
        return Err(SfcPacketError::InvalidPlan);
    }

    let resources = [
        SfcGpuBuffer {
            gpu_addr: plan.output.gpu_addr,
            byte_len: plan.output.byte_len,
        },
        scratch.avs_line,
        scratch.avs_line_tile,
        scratch.sfd_line,
        scratch.sfd_line_tile,
    ];
    let required = [
        plan.output.pitch_bytes as usize * plan.output.height as usize,
        plan.scratch.avs_line_bytes,
        plan.scratch.avs_line_tile_bytes,
        plan.scratch.sfd_line_bytes,
        plan.scratch.sfd_line_tile_bytes,
    ];
    let mut index = 0;
    while index < resources.len() {
        if !valid_gpu_buffer(resources[index]) {
            return Err(SfcPacketError::ResourceAddressInvalid);
        }
        if resources[index].byte_len < required[index] {
            return Err(SfcPacketError::ResourceTooSmall);
        }
        let mut other = 0;
        while other < index {
            if gpu_buffers_overlap(resources[index], resources[other]) {
                return Err(SfcPacketError::ResourceOverlap);
            }
            other += 1;
        }
        index += 1;
    }

    Ok(SfcStateDwords {
        dwords: encode_sfc_state_core(
            plan.input.coded_width,
            plan.input.coded_height,
            plan.output.width,
            plan.output.height,
            plan.output.pitch_bytes,
            plan.output.gpu_addr,
            scratch.avs_line.gpu_addr,
            scratch.sfd_line.gpu_addr,
            scratch.avs_line_tile.gpu_addr,
            scratch.sfd_line_tile.gpu_addr,
            plan.scaling_enabled,
            plan.alpha_default_u10,
        ),
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

#[allow(clippy::too_many_arguments)]
const fn encode_sfc_state_core(
    input_width: u32,
    input_height: u32,
    output_width: u32,
    output_height: u32,
    output_pitch: u32,
    output_addr: u64,
    avs_line_addr: u64,
    sfd_line_addr: u64,
    avs_line_tile_addr: u64,
    sfd_line_tile_addr: u64,
    scaling_enabled: bool,
    alpha_default_u10: u16,
) -> [u32; SFC_STATE_DWORD_COUNT] {
    let mut dwords = [0u32; SFC_STATE_DWORD_COUNT];
    dwords[0] = SFC_STATE_DW0;
    // VD-to-SFC, NV12 4:2:0, post-deblock 16x16 shifted ordering, one pipe,
    // progressive input and output.
    dwords[1] = (1 << 4) | (1 << 8);
    dwords[2] = pack_extent(input_width, input_height);
    // A8B8G8R8 is MSB A:B:G:R, hence little-endian memory bytes R,G,B,A.
    // UI4 therefore needs no channel swap.
    dwords[3] = 1;
    // IEF sharpening stays disabled. Adaptive filtering is bypassed for a
    // non-upscale, CSC is enabled, and coded-padding removal enables AVS plus
    // 4:2:0 chroma upsampling.
    dwords[4] = (1 << 8)
        | (1 << 9)
        | (1 << 19)
        | if scaling_enabled {
            (1 << 7) | (1 << 12)
        } else {
            0
        };
    dwords[5] = pack_extent(input_width, input_height);
    dwords[7] = pack_extent(output_width, output_height);
    dwords[8] = pack_extent(output_width, output_height);
    dwords[13] = alpha_default_u10 as u32;
    dwords[14] = scaling_factor(input_height, output_height) << 5;
    dwords[15] = scaling_factor(input_width, output_width) << 5;
    write_gpu_address(&mut dwords, 17, output_addr);
    write_gpu_address(&mut dwords, 20, avs_line_addr);
    write_gpu_address(&mut dwords, 26, sfd_line_addr);
    dwords[29] = (1 << 28) | ((output_pitch - 1) << 3);
    write_gpu_address(&mut dwords, 38, avs_line_tile_addr);
    write_gpu_address(&mut dwords, 44, sfd_line_tile_addr);
    dwords
}

const fn pack_extent(width: u32, height: u32) -> u32 {
    (width - 1) | ((height - 1) << 16)
}

const fn scaling_factor(input: u32, output: u32) -> u32 {
    ((input as u64 * SFC_SCALING_FACTOR_ONE) / output as u64) as u32
}

const fn write_gpu_address(
    dwords: &mut [u32; SFC_STATE_DWORD_COUNT],
    low_index: usize,
    address: u64,
) {
    dwords[low_index] = address as u32;
    dwords[low_index + 1] = (address >> 32) as u32;
}

const fn signed_field(value: i16, bits: u32) -> u32 {
    (value as i32 as u32) & ((1u32 << bits) - 1)
}

fn valid_gpu_buffer(buffer: SfcGpuBuffer) -> bool {
    buffer.gpu_addr != 0
        && buffer.gpu_addr.is_multiple_of(SFC_RESOURCE_ALIGNMENT)
        && buffer.byte_len != 0
        && buffer
            .byte_len
            .is_multiple_of(SFC_RESOURCE_ALIGNMENT as usize)
        && buffer
            .gpu_addr
            .checked_add(buffer.byte_len as u64 - 1)
            .is_some_and(|end| end <= SFC_MAX_GPU_ADDRESS)
}

fn gpu_buffers_overlap(left: SfcGpuBuffer, right: SfcGpuBuffer) -> bool {
    let left_end = left.gpu_addr + left.byte_len as u64;
    let right_end = right.gpu_addr + right.byte_len as u64;
    left.gpu_addr < right_end && right.gpu_addr < left_end
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
    let scaled_avs = encode_sfc_avs_state_avc(true);
    let ief = encode_sfc_ief_state_trueos_bt601_limited();
    let start = encode_sfc_frame_start();
    let state = encode_sfc_state_core(
        1920,
        1088,
        1920,
        1080,
        7680,
        0x1E00_0000,
        0x3000_0000,
        0x3001_0000,
        0x3002_0000,
        0x3004_0000,
        true,
        1023,
    );
    lock.dwords[0] == SFC_LOCK_DW0
        && lock.dwords[1] == 2
        && avs.dwords[0] == SFC_AVS_STATE_DW0
        && avs.dwords[1] == 0xFF00_0045
        && avs.dwords[2] == 0x0007_0014
        && avs.dwords[3] == 0
        && scaled_avs.dwords[3] == 4
        && ief.dwords[0] == SFC_IEF_STATE_DW0
        && ief.dwords[16] == 0x0000_2541
        && ief.dwords[17] == 0x0095_0664
        && ief.dwords[18] == 0x0398_1E70
        && ief.dwords[19] == 0x0102_04A8
        && ief.dwords[21] == 0x0000_07C0
        && ief.dwords[22] == 0x0000_0600
        && state[0] == SFC_STATE_DW0
        && state[1] == 0x0000_0110
        && state[2] == 0x043F_077F
        && state[3] == 1
        && state[4] == 0x0008_1380
        && state[13] == 1023
        && state[15] == 0x0100_0000
        && state[17] == 0x1E00_0000
        && state[29] == 0x1000_EFF8
        && start.dwords[0] == SFC_FRAME_START_DW0
        && start.dwords[1] == 0
        && SFC_AVC_UI4_BASE_COMMAND_DWORDS == 93
        && SFC_AVC_UI4_SCALING_COMMAND_DWORDS == 287
        && SFC_AVC_UI4_SAME_SIZE_RECIPE.len() == 5
        && SFC_AVC_UI4_SCALING_RECIPE.len() == 7
}

const _: () = assert!(validate_sfc_foundation());
