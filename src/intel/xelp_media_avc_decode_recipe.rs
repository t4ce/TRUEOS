#![allow(dead_code)]

use alloc::vec::Vec;

pub(crate) const UPSTREAM_INTEL_MEDIA_DRIVER_REPO: &str = "https://github.com/intel/media-driver";
pub(crate) const UPSTREAM_INTEL_MEDIA_DRIVER_COMMIT: &str = "a203cfc";
pub(crate) const UPSTREAM_AVC_PLATFORM: &str = "Xe_LPM_plus_base";

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcDecodePhase {
    Prolog,
    Picture,
    Slice,
    Epilog,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcDecodeCommandStep {
    pub phase: AvcDecodePhase,
    pub command: &'static str,
    pub upstream_file: &'static str,
    pub upstream_symbol: &'static str,
    pub trueos_gate: &'static str,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcCommandBlock {
    pub offset: usize,
    pub dword_count: usize,
    pub command: &'static str,
    pub upstream_file: &'static str,
    pub upstream_symbol: &'static str,
}

pub(crate) const AVC_DECODE_COMMAND_RECIPE: &[AvcDecodeCommandStep] = &[
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Prolog,
        command: "MI_FORCE_WAKEUP",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_packet.cpp",
        upstream_symbol: "AvcDecodePkt::AddForceWakeup",
        trueos_gate: "existing media forcewake path must prove VDBOX awake",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_WAIT",
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_picture_packet_xe_lpm_plus_base.cpp",
        upstream_symbol: "AvcDecodePicPktXe_Lpm_Plus_Base::Execute",
        trueos_gate: "emit before and after MFX_PIPE_MODE_SELECT",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_PIPE_MODE_SELECT",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_PIPE_MODE_SELECT, AvcDecodePicPkt)",
        trueos_gate: "long format first; short format only after parser parity",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_SURFACE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_SURFACE_STATE, AvcDecodePicPkt)",
        trueos_gate: "NV12 surface pitch, tile mode, and UV offsets must be modeled",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_PIPE_BUF_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_PIPE_BUF_ADDR_STATE, AvcDecodePicPkt)",
        trueos_gate: "dest, refs, intra rowstore, deblock rowstore",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_IND_OBJ_BASE_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_IND_OBJ_BASE_ADDR_STATE, AvcDecodePicPkt)",
        trueos_gate: "bitstream data buffer size and offset must be parser-derived",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_BSP_BUF_BASE_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_BSP_BUF_BASE_ADDR_STATE, AvcDecodePicPkt)",
        trueos_gate: "bsd/mpc and mpr rowstore buffers must exist or cache-enable must be proven",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFD_AVC_DPB_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFD_AVC_DPB_STATE, AvcDecodePicPkt)",
        trueos_gate: "required for short format and reference-picture correctness",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFD_AVC_PICID_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFD_AVC_PICID_STATE, AvcDecodePicPkt)",
        trueos_gate: "pic id remapping policy must be explicit",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_AVC_IMG_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_AVC_IMG_STATE, AvcDecodePicPkt)",
        trueos_gate: "SPS/PPS parser must fill the full image parameter set",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_QM_STATE x4",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "AvcDecodePicPkt::AddAllCmds_MFX_QM_STATE",
        trueos_gate: "intra/inter 4x4 and 8x8 matrices must come from PPS/SPS defaults",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Picture,
        command: "MFX_AVC_DIRECTMODE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_AVC_DIRECTMODE_STATE, AvcDecodePicPkt)",
        trueos_gate: "current/reference DMV buffers must be allocated and mapped",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Slice,
        command: "MFX_AVC_REF_IDX_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::AddCmd_AVC_SLICE_REF_IDX",
        trueos_gate: "needed for P/B slices; skip for simple I-only milestone",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Slice,
        command: "MFX_AVC_WEIGHTOFFSET_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::AddCmd_AVC_SLICE_WEIGHT_OFFSET",
        trueos_gate: "needed only when weighted prediction flags request it",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Slice,
        command: "MFX_AVC_SLICE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::SET_AVC_SLICE_STATE",
        trueos_gate: "slice parser must provide first/next MB, QP, offsets, and bit offsets",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Slice,
        command: "MFD_AVC_BSD_OBJECT",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::AddCmd_AVC_BSD_OBJECT",
        trueos_gate: "indirect BSD length/start must be exact for each slice",
    },
    AvcDecodeCommandStep {
        phase: AvcDecodePhase::Epilog,
        command: "MI_FLUSH_DW",
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_packet_xe_lpm_plus_base.cpp",
        upstream_symbol: "AvcDecodePktXe_Lpm_Plus_Base::EnsureAllCommandsExecuted",
        trueos_gate: "final flush before status/marker completion",
    },
];

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcDecodePortMilestone {
    LongFormatSingleIdr,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcSliceClass {
    I,
    P,
    B,
    Si,
    Sp,
    Unknown,
}

impl AvcSliceClass {
    pub(crate) const fn is_i_only(self) -> bool {
        matches!(self, Self::I | Self::Si)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcPictureStructure {
    Frame,
    TopField,
    BottomField,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcChromaFormat {
    Monochrome,
    Yuv420,
    Yuv422,
    Yuv444,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcSurfaceFormat {
    Nv12,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcSurfaceLayout {
    pub format: AvcSurfaceFormat,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: usize,
    pub y_offset: usize,
    pub uv_offset: usize,
    pub byte_len: usize,
}

impl AvcSurfaceLayout {
    pub(crate) const fn nv12_tile64(
        width: u32,
        height: u32,
        pitch_bytes: usize,
        uv_offset: usize,
    ) -> Self {
        let total_rows = align_ceil_usize(
            (uv_offset / pitch_bytes) + (height as usize).div_ceil(2),
            MEDIA_TILE64_H,
        );
        Self {
            format: AvcSurfaceFormat::Nv12,
            width,
            height,
            pitch_bytes,
            y_offset: 0,
            uv_offset,
            byte_len: total_rows * pitch_bytes,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcPictureParams {
    pub pic_width_in_mbs_minus1: u16,
    pub pic_height_in_mbs_minus1: u16,
    pub picture_structure: AvcPictureStructure,
    pub chroma_format: AvcChromaFormat,
    pub entropy_coding_mode: bool,
    pub weighted_pred: bool,
    pub weighted_bipred_idc: u8,
    pub transform_8x8: bool,
    pub constrained_intra_pred: bool,
    pub direct_8x8_inference: bool,
    pub frame_mbs_only: bool,
    pub mb_adaptive_frame_field: bool,
    pub field_pic: bool,
    pub reference_pic: bool,
    pub num_ref_idx_l0_active_minus1: u8,
    pub num_ref_idx_l1_active_minus1: u8,
    pub pic_init_qp_minus26: i8,
    pub chroma_qp_index_offset: i8,
    pub second_chroma_qp_index_offset: i8,
    pub frame_num: u16,
    pub log2_max_frame_num_minus4: u8,
    pub log2_max_pic_order_cnt_lsb_minus4: u8,
    pub num_slice_groups_minus1: u8,
    pub redundant_pic_cnt_present: bool,
    pub pic_order_present: bool,
    pub slice_group_map_type: u8,
    pub pic_order_cnt_type: u8,
    pub top_field_order_cnt: i32,
    pub bottom_field_order_cnt: i32,
    pub deblocking_filter_control_present: bool,
    pub delta_pic_order_always_zero: bool,
    pub slice_group_change_rate_minus1: u16,
    pub visible_width: u32,
    pub visible_height: u32,
}

impl AvcPictureParams {
    pub(crate) const fn pic_width_in_mbs(self) -> usize {
        self.pic_width_in_mbs_minus1 as usize + 1
    }

    pub(crate) const fn pic_height_in_mbs(self) -> usize {
        self.pic_height_in_mbs_minus1 as usize + 1
    }

    pub(crate) const fn macroblock_count(self) -> usize {
        self.pic_width_in_mbs() * self.pic_height_in_mbs()
    }

    pub(crate) const fn coded_width(self) -> u32 {
        self.pic_width_in_mbs() as u32 * 16
    }

    pub(crate) const fn coded_height(self) -> u32 {
        self.pic_height_in_mbs() as u32 * 16
    }

    pub(crate) const fn visible_width(self) -> u32 {
        self.visible_width
    }

    pub(crate) const fn visible_height(self) -> u32 {
        self.visible_height
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcSliceParams {
    pub class: AvcSliceClass,
    pub first_mb_in_slice: u32,
    pub first_mb_in_next_slice: u32,
    pub slice_data_offset: u32,
    pub slice_data_size: u32,
    pub slice_data_bit_offset_from_payload: u32,
    pub first_mb_byte_offset: u32,
    pub slice_data_bit_offset: u8,
    pub disable_deblocking_filter_idc: u8,
    pub slice_alpha_c0_offset_div2: i8,
    pub slice_beta_offset_div2: i8,
    pub slice_qp_delta: i8,
    pub direct_spatial_mv_pred: bool,
    pub num_ref_idx_l0_active_minus1: u8,
    pub num_ref_idx_l1_active_minus1: u8,
    top_field_order_cnt: i32,
    bottom_field_order_cnt: i32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcLongFormatSliceRecord {
    pub offset: u32,
    pub length: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcDecodeResourcePlan {
    pub dest_surface: AvcSurfaceLayout,
    pub rowstore: AvcRowstoreScratchBytes,
    pub dmv_write_buffer_bytes: usize,
    pub dmv_reference_buffer_bytes: usize,
    pub reference_surface_count: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcLongFormatIdrPlan {
    pub picture: AvcPictureParams,
    pub slice: AvcSliceParams,
    pub resources: AvcDecodeResourcePlan,
    pub bitstream_bytes: usize,
    pub bitstream_data_offset: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcLongFormatIdrPacketParams {
    pub pipe_mode_select: MfxPipeModeSelectParams,
    pub surface_state: MfxSurfaceStateParams,
    pub pipe_buf_addr_state: MfxPipeBufAddrStateParams,
    pub ind_obj_base_addr_state: MfxIndObjBaseAddrStateParams,
    pub bsp_buf_base_addr_state: MfxBspBufBaseAddrStateParams,
    pub avc_picid_state: MfdAvcPicidStateParams,
    pub avc_img_state: MfxAvcImgStateParams,
    pub avc_qm_states: [MfxQmStateParams; AVC_QM_STATE_COUNT],
    pub avc_directmode_state: MfxAvcDirectmodeStateParams,
    pub avc_ref_idx_state: MfxAvcRefIdxStateParams,
    pub avc_slice_state: MfxAvcSliceStateParams,
    pub avc_bsd_object: MfdAvcBsdObjectParams,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcLongFormatIdrCommandStream {
    pub dwords: Vec<u32>,
    pub command_count: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcCommandStreamBlocker {
    Milestone(AvcMilestoneBlocker),
    ResourceTooSmall(&'static str),
    ResourceUnaligned(&'static str),
    CommandShapeMismatch,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcAnnexBPlanError {
    MissingSps,
    MissingPps,
    MissingIdrSlice,
    UnsupportedNal,
    InvalidBitstream,
    UnsupportedSps,
    UnsupportedPps,
    UnsupportedSlice,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxPipeModeSelectParams {
    pub mode: u32,
    pub standard_select: u32,
    pub codec_select: u8,
    pub pre_deblocking_output_enable: bool,
    pub post_deblocking_output_enable: bool,
    pub stream_out_enable: bool,
    pub deblocker_stream_out_enable: bool,
    pub decoder_mode_select: u8,
    pub decoder_short_format_mode: u8,
    pub short_format_in_use: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxSurfaceStateParams {
    pub surface_id: u8,
    pub width_minus1: u32,
    pub height_minus1: u32,
    pub tilemode: u32,
    pub surface_pitch_minus1: u32,
    pub compression_format: u32,
    pub interleave_chroma: u8,
    pub surface_format: u32,
    pub y_offset_for_u_cb: u32,
    pub y_offset_for_v_cr: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxPipeBufAddrStateParams {
    pub mode: u32,
    pub decode_in_use: bool,
    pub post_deblock_surface_is_dest: bool,
    pub pre_deblock_surface_is_dest: bool,
    pub intra_rowstore_bytes: usize,
    pub deblocking_filter_rowstore_bytes: usize,
    pub reference_surface_count: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxIndObjBaseAddrStateParams {
    pub mode: u32,
    pub data_size: u32,
    pub data_offset: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxBspBufBaseAddrStateParams {
    pub bsd_mpc_rowstore_bytes: usize,
    pub mpr_rowstore_bytes: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcDpbStateParams {
    pub non_existing_frame_flags: u16,
    pub long_term_frame_flags: u16,
    pub used_for_reference_flags: u32,
    pub ref_frame_order: [u16; 16],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcPicidStateParams {
    pub picture_id_remapping_disable: bool,
    pub picture_id_list: [u16; 16],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxQmStateParams {
    pub qm_type: u8,
    pub matrix: [u32; 16],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcDirectmodeStateParams {
    pub dmv_write_addr: u64,
    pub dmv_reference_addrs: [u64; 16],
    pub poc_list: [u32; 34],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcRefIdxStateParams {
    pub ref_pic_list_select: u8,
    pub reference_list_entry: [u32; 8],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcImgStateParams {
    pub frame_size_mbs: u32,
    pub frame_width_in_mbs_minus1: u16,
    pub frame_height_in_mbs_minus1: u16,
    pub image_structure: u8,
    pub weighted_bipred_idc: u8,
    pub weighted_pred: bool,
    pub first_chroma_qp_offset: i8,
    pub second_chroma_qp_offset: i8,
    pub field_pic: bool,
    pub mb_adaptive_frame_field: bool,
    pub frame_mbs_only: bool,
    pub transform_8x8: bool,
    pub direct_8x8_inference: bool,
    pub constrained_intra_pred: bool,
    pub disposable: bool,
    pub entropy_coding: bool,
    pub chroma_format_idc: u8,
    pub initial_qp_value: i8,
    pub active_ref_l0: u8,
    pub active_ref_l1: u8,
    pub reference_frames: u8,
    pub pic_order_present: bool,
    pub delta_pic_order_always_zero: bool,
    pub pic_order_cnt_type: u8,
    pub slice_group_map_type: u8,
    pub redundant_pic_cnt_present: bool,
    pub num_slice_groups_minus1: u8,
    pub deblocking_filter_control_present: bool,
    pub log2_max_frame_num_minus4: u8,
    pub log2_max_pic_order_cnt_lsb_minus4: u8,
    pub slice_group_change_rate: u16,
    pub curr_pic_frame_num: u16,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcSliceStateParams {
    pub slice_type: u8,
    pub log2_weight_denom_luma: u8,
    pub log2_weight_denom_chroma: u8,
    pub number_of_ref_pictures_l0: u8,
    pub number_of_ref_pictures_l1: u8,
    pub slice_alpha_c0_offset_div2: i8,
    pub slice_beta_offset_div2: i8,
    pub slice_quantization_parameter: u8,
    pub cabac_init_idc: u8,
    pub disable_deblocking_filter_indicator: u8,
    pub direct_prediction_type: u8,
    pub weighted_prediction_indicator: u8,
    pub slice_start_mb_num: u32,
    pub slice_horizontal_position: u32,
    pub slice_vertical_position: u32,
    pub next_slice_horizontal_position: u32,
    pub next_slice_vertical_position: u32,
    pub slice_id: u8,
    pub cabac_zero_word_insertion_enable: bool,
    pub emulation_byte_slice_insert_enable: bool,
    pub tail_insertion_present_in_bitstream: bool,
    pub slice_data_insertion_present_in_bitstream: bool,
    pub header_insertion_present_in_bitstream: bool,
    pub is_last_slice: bool,
    pub round_intra: u8,
    pub round_inter: u8,
    pub round_inter_enable: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcBsdObjectParams {
    pub indirect_bsd_data_length: u32,
    pub indirect_bsd_data_start_address: u32,
    pub first_mb_byte_offset_of_slice_data_or_slice_header: u32,
    pub first_macroblock_mb_bit_offset: u8,
    pub last_slice: bool,
    pub fix_prev_mb_skipped: bool,
    pub intra_predmode_4x4_8x8_luma_error_control: bool,
    pub intra_prediction_error_control: bool,
    pub intra_8x8_4x4_prediction_error_concealment: bool,
    pub i_slice_concealment_mode: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcGpuResourceRange {
    pub gpu_addr: u64,
    pub bytes: usize,
}

impl AvcGpuResourceRange {
    pub(crate) const fn end_gpu_addr(self) -> u64 {
        self.gpu_addr.saturating_add(self.bytes as u64)
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcPacketResourceBindings {
    pub dest_surface: AvcGpuResourceRange,
    pub missing_reference_surface: AvcGpuResourceRange,
    pub bitstream: AvcGpuResourceRange,
    pub intra_rowstore: AvcGpuResourceRange,
    pub deblocking_filter_rowstore: AvcGpuResourceRange,
    pub bsd_mpc_rowstore: AvcGpuResourceRange,
    pub mpr_rowstore: AvcGpuResourceRange,
    pub dmv_write_buffer: AvcGpuResourceRange,
    pub dmv_reference_buffer: AvcGpuResourceRange,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxPipeModeSelectDwords {
    pub dwords: [u32; MFX_PIPE_MODE_SELECT_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxSurfaceStateDwords {
    pub dwords: [u32; MFX_SURFACE_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxIndObjBaseAddrStateDwords {
    pub dwords: [u32; MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxPipeBufAddrStateDwords {
    pub dwords: [u32; MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxBspBufBaseAddrStateDwords {
    pub dwords: [u32; MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcDpbStateDwords {
    pub dwords: [u32; MFD_AVC_DPB_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcPicidStateDwords {
    pub dwords: [u32; MFD_AVC_PICID_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcImgStateDwords {
    pub dwords: [u32; MFX_AVC_IMG_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxQmStateDwords {
    pub dwords: [u32; MFX_QM_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcDirectmodeStateDwords {
    pub dwords: [u32; MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcRefIdxStateDwords {
    pub dwords: [u32; MFX_AVC_REF_IDX_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfxAvcSliceStateDwords {
    pub dwords: [u32; MFX_AVC_SLICE_STATE_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct MfdAvcBsdObjectDwords {
    pub dwords: [u32; MFD_AVC_BSD_OBJECT_DWORD_COUNT],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum AvcMilestoneBlocker {
    NotLongFormatSingleIdr,
    UnsupportedPictureStructure,
    UnsupportedChromaFormat,
    UnsupportedSliceClass,
    UnsupportedSliceGroups,
    WeightedPrediction,
    ReferencesPresent,
    InvalidDimensions,
    InvalidSurfaceLayout,
    InvalidSliceRange,
    MissingRowstore,
    MissingDmvBuffer,
}

pub(crate) fn build_long_format_single_idr_packet_params(
    plan: AvcLongFormatIdrPlan,
) -> Result<AvcLongFormatIdrPacketParams, AvcMilestoneBlocker> {
    validate_long_format_single_idr(plan)?;
    Ok(AvcLongFormatIdrPacketParams {
        pipe_mode_select: mfx_pipe_mode_select_params_long_format_avc_vld(),
        surface_state: mfx_surface_state_params_for_nv12_decode_dest(plan.resources.dest_surface),
        pipe_buf_addr_state: mfx_pipe_buf_addr_state_params_for_idr(plan.resources),
        ind_obj_base_addr_state: mfx_ind_obj_base_addr_state_params_for_bitstream(plan),
        bsp_buf_base_addr_state: mfx_bsp_buf_base_addr_state_params(plan.resources.rowstore),
        avc_picid_state: mfd_avc_picid_state_params_for_idr(),
        avc_img_state: mfx_avc_img_state_params_for_idr(plan),
        avc_qm_states: mfx_qm_state_params_flat_avc_defaults(),
        avc_directmode_state: mfx_avc_directmode_state_params_for_idr(plan),
        avc_ref_idx_state: mfx_avc_ref_idx_state_params_dummy_l0(),
        avc_slice_state: mfx_avc_slice_state_params_for_single_idr(plan),
        avc_bsd_object: mfd_avc_bsd_object_params_for_single_idr(plan),
    })
}

pub(crate) fn build_long_format_single_idr_command_stream(
    plan: AvcLongFormatIdrPlan,
    resources: AvcPacketResourceBindings,
) -> Result<AvcLongFormatIdrCommandStream, AvcCommandStreamBlocker> {
    let params = build_long_format_single_idr_packet_params(plan)
        .map_err(AvcCommandStreamBlocker::Milestone)?;
    validate_long_format_single_idr_resource_bindings(plan, resources)?;

    let pipe_mode = encode_mfx_pipe_mode_select(params.pipe_mode_select);
    let surface = encode_mfx_surface_state(params.surface_state);
    let pipe_buf = encode_mfx_pipe_buf_addr_state(params.pipe_buf_addr_state, resources);
    let ind_obj =
        encode_mfx_ind_obj_base_addr_state(params.ind_obj_base_addr_state, resources.bitstream);
    let bsp_buf = encode_mfx_bsp_buf_base_addr_state(params.bsp_buf_base_addr_state, resources);
    let picid = encode_mfd_avc_picid_state(params.avc_picid_state);
    let img = encode_mfx_avc_img_state(params.avc_img_state);
    let qm_states = [
        encode_mfx_qm_state(params.avc_qm_states[0]),
        encode_mfx_qm_state(params.avc_qm_states[1]),
        encode_mfx_qm_state(params.avc_qm_states[2]),
        encode_mfx_qm_state(params.avc_qm_states[3]),
    ];
    let directmode = encode_mfx_avc_directmode_state(
        params.avc_directmode_state,
        resources.dmv_write_buffer,
        resources.dmv_reference_buffer,
    );
    let ref_idx = encode_mfx_avc_ref_idx_state(params.avc_ref_idx_state);
    let slice = encode_mfx_avc_slice_state(params.avc_slice_state);
    let bsd = encode_mfd_avc_bsd_object(params.avc_bsd_object);

    let mut dwords = Vec::with_capacity(AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_DWORDS);
    dwords.extend_from_slice(&[MI_FORCE_WAKEUP_DW0, MI_FORCE_WAKEUP_MFX_WELL_DW1]);
    dwords.push(MFX_WAIT_SYNC_DW0);
    dwords.extend_from_slice(&pipe_mode.dwords);
    dwords.push(MFX_WAIT_SYNC_DW0);
    dwords.extend_from_slice(&surface.dwords);
    dwords.extend_from_slice(&pipe_buf.dwords);
    dwords.extend_from_slice(&ind_obj.dwords);
    dwords.extend_from_slice(&bsp_buf.dwords);
    dwords.extend_from_slice(&picid.dwords);
    dwords.extend_from_slice(&img.dwords);
    for qm in qm_states {
        dwords.extend_from_slice(&qm.dwords);
    }
    dwords.extend_from_slice(&directmode.dwords);
    dwords.extend_from_slice(&ref_idx.dwords);
    dwords.extend_from_slice(&slice.dwords);
    dwords.extend_from_slice(&bsd.dwords);
    dwords.push(MI_FLUSH_DW_VIDEO_DW0);
    dwords.extend_from_slice(&[0, 0, 0, 0]);

    let stream = AvcLongFormatIdrCommandStream {
        dwords,
        command_count: AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT,
    };
    if !validate_long_format_single_idr_command_stream_shape(&stream) {
        return Err(AvcCommandStreamBlocker::CommandShapeMismatch);
    }
    Ok(stream)
}

pub(crate) fn validate_long_format_single_idr_command_stream_shape(
    stream: &AvcLongFormatIdrCommandStream,
) -> bool {
    stream.command_count == AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT
        && stream.dwords.len() == AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_DWORDS
        && validate_long_format_single_idr_command_blocks()
        && stream.dwords[AVC_CMD_OFFSET_FORCE_WAKEUP] == MI_FORCE_WAKEUP_DW0
        && stream.dwords[AVC_CMD_OFFSET_WAIT_BEFORE_PIPE_MODE] == MFX_WAIT_SYNC_DW0
        && stream.dwords[AVC_CMD_OFFSET_PIPE_MODE] == MFX_PIPE_MODE_SELECT_DW0
        && stream.dwords[AVC_CMD_OFFSET_WAIT_AFTER_PIPE_MODE] == MFX_WAIT_SYNC_DW0
        && stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE] == MFX_SURFACE_STATE_DW0
        && (stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 3] & 0x03)
            == MFX_SURFACE_TILEMODE_TILEYS_64K
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE] == MFX_PIPE_BUF_ADDR_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 3] == MFX_MEMORY_OBJECT_CONTROL_UC
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 6] == MFX_MEMORY_OBJECT_CONTROL_UC
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 15] == MFX_MEMORY_OBJECT_CONTROL_UC
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 18] == MFX_MEMORY_OBJECT_CONTROL_UC
        && stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 51] == MFX_MEMORY_OBJECT_CONTROL_UC
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE] == MFX_IND_OBJ_BASE_ADDR_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 3]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 8]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 13]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 18]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 23]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE] == MFX_BSP_BUF_BASE_ADDR_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_PICID_STATE] == MFD_AVC_PICID_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE] == MFX_AVC_IMG_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE] == MFX_QM_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_QM_INTER_4X4_STATE] == MFX_QM_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_QM_INTRA_8X8_STATE] == MFX_QM_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_QM_INTER_8X8_STATE] == MFX_QM_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE] == MFX_AVC_DIRECTMODE_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 33]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 36]
            == MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE] == MFX_AVC_REF_IDX_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 1] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 2] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 3] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 4] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 5] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 6] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 7] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 8] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 9] == 0
        && stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE] == MFX_AVC_SLICE_STATE_DW0
        && stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT] == MFD_AVC_BSD_OBJECT_DW0
        && stream.dwords[AVC_CMD_OFFSET_FLUSH] == MI_FLUSH_DW_VIDEO_DW0
}

pub(crate) const fn validate_long_format_single_idr_command_blocks() -> bool {
    let mut idx = 0;
    let mut expected_offset = 0;
    while idx < AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT {
        let block = AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_BLOCKS[idx];
        if block.offset != expected_offset || block.dword_count == 0 {
            return false;
        }
        expected_offset += block.dword_count;
        idx += 1;
    }
    expected_offset == AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_DWORDS
}

pub(crate) fn validate_long_format_single_idr_resource_bindings(
    plan: AvcLongFormatIdrPlan,
    resources: AvcPacketResourceBindings,
) -> Result<(), AvcCommandStreamBlocker> {
    validate_resource_range(
        resources.dest_surface,
        plan.resources.dest_surface.byte_len,
        "dest_surface",
    )?;
    validate_resource_range(
        resources.missing_reference_surface,
        plan.resources.dest_surface.byte_len,
        "missing_reference_surface",
    )?;
    validate_resource_range(resources.bitstream, plan.bitstream_bytes, "bitstream")?;
    if resources.bitstream.gpu_addr & (MFX_INDIRECT_OBJECT_BASE_ALIGNMENT - 1) != 0 {
        return Err(AvcCommandStreamBlocker::ResourceUnaligned("bitstream_4k"));
    }
    if resources.bitstream.bytes as u64 & (MFX_INDIRECT_OBJECT_BASE_ALIGNMENT - 1) != 0 {
        return Err(AvcCommandStreamBlocker::ResourceUnaligned("bitstream_upper_bound_4k"));
    }
    validate_resource_range(
        resources.intra_rowstore,
        plan.resources.rowstore.intra,
        "intra_rowstore",
    )?;
    validate_resource_range(
        resources.deblocking_filter_rowstore,
        plan.resources.rowstore.deblocking_filter,
        "deblocking_filter_rowstore",
    )?;
    validate_resource_range(
        resources.bsd_mpc_rowstore,
        plan.resources.rowstore.bsd_mpc,
        "bsd_mpc_rowstore",
    )?;
    validate_resource_range(resources.mpr_rowstore, plan.resources.rowstore.mpr, "mpr_rowstore")?;
    validate_resource_range(
        resources.dmv_write_buffer,
        plan.resources.dmv_write_buffer_bytes,
        "dmv_write_buffer",
    )?;
    validate_resource_range(
        resources.dmv_reference_buffer,
        plan.resources.dmv_reference_buffer_bytes,
        "dmv_reference_buffer",
    )?;
    Ok(())
}

pub(crate) fn parse_annexb_single_idr_plan(
    bytes: &[u8],
) -> Result<AvcLongFormatIdrPlan, AvcAnnexBPlanError> {
    let mut sps = None;
    let mut pps = None;
    let mut idr = None;

    let mut scan = AnnexBNalScanner::new(bytes);
    while let Some(nal) = scan.next() {
        if nal.payload_start >= nal.payload_end {
            continue;
        }
        let nal_header = bytes[nal.payload_start];
        let nal_ref_idc = (nal_header >> 5) & 0x03;
        let nal_type = nal_header & 0x1f;
        let payload = &bytes[nal.payload_start + 1..nal.payload_end];
        match nal_type {
            7 => sps = Some(parse_sps(payload)?),
            8 => pps = Some(parse_pps(payload)?),
            5 => idr = Some((nal, nal_ref_idc, payload)),
            1 | 6 | 9 => {}
            _ => return Err(AvcAnnexBPlanError::UnsupportedNal),
        }
    }

    let sps = sps.ok_or(AvcAnnexBPlanError::MissingSps)?;
    let pps = pps.ok_or(AvcAnnexBPlanError::MissingPps)?;
    let (idr_nal, nal_ref_idc, idr_payload) = idr.ok_or(AvcAnnexBPlanError::MissingIdrSlice)?;
    if pps.seq_parameter_set_id != sps.seq_parameter_set_id {
        return Err(AvcAnnexBPlanError::UnsupportedPps);
    }

    let slice = parse_idr_slice(idr_payload, &sps, &pps)?;
    let pitch_bytes = align_ceil_usize(sps.coded_width as usize, MEDIA_TILE64_W);
    let chroma_y_offset = align_ceil_usize(sps.coded_height as usize, MEDIA_TILE64_H);
    let surface = AvcSurfaceLayout::nv12_tile64(
        sps.coded_width,
        sps.coded_height,
        pitch_bytes,
        pitch_bytes * chroma_y_offset,
    );
    let picture = AvcPictureParams {
        pic_width_in_mbs_minus1: sps.pic_width_in_mbs_minus1,
        pic_height_in_mbs_minus1: sps.pic_height_in_mbs_minus1,
        picture_structure: AvcPictureStructure::Frame,
        chroma_format: AvcChromaFormat::Yuv420,
        entropy_coding_mode: pps.entropy_coding_mode,
        weighted_pred: pps.weighted_pred,
        weighted_bipred_idc: pps.weighted_bipred_idc,
        transform_8x8: pps.transform_8x8,
        constrained_intra_pred: pps.constrained_intra_pred,
        direct_8x8_inference: sps.direct_8x8_inference,
        frame_mbs_only: sps.frame_mbs_only,
        mb_adaptive_frame_field: sps.mb_adaptive_frame_field,
        field_pic: false,
        reference_pic: nal_ref_idc != 0,
        num_ref_idx_l0_active_minus1: pps.num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_active_minus1: pps.num_ref_idx_l1_default_active_minus1,
        pic_init_qp_minus26: pps.pic_init_qp_minus26,
        chroma_qp_index_offset: pps.chroma_qp_index_offset,
        second_chroma_qp_index_offset: pps.second_chroma_qp_index_offset,
        frame_num: slice.frame_num,
        log2_max_frame_num_minus4: sps.log2_max_frame_num_minus4,
        log2_max_pic_order_cnt_lsb_minus4: sps.log2_max_pic_order_cnt_lsb_minus4,
        num_slice_groups_minus1: pps.num_slice_groups_minus1,
        redundant_pic_cnt_present: pps.redundant_pic_cnt_present,
        pic_order_present: pps.pic_order_present,
        slice_group_map_type: pps.slice_group_map_type,
        pic_order_cnt_type: sps.pic_order_cnt_type,
        top_field_order_cnt: slice.top_field_order_cnt,
        bottom_field_order_cnt: slice.bottom_field_order_cnt,
        deblocking_filter_control_present: pps.deblocking_filter_control_present,
        delta_pic_order_always_zero: sps.delta_pic_order_always_zero,
        slice_group_change_rate_minus1: pps.slice_group_change_rate_minus1,
        visible_width: sps.visible_width,
        visible_height: sps.visible_height,
    };
    let resources = AvcDecodeResourcePlan {
        dest_surface: surface,
        rowstore: avc_rowstore_scratch_bytes(picture.pic_width_in_mbs()),
        dmv_write_buffer_bytes: avc_dmv_buffer_bytes_for_picture(picture),
        dmv_reference_buffer_bytes: avc_dmv_buffer_bytes_for_picture(picture),
        reference_surface_count: 0,
    };
    Ok(AvcLongFormatIdrPlan {
        picture,
        slice: AvcSliceParams {
            class: slice.class,
            first_mb_in_slice: slice.first_mb_in_slice,
            first_mb_in_next_slice: picture.macroblock_count() as u32,
            slice_data_offset: idr_nal.payload_start as u32,
            slice_data_size: (idr_nal.payload_end - idr_nal.payload_start) as u32,
            slice_data_bit_offset_from_payload: slice.first_mb_bit_offset_from_payload,
            first_mb_byte_offset: avc_long_format_first_mb_byte_offset(
                slice.first_mb_bit_offset_from_payload,
            ),
            slice_data_bit_offset: avc_long_format_first_mb_bit_offset(
                slice.first_mb_bit_offset_from_payload,
            ),
            disable_deblocking_filter_idc: slice.disable_deblocking_filter_idc,
            slice_alpha_c0_offset_div2: slice.slice_alpha_c0_offset_div2,
            slice_beta_offset_div2: slice.slice_beta_offset_div2,
            slice_qp_delta: slice.slice_qp_delta,
            direct_spatial_mv_pred: false,
            num_ref_idx_l0_active_minus1: pps.num_ref_idx_l0_default_active_minus1,
            num_ref_idx_l1_active_minus1: pps.num_ref_idx_l1_default_active_minus1,
            top_field_order_cnt: slice.top_field_order_cnt,
            bottom_field_order_cnt: slice.bottom_field_order_cnt,
        },
        resources,
        bitstream_bytes: bytes.len(),
        bitstream_data_offset: 0,
    })
}

pub(crate) fn validate_long_format_single_idr(
    plan: AvcLongFormatIdrPlan,
) -> Result<(), AvcMilestoneBlocker> {
    if plan.picture.picture_structure != AvcPictureStructure::Frame {
        return Err(AvcMilestoneBlocker::UnsupportedPictureStructure);
    }
    if !plan.picture.frame_mbs_only
        || plan.picture.field_pic
        || plan.picture.mb_adaptive_frame_field
    {
        return Err(AvcMilestoneBlocker::UnsupportedPictureStructure);
    }
    if plan.picture.chroma_format != AvcChromaFormat::Yuv420 {
        return Err(AvcMilestoneBlocker::UnsupportedChromaFormat);
    }
    if !plan.slice.class.is_i_only() {
        return Err(AvcMilestoneBlocker::UnsupportedSliceClass);
    }
    if plan.picture.weighted_pred || plan.picture.weighted_bipred_idc != 0 {
        return Err(AvcMilestoneBlocker::WeightedPrediction);
    }
    let slice_qp = 26 + plan.picture.pic_init_qp_minus26 as i32 + plan.slice.slice_qp_delta as i32;
    if !(0..=51).contains(&slice_qp)
        || plan.slice.disable_deblocking_filter_idc > 2
        || !(-6..=6).contains(&plan.slice.slice_alpha_c0_offset_div2)
        || !(-6..=6).contains(&plan.slice.slice_beta_offset_div2)
    {
        return Err(AvcMilestoneBlocker::InvalidSliceRange);
    }
    if plan.resources.reference_surface_count != 0 {
        return Err(AvcMilestoneBlocker::ReferencesPresent);
    }
    if plan.picture.num_slice_groups_minus1 != 0 {
        return Err(AvcMilestoneBlocker::UnsupportedSliceGroups);
    }
    if plan.picture.coded_width() == 0
        || plan.picture.coded_height() == 0
        || plan.picture.macroblock_count() == 0
        || plan.picture.macroblock_count() > MFX_AVC_IMG_FRAME_SIZE_MAX as usize
        || plan.picture.pic_width_in_mbs_minus1 as u32 > MFX_AVC_IMG_DIMENSION_MAX
        || plan.picture.pic_height_in_mbs_minus1 as u32 > MFX_AVC_IMG_DIMENSION_MAX
    {
        return Err(AvcMilestoneBlocker::InvalidDimensions);
    }
    if plan.resources.dest_surface.format != AvcSurfaceFormat::Nv12
        || plan.resources.dest_surface.width < plan.picture.coded_width()
        || plan.resources.dest_surface.height < plan.picture.coded_height()
        || plan.resources.dest_surface.pitch_bytes < plan.picture.coded_width() as usize
        || plan.resources.dest_surface.uv_offset
            < plan.resources.dest_surface.pitch_bytes * plan.picture.coded_height() as usize
        || plan.resources.dest_surface.byte_len <= plan.resources.dest_surface.uv_offset
    {
        return Err(AvcMilestoneBlocker::InvalidSurfaceLayout);
    }
    let surface = plan.resources.dest_surface;
    let uv_y =
        align_ceil_usize(surface.uv_offset / surface.pitch_bytes, MFX_UV_PLANE_ALIGNMENT_LEGACY);
    if surface.width - 1 > MFX_SURFACE_DIMENSION_MAX
        || surface.height - 1 > MFX_SURFACE_DIMENSION_MAX
        || surface.pitch_bytes as u32 - 1 > MFX_SURFACE_PITCH_MAX
        || uv_y as u32 > MFX_SURFACE_U_CB_Y_OFFSET_MAX
        || uv_y as u32 > MFX_SURFACE_V_CR_Y_OFFSET_MAX
    {
        return Err(AvcMilestoneBlocker::InvalidSurfaceLayout);
    }
    if plan.slice.slice_data_size == 0
        || plan.slice.first_mb_in_slice != 0
        || plan.slice.first_mb_in_next_slice as usize != plan.picture.macroblock_count()
        || plan.slice.slice_data_offset as usize >= plan.bitstream_bytes
        || plan.slice.first_mb_byte_offset >= plan.slice.slice_data_size
        || plan.slice.first_mb_byte_offset
            != avc_long_format_first_mb_byte_offset(plan.slice.slice_data_bit_offset_from_payload)
        || plan.slice.slice_data_bit_offset
            != avc_long_format_first_mb_bit_offset(plan.slice.slice_data_bit_offset_from_payload)
        || avc_long_format_slice_record(plan.slice).offset != plan.slice.first_mb_byte_offset
        || avc_long_format_slice_record(plan.slice).length
            + avc_long_format_slice_record(plan.slice).offset
            != plan.slice.slice_data_size
        || (plan.slice.slice_data_offset as usize)
            .saturating_add(plan.slice.slice_data_size as usize)
            > plan.bitstream_bytes
        || plan.slice.slice_data_bit_offset > 7
    {
        return Err(AvcMilestoneBlocker::InvalidSliceRange);
    }
    if plan.resources.rowstore != avc_rowstore_scratch_bytes(plan.picture.pic_width_in_mbs()) {
        return Err(AvcMilestoneBlocker::MissingRowstore);
    }
    let expected_dmv_bytes = avc_dmv_buffer_bytes_for_picture(plan.picture);
    if plan.resources.dmv_write_buffer_bytes < expected_dmv_bytes
        || plan.resources.dmv_reference_buffer_bytes < expected_dmv_bytes
    {
        return Err(AvcMilestoneBlocker::MissingDmvBuffer);
    }
    Ok(())
}

pub(crate) const CODECHAL_DECODE_MODE_AVCVLD: u32 = 4;
pub(crate) const CODECHAL_AVC: u32 = 2;
pub(crate) const MFX_CODEC_SELECT_DECODE: u8 = 0;
pub(crate) const MFX_DECODER_MODE_VLD: u8 = 0;
pub(crate) const MFX_DECODER_LONG_FORMAT_MODE: u8 = 1;
pub(crate) const MFX_SURFACE_ID_DECODED_PICTURE_AND_REFERENCES: u8 = 0;
pub(crate) const MFX_SURFACE_TILEMODE_TILEYS_64K: u32 = 1;
pub(crate) const MFX_SURFACE_FORMAT_PLANAR_420_8: u32 = 4;
pub(crate) const MFX_INTERLEAVE_CHROMA_ENABLE: u8 = 1;
pub(crate) const MFX_COMPRESSION_FORMAT_DISABLED: u32 = 0;
pub(crate) const MFX_UV_PLANE_ALIGNMENT_LEGACY: usize = 16;
pub(crate) const MFX_SURFACE_DIMENSION_MAX: u32 = 0x3fff;
pub(crate) const MFX_SURFACE_PITCH_MAX: u32 = 0x1_ffff;
pub(crate) const MFX_SURFACE_U_CB_Y_OFFSET_MAX: u32 = 0x7fff;
pub(crate) const MFX_SURFACE_V_CR_Y_OFFSET_MAX: u32 = 0xffff;
pub(crate) const MEDIA_TILE64_W: usize = 256;
pub(crate) const MEDIA_TILE64_H: usize = 256;
pub(crate) const MFX_MEMORY_OBJECT_CONTROL_UC: u32 = 1;
pub(crate) const MFX_MEMORY_ADDRESS_ATTRIBUTES_UC: u32 = 1 << 1;
pub(crate) const MFX_INDIRECT_OBJECT_BASE_ALIGNMENT: u64 = 4096;

pub(crate) const MFX_PIPE_MODE_SELECT_DWORD_COUNT: usize = 5;
pub(crate) const MFX_SURFACE_STATE_DWORD_COUNT: usize = 6;
pub(crate) const MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT: usize = 68;
pub(crate) const MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT: usize = 26;
pub(crate) const MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT: usize = 10;
pub(crate) const MFD_AVC_DPB_STATE_DWORD_COUNT: usize = 27;
pub(crate) const MFD_AVC_PICID_STATE_DWORD_COUNT: usize = 10;
pub(crate) const MFX_AVC_IMG_STATE_DWORD_COUNT: usize = 21;
pub(crate) const MFX_QM_STATE_DWORD_COUNT: usize = 18;
pub(crate) const AVC_QM_STATE_COUNT: usize = 4;
pub(crate) const MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT: usize = 71;
pub(crate) const MFX_AVC_REF_IDX_STATE_DWORD_COUNT: usize = 10;
pub(crate) const MFX_AVC_SLICE_STATE_DWORD_COUNT: usize = 11;
pub(crate) const MFD_AVC_BSD_OBJECT_DWORD_COUNT: usize = 7;
pub(crate) const MFX_AVC_DMV_DEST_TOP: usize = 32;
pub(crate) const MFX_AVC_DMV_DEST_BOTTOM: usize = 33;
pub(crate) const AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT: usize = 19;
pub(crate) const AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_DWORDS: usize = 2
    + 1
    + MFX_PIPE_MODE_SELECT_DWORD_COUNT
    + 1
    + MFX_SURFACE_STATE_DWORD_COUNT
    + MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT
    + MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT
    + MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT
    + MFD_AVC_PICID_STATE_DWORD_COUNT
    + MFX_AVC_IMG_STATE_DWORD_COUNT
    + (MFX_QM_STATE_DWORD_COUNT * AVC_QM_STATE_COUNT)
    + MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT
    + MFX_AVC_REF_IDX_STATE_DWORD_COUNT
    + MFX_AVC_SLICE_STATE_DWORD_COUNT
    + MFD_AVC_BSD_OBJECT_DWORD_COUNT
    + MI_FLUSH_DW_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_FORCE_WAKEUP: usize = 0;
pub(crate) const AVC_CMD_OFFSET_WAIT_BEFORE_PIPE_MODE: usize = 2;
pub(crate) const AVC_CMD_OFFSET_PIPE_MODE: usize = AVC_CMD_OFFSET_WAIT_BEFORE_PIPE_MODE + 1;
pub(crate) const AVC_CMD_OFFSET_WAIT_AFTER_PIPE_MODE: usize =
    AVC_CMD_OFFSET_PIPE_MODE + MFX_PIPE_MODE_SELECT_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_SURFACE_STATE: usize = AVC_CMD_OFFSET_WAIT_AFTER_PIPE_MODE + 1;
pub(crate) const AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE: usize =
    AVC_CMD_OFFSET_SURFACE_STATE + MFX_SURFACE_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE: usize =
    AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE: usize =
    AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_PICID_STATE: usize =
    AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE + MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_IMG_STATE: usize =
    AVC_CMD_OFFSET_AVC_PICID_STATE + MFD_AVC_PICID_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE: usize =
    AVC_CMD_OFFSET_AVC_IMG_STATE + MFX_AVC_IMG_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_QM_INTER_4X4_STATE: usize =
    AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE + MFX_QM_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_QM_INTRA_8X8_STATE: usize =
    AVC_CMD_OFFSET_AVC_QM_INTER_4X4_STATE + MFX_QM_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_QM_INTER_8X8_STATE: usize =
    AVC_CMD_OFFSET_AVC_QM_INTRA_8X8_STATE + MFX_QM_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE: usize =
    AVC_CMD_OFFSET_AVC_QM_INTER_8X8_STATE + MFX_QM_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_REF_IDX_STATE: usize =
    AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_SLICE_STATE: usize =
    AVC_CMD_OFFSET_AVC_REF_IDX_STATE + MFX_AVC_REF_IDX_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_AVC_BSD_OBJECT: usize =
    AVC_CMD_OFFSET_AVC_SLICE_STATE + MFX_AVC_SLICE_STATE_DWORD_COUNT;
pub(crate) const AVC_CMD_OFFSET_FLUSH: usize =
    AVC_CMD_OFFSET_AVC_BSD_OBJECT + MFD_AVC_BSD_OBJECT_DWORD_COUNT;

pub(crate) const AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_BLOCKS: [AvcCommandBlock;
    AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT] = [
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_FORCE_WAKEUP,
        dword_count: 2,
        command: "MI_FORCE_WAKEUP",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_packet.cpp",
        upstream_symbol: "AvcDecodePkt::AddForceWakeup",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_WAIT_BEFORE_PIPE_MODE,
        dword_count: 1,
        command: "MFX_WAIT(before pipe mode)",
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_picture_packet_xe_lpm_plus_base.cpp",
        upstream_symbol: "AvcDecodePicPktXe_Lpm_Plus_Base::Execute",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_PIPE_MODE,
        dword_count: MFX_PIPE_MODE_SELECT_DWORD_COUNT,
        command: "MFX_PIPE_MODE_SELECT",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_PIPE_MODE_SELECT, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_WAIT_AFTER_PIPE_MODE,
        dword_count: 1,
        command: "MFX_WAIT(after pipe mode)",
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_picture_packet_xe_lpm_plus_base.cpp",
        upstream_symbol: "AvcDecodePicPktXe_Lpm_Plus_Base::Execute",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_SURFACE_STATE,
        dword_count: MFX_SURFACE_STATE_DWORD_COUNT,
        command: "MFX_SURFACE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_SURFACE_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE,
        dword_count: MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT,
        command: "MFX_PIPE_BUF_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_PIPE_BUF_ADDR_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE,
        dword_count: MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT,
        command: "MFX_IND_OBJ_BASE_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_IND_OBJ_BASE_ADDR_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE,
        dword_count: MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT,
        command: "MFX_BSP_BUF_BASE_ADDR_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_BSP_BUF_BASE_ADDR_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_PICID_STATE,
        dword_count: MFD_AVC_PICID_STATE_DWORD_COUNT,
        command: "MFD_AVC_PICID_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFD_AVC_PICID_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_IMG_STATE,
        dword_count: MFX_AVC_IMG_STATE_DWORD_COUNT,
        command: "MFX_AVC_IMG_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_AVC_IMG_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_QM_INTRA_4X4_STATE,
        dword_count: MFX_QM_STATE_DWORD_COUNT,
        command: "MFX_QM_STATE(intra 4x4)",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "AvcDecodePicPkt::AddAllCmds_MFX_QM_STATE",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_QM_INTER_4X4_STATE,
        dword_count: MFX_QM_STATE_DWORD_COUNT,
        command: "MFX_QM_STATE(inter 4x4)",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "AvcDecodePicPkt::AddAllCmds_MFX_QM_STATE",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_QM_INTRA_8X8_STATE,
        dword_count: MFX_QM_STATE_DWORD_COUNT,
        command: "MFX_QM_STATE(intra 8x8)",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "AvcDecodePicPkt::AddAllCmds_MFX_QM_STATE",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_QM_INTER_8X8_STATE,
        dword_count: MFX_QM_STATE_DWORD_COUNT,
        command: "MFX_QM_STATE(inter 8x8)",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "AvcDecodePicPkt::AddAllCmds_MFX_QM_STATE",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE,
        dword_count: MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT,
        command: "MFX_AVC_DIRECTMODE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp",
        upstream_symbol: "SETPAR(MFX_AVC_DIRECTMODE_STATE, AvcDecodePicPkt)",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_REF_IDX_STATE,
        dword_count: MFX_AVC_REF_IDX_STATE_DWORD_COUNT,
        command: "MFX_AVC_REF_IDX_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::AddCmd_AVC_SLICE_REF_IDX",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_SLICE_STATE,
        dword_count: MFX_AVC_SLICE_STATE_DWORD_COUNT,
        command: "MFX_AVC_SLICE_STATE",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::SET_AVC_SLICE_STATE",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_AVC_BSD_OBJECT,
        dword_count: MFD_AVC_BSD_OBJECT_DWORD_COUNT,
        command: "MFD_AVC_BSD_OBJECT",
        upstream_file: "media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp",
        upstream_symbol: "AvcDecodeSlcPkt::AddCmd_AVC_BSD_OBJECT",
    },
    AvcCommandBlock {
        offset: AVC_CMD_OFFSET_FLUSH,
        dword_count: MI_FLUSH_DW_DWORD_COUNT,
        command: "MI_FLUSH_DW",
        upstream_file: "media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_packet_xe_lpm_plus_base.cpp",
        upstream_symbol: "AvcDecodePktXe_Lpm_Plus_Base::EnsureAllCommandsExecuted",
    },
];
pub(crate) const MI_FORCE_WAKEUP_DW0: u32 = 29 << 23;
pub(crate) const MI_FORCE_WAKEUP_MFX_WELL_DW1: u32 = (1 << 9) | (0x300 << 16);
pub(crate) const MFX_WAIT_SYNC_DW0: u32 = (3 << 29) | (1 << 27) | (1 << 8);
pub(crate) const MI_FLUSH_DW_DWORD_COUNT: usize = 5;
pub(crate) const MI_FLUSH_DW_VIDEO_DW0: u32 = ((0x26 << 23) | 3) | (1 << 7);
pub(crate) const MFX_PIPE_MODE_SELECT_DW0: u32 = 0x7000_0003;
pub(crate) const MFX_SURFACE_STATE_DW0: u32 = 0x7001_0004;
pub(crate) const MFX_PIPE_BUF_ADDR_STATE_DW0: u32 = 0x7002_0042;
pub(crate) const MFX_IND_OBJ_BASE_ADDR_STATE_DW0: u32 = 0x7003_0018;
pub(crate) const MFX_BSP_BUF_BASE_ADDR_STATE_DW0: u32 = 0x7004_0008;
pub(crate) const MFD_AVC_DPB_STATE_DW0: u32 = 0x7126_0019;
pub(crate) const MFD_AVC_PICID_STATE_DW0: u32 = 0x7125_0008;
pub(crate) const MFX_AVC_IMG_STATE_DW0: u32 = 0x7100_0013;
pub(crate) const MFX_AVC_IMG_STATE_DW5_DEFAULT: u32 = 0x3000_0000;
pub(crate) const MFX_AVC_IMG_FRAME_SIZE_MAX: u32 = 0xffff;
pub(crate) const MFX_AVC_IMG_DIMENSION_MAX: u32 = 0xff;
pub(crate) const MFX_QM_STATE_DW0: u32 = 0x7007_0010;
pub(crate) const MFX_AVC_DIRECTMODE_STATE_DW0: u32 = 0x7102_0045;
pub(crate) const MFX_AVC_REF_IDX_STATE_DW0: u32 = 0x7104_0008;
pub(crate) const MFX_AVC_SLICE_STATE_DW0: u32 = 0x7103_0009;
pub(crate) const MFX_AVC_SLICE_POSITION_MAX: u32 = 0xff;
pub(crate) const MFX_AVC_SLICE_NEXT_POSITION_MAX: u32 = 0x1ff;
pub(crate) const MFD_AVC_BSD_OBJECT_DW0: u32 = 0x7128_0005;
pub(crate) const MFX_QM_AVC_4X4_INTRA: u8 = 0;
pub(crate) const MFX_QM_AVC_4X4_INTER: u8 = 1;
pub(crate) const MFX_QM_AVC_8X8_INTRA: u8 = 2;
pub(crate) const MFX_QM_AVC_8X8_INTER: u8 = 3;
pub(crate) const AVC_FLAT_QM_DWORD: u32 = 0x1010_1010;
pub(crate) const MFX_AVC_IMG_STRUCTURE_FRAME: u8 = 0;
pub(crate) const MFX_AVC_IMG_STRUCTURE_TOP_FIELD: u8 = 1;
pub(crate) const MFX_AVC_IMG_STRUCTURE_BOTTOM_FIELD: u8 = 3;
pub(crate) const MFX_AVC_SLICE_TYPE_P: u8 = 0;
pub(crate) const MFX_AVC_SLICE_TYPE_B: u8 = 1;
pub(crate) const MFX_AVC_SLICE_TYPE_I: u8 = 2;
pub(crate) const MFX_GENERAL_STATE_ALIGNMENT: u64 = 64;
pub(crate) const AVC_NAL_UNIT_BYTES_INCLUDED: u32 = 1;

pub(crate) const fn avc_missing_reference_surface_offset(dest_surface_bytes: usize) -> usize {
    align_ceil_usize(dest_surface_bytes, MFX_GENERAL_STATE_ALIGNMENT as usize)
}

pub(crate) const fn avc_long_format_first_mb_byte_offset(
    slice_data_bit_offset_from_payload: u32,
) -> u32 {
    (slice_data_bit_offset_from_payload >> 3) + AVC_NAL_UNIT_BYTES_INCLUDED
}

pub(crate) const fn avc_long_format_first_mb_bit_offset(
    slice_data_bit_offset_from_payload: u32,
) -> u8 {
    (slice_data_bit_offset_from_payload & 0x07) as u8
}

pub(crate) const fn avc_long_format_slice_record(
    slice: AvcSliceParams,
) -> AvcLongFormatSliceRecord {
    let offset = avc_long_format_first_mb_byte_offset(slice.slice_data_bit_offset_from_payload);
    AvcLongFormatSliceRecord {
        offset,
        length: slice.slice_data_size.saturating_sub(offset),
    }
}

pub(crate) const fn mfx_pipe_mode_select_params_long_format_avc_vld() -> MfxPipeModeSelectParams {
    MfxPipeModeSelectParams {
        mode: CODECHAL_DECODE_MODE_AVCVLD,
        standard_select: CODECHAL_AVC,
        codec_select: MFX_CODEC_SELECT_DECODE,
        pre_deblocking_output_enable: false,
        post_deblocking_output_enable: true,
        stream_out_enable: false,
        deblocker_stream_out_enable: false,
        decoder_mode_select: MFX_DECODER_MODE_VLD,
        decoder_short_format_mode: MFX_DECODER_LONG_FORMAT_MODE,
        short_format_in_use: false,
    }
}

pub(crate) const fn encode_mfx_pipe_mode_select(
    params: MfxPipeModeSelectParams,
) -> MfxPipeModeSelectDwords {
    MfxPipeModeSelectDwords {
        dwords: [
            MFX_PIPE_MODE_SELECT_DW0,
            mfx_pipe_mode_select_dw1(params),
            0,
            0,
            0,
        ],
    }
}

pub(crate) const fn mfx_surface_state_params_for_nv12_decode_dest(
    surface: AvcSurfaceLayout,
) -> MfxSurfaceStateParams {
    let uv_y =
        align_ceil_usize(surface.uv_offset / surface.pitch_bytes, MFX_UV_PLANE_ALIGNMENT_LEGACY);
    MfxSurfaceStateParams {
        surface_id: MFX_SURFACE_ID_DECODED_PICTURE_AND_REFERENCES,
        width_minus1: surface.width - 1,
        height_minus1: surface.height - 1,
        tilemode: MFX_SURFACE_TILEMODE_TILEYS_64K,
        surface_pitch_minus1: surface.pitch_bytes as u32 - 1,
        compression_format: MFX_COMPRESSION_FORMAT_DISABLED,
        interleave_chroma: MFX_INTERLEAVE_CHROMA_ENABLE,
        surface_format: MFX_SURFACE_FORMAT_PLANAR_420_8,
        y_offset_for_u_cb: uv_y as u32,
        y_offset_for_v_cr: uv_y as u32,
    }
}

pub(crate) const fn encode_mfx_surface_state(
    params: MfxSurfaceStateParams,
) -> MfxSurfaceStateDwords {
    MfxSurfaceStateDwords {
        dwords: [
            MFX_SURFACE_STATE_DW0,
            (params.surface_id as u32) & 0x0f,
            ((params.width_minus1 & 0x3fff) << 4) | ((params.height_minus1 & 0x3fff) << 18),
            (params.tilemode & 0x03)
                | ((params.surface_pitch_minus1 & 0x1_ffff) << 3)
                | ((params.compression_format & 0x1f) << 22)
                | (((params.interleave_chroma as u32) & 0x01) << 27)
                | ((params.surface_format & 0x0f) << 28),
            params.y_offset_for_u_cb & 0x7fff,
            params.y_offset_for_v_cr & 0xffff,
        ],
    }
}

pub(crate) const fn encode_mfx_pipe_buf_addr_state(
    _params: MfxPipeBufAddrStateParams,
    resources: AvcPacketResourceBindings,
) -> MfxPipeBufAddrStateDwords {
    let mut dwords = [0u32; MFX_PIPE_BUF_ADDR_STATE_DWORD_COUNT];
    dwords[0] = MFX_PIPE_BUF_ADDR_STATE_DW0;
    write_addr64(&mut dwords, 1, resources.dest_surface.gpu_addr);
    dwords[3] = MFX_MEMORY_OBJECT_CONTROL_UC;
    write_addr64(&mut dwords, 4, resources.dest_surface.gpu_addr);
    dwords[6] = MFX_MEMORY_OBJECT_CONTROL_UC;
    write_addr64(&mut dwords, 13, resources.intra_rowstore.gpu_addr);
    dwords[15] = MFX_MEMORY_OBJECT_CONTROL_UC;
    write_addr64(&mut dwords, 16, resources.deblocking_filter_rowstore.gpu_addr);
    dwords[18] = MFX_MEMORY_OBJECT_CONTROL_UC;
    let mut ref_idx = 0;
    while ref_idx < 16 {
        write_addr64(&mut dwords, 19 + ref_idx * 2, resources.missing_reference_surface.gpu_addr);
        ref_idx += 1;
    }
    dwords[51] = MFX_MEMORY_OBJECT_CONTROL_UC;
    MfxPipeBufAddrStateDwords { dwords }
}

pub(crate) const fn mfx_pipe_buf_addr_state_params_for_idr(
    resources: AvcDecodeResourcePlan,
) -> MfxPipeBufAddrStateParams {
    MfxPipeBufAddrStateParams {
        mode: CODECHAL_DECODE_MODE_AVCVLD,
        decode_in_use: true,
        post_deblock_surface_is_dest: true,
        pre_deblock_surface_is_dest: false,
        intra_rowstore_bytes: resources.rowstore.intra,
        deblocking_filter_rowstore_bytes: resources.rowstore.deblocking_filter,
        reference_surface_count: resources.reference_surface_count,
    }
}

pub(crate) const fn mfx_ind_obj_base_addr_state_params_for_bitstream(
    plan: AvcLongFormatIdrPlan,
) -> MfxIndObjBaseAddrStateParams {
    MfxIndObjBaseAddrStateParams {
        mode: CODECHAL_DECODE_MODE_AVCVLD,
        data_size: plan.bitstream_bytes as u32,
        data_offset: plan.bitstream_data_offset,
    }
}

pub(crate) const fn encode_mfx_ind_obj_base_addr_state(
    params: MfxIndObjBaseAddrStateParams,
    bitstream: AvcGpuResourceRange,
) -> MfxIndObjBaseAddrStateDwords {
    let base = bitstream.gpu_addr.saturating_add(params.data_offset as u64);
    let upper_bound = bitstream.gpu_addr.saturating_add(bitstream.bytes as u64);
    let mut dwords = [0u32; MFX_IND_OBJ_BASE_ADDR_STATE_DWORD_COUNT];
    dwords[0] = MFX_IND_OBJ_BASE_ADDR_STATE_DW0;
    dwords[1] = base as u32;
    dwords[2] = (base >> 32) as u32;
    dwords[3] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    dwords[4] = upper_bound as u32;
    dwords[5] = (upper_bound >> 32) as u32;
    dwords[8] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    dwords[13] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    dwords[18] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    dwords[23] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    MfxIndObjBaseAddrStateDwords { dwords }
}

pub(crate) const fn mfx_bsp_buf_base_addr_state_params(
    rowstore: AvcRowstoreScratchBytes,
) -> MfxBspBufBaseAddrStateParams {
    MfxBspBufBaseAddrStateParams {
        bsd_mpc_rowstore_bytes: rowstore.bsd_mpc,
        mpr_rowstore_bytes: rowstore.mpr,
    }
}

pub(crate) const fn encode_mfx_bsp_buf_base_addr_state(
    _params: MfxBspBufBaseAddrStateParams,
    resources: AvcPacketResourceBindings,
) -> MfxBspBufBaseAddrStateDwords {
    let mut dwords = [0u32; MFX_BSP_BUF_BASE_ADDR_STATE_DWORD_COUNT];
    dwords[0] = MFX_BSP_BUF_BASE_ADDR_STATE_DW0;
    write_addr64(&mut dwords, 1, resources.bsd_mpc_rowstore.gpu_addr);
    dwords[3] = mfx_bsp_rowstore_mocs_attr(MFX_MEMORY_OBJECT_CONTROL_UC);
    write_addr64(&mut dwords, 4, resources.mpr_rowstore.gpu_addr);
    dwords[6] = mfx_bsp_rowstore_mocs_attr(MFX_MEMORY_OBJECT_CONTROL_UC);
    MfxBspBufBaseAddrStateDwords { dwords }
}

pub(crate) const fn mfd_avc_dpb_state_params_for_idr() -> MfdAvcDpbStateParams {
    MfdAvcDpbStateParams {
        non_existing_frame_flags: 0,
        long_term_frame_flags: 0,
        used_for_reference_flags: 0,
        ref_frame_order: [0; 16],
    }
}

pub(crate) const fn encode_mfd_avc_dpb_state(params: MfdAvcDpbStateParams) -> MfdAvcDpbStateDwords {
    let mut dwords = [0u32; MFD_AVC_DPB_STATE_DWORD_COUNT];
    dwords[0] = MFD_AVC_DPB_STATE_DW0;
    dwords[1] =
        (params.non_existing_frame_flags as u32) | ((params.long_term_frame_flags as u32) << 16);
    dwords[2] = params.used_for_reference_flags;
    let mut idx = 0;
    while idx < 8 {
        dwords[3 + idx] =
            pack_u16x2(params.ref_frame_order[idx * 2], params.ref_frame_order[idx * 2 + 1]);
        idx += 1;
    }
    MfdAvcDpbStateDwords { dwords }
}

pub(crate) const fn mfd_avc_picid_state_params_for_idr() -> MfdAvcPicidStateParams {
    MfdAvcPicidStateParams {
        picture_id_remapping_disable: true,
        picture_id_list: [0; 16],
    }
}

pub(crate) const fn encode_mfd_avc_picid_state(
    params: MfdAvcPicidStateParams,
) -> MfdAvcPicidStateDwords {
    let mut dwords = [0u32; MFD_AVC_PICID_STATE_DWORD_COUNT];
    dwords[0] = MFD_AVC_PICID_STATE_DW0;
    dwords[1] = bool_bit(params.picture_id_remapping_disable);
    let mut idx = 0;
    while idx < 8 {
        dwords[2 + idx] =
            pack_u16x2(params.picture_id_list[idx * 2], params.picture_id_list[idx * 2 + 1]);
        idx += 1;
    }
    MfdAvcPicidStateDwords { dwords }
}

pub(crate) const fn mfx_avc_img_state_params_for_idr(
    plan: AvcLongFormatIdrPlan,
) -> MfxAvcImgStateParams {
    let picture = plan.picture;
    MfxAvcImgStateParams {
        frame_size_mbs: picture.macroblock_count() as u32,
        frame_width_in_mbs_minus1: picture.pic_width_in_mbs_minus1,
        frame_height_in_mbs_minus1: picture.pic_height_in_mbs_minus1,
        image_structure: avc_mfx_image_structure(picture.picture_structure),
        weighted_bipred_idc: picture.weighted_bipred_idc,
        weighted_pred: picture.weighted_pred,
        first_chroma_qp_offset: picture.chroma_qp_index_offset,
        second_chroma_qp_offset: picture.second_chroma_qp_index_offset,
        field_pic: picture.field_pic,
        mb_adaptive_frame_field: picture.mb_adaptive_frame_field && !picture.field_pic,
        frame_mbs_only: picture.frame_mbs_only,
        transform_8x8: picture.transform_8x8,
        direct_8x8_inference: picture.direct_8x8_inference,
        constrained_intra_pred: picture.constrained_intra_pred,
        disposable: !picture.reference_pic,
        entropy_coding: picture.entropy_coding_mode,
        chroma_format_idc: avc_chroma_format_idc(picture.chroma_format),
        initial_qp_value: picture.pic_init_qp_minus26,
        active_ref_l0: picture.num_ref_idx_l0_active_minus1.saturating_add(1),
        active_ref_l1: picture.num_ref_idx_l1_active_minus1.saturating_add(1),
        reference_frames: plan.resources.reference_surface_count as u8,
        pic_order_present: picture.pic_order_present,
        delta_pic_order_always_zero: picture.delta_pic_order_always_zero,
        pic_order_cnt_type: picture.pic_order_cnt_type,
        slice_group_map_type: picture.slice_group_map_type,
        redundant_pic_cnt_present: picture.redundant_pic_cnt_present,
        num_slice_groups_minus1: picture.num_slice_groups_minus1,
        deblocking_filter_control_present: picture.deblocking_filter_control_present,
        log2_max_frame_num_minus4: picture.log2_max_frame_num_minus4,
        log2_max_pic_order_cnt_lsb_minus4: picture.log2_max_pic_order_cnt_lsb_minus4,
        slice_group_change_rate: picture.slice_group_change_rate_minus1,
        curr_pic_frame_num: picture.frame_num,
    }
}

pub(crate) const fn encode_mfx_avc_img_state(params: MfxAvcImgStateParams) -> MfxAvcImgStateDwords {
    let mut dwords = [0u32; MFX_AVC_IMG_STATE_DWORD_COUNT];
    dwords[0] = MFX_AVC_IMG_STATE_DW0;
    dwords[1] = params.frame_size_mbs & MFX_AVC_IMG_FRAME_SIZE_MAX;
    dwords[2] = ((params.frame_width_in_mbs_minus1 as u32) & MFX_AVC_IMG_DIMENSION_MAX)
        | (((params.frame_height_in_mbs_minus1 as u32) & MFX_AVC_IMG_DIMENSION_MAX) << 16);
    dwords[3] = ((params.image_structure as u32) << 8)
        | (((params.weighted_bipred_idc as u32) & 0x03) << 10)
        | (bool_bit(params.weighted_pred) << 12)
        | ((i5_twos(params.first_chroma_qp_offset) & 0x1f) << 16)
        | ((i5_twos(params.second_chroma_qp_offset) & 0x1f) << 24);
    dwords[4] = bool_bit(params.field_pic)
        | (bool_bit(params.mb_adaptive_frame_field) << 1)
        | (bool_bit(params.frame_mbs_only) << 2)
        | (bool_bit(params.transform_8x8) << 3)
        | (bool_bit(params.direct_8x8_inference) << 4)
        | (bool_bit(params.constrained_intra_pred) << 5)
        | (bool_bit(params.disposable) << 6)
        | (bool_bit(params.entropy_coding) << 7)
        | (((params.chroma_format_idc as u32) & 0x03) << 10);
    dwords[5] = MFX_AVC_IMG_STATE_DW5_DEFAULT;
    dwords[13] = (i8_twos(params.initial_qp_value) & 0xff)
        | (((params.active_ref_l0 as u32) & 0x3f) << 8)
        | (((params.active_ref_l1 as u32) & 0x3f) << 16)
        | (((params.reference_frames as u32) & 0x1f) << 24);
    dwords[14] = bool_bit(params.pic_order_present)
        | (bool_bit(params.delta_pic_order_always_zero) << 1)
        | (((params.pic_order_cnt_type as u32) & 0x03) << 2)
        | (((params.slice_group_map_type as u32) & 0x07) << 8)
        | (bool_bit(params.redundant_pic_cnt_present) << 11)
        | (((params.num_slice_groups_minus1 as u32) & 0x07) << 12)
        | (bool_bit(params.deblocking_filter_control_present) << 15)
        | (((params.log2_max_frame_num_minus4 as u32) & 0xff) << 16)
        | (((params.log2_max_pic_order_cnt_lsb_minus4 as u32) & 0xff) << 24);
    dwords[15] = ((params.slice_group_change_rate as u32) & 0xffff)
        | (((params.curr_pic_frame_num as u32) & 0xffff) << 16);
    MfxAvcImgStateDwords { dwords }
}

pub(crate) const fn mfx_qm_state_params_flat_avc_defaults() -> [MfxQmStateParams; AVC_QM_STATE_COUNT]
{
    let flat = [AVC_FLAT_QM_DWORD; 16];
    [
        MfxQmStateParams {
            qm_type: MFX_QM_AVC_4X4_INTRA,
            matrix: flat,
        },
        MfxQmStateParams {
            qm_type: MFX_QM_AVC_4X4_INTER,
            matrix: flat,
        },
        MfxQmStateParams {
            qm_type: MFX_QM_AVC_8X8_INTRA,
            matrix: flat,
        },
        MfxQmStateParams {
            qm_type: MFX_QM_AVC_8X8_INTER,
            matrix: flat,
        },
    ]
}

pub(crate) const fn encode_mfx_qm_state(params: MfxQmStateParams) -> MfxQmStateDwords {
    let mut dwords = [0u32; MFX_QM_STATE_DWORD_COUNT];
    dwords[0] = MFX_QM_STATE_DW0;
    dwords[1] = params.qm_type as u32;
    let mut idx = 0;
    while idx < 16 {
        dwords[2 + idx] = params.matrix[idx];
        idx += 1;
    }
    MfxQmStateDwords { dwords }
}

pub(crate) const fn mfx_avc_directmode_state_params_for_idr(
    plan: AvcLongFormatIdrPlan,
) -> MfxAvcDirectmodeStateParams {
    let mut poc_list = [0u32; 34];
    poc_list[MFX_AVC_DMV_DEST_TOP] = plan.picture.top_field_order_cnt as u32;
    poc_list[MFX_AVC_DMV_DEST_BOTTOM] = plan.picture.bottom_field_order_cnt as u32;
    MfxAvcDirectmodeStateParams {
        dmv_write_addr: 0,
        dmv_reference_addrs: [0; 16],
        poc_list,
    }
}

pub(crate) const fn encode_mfx_avc_directmode_state(
    params: MfxAvcDirectmodeStateParams,
    dmv_write_buffer: AvcGpuResourceRange,
    dmv_reference_buffer: AvcGpuResourceRange,
) -> MfxAvcDirectmodeStateDwords {
    let mut dwords = [0u32; MFX_AVC_DIRECTMODE_STATE_DWORD_COUNT];
    dwords[0] = MFX_AVC_DIRECTMODE_STATE_DW0;
    let mut ref_idx = 0;
    while ref_idx < 16 {
        let addr = if params.dmv_reference_addrs[ref_idx] != 0 {
            params.dmv_reference_addrs[ref_idx]
        } else {
            dmv_reference_buffer.gpu_addr
        };
        write_addr64(&mut dwords, 1 + ref_idx * 2, addr);
        ref_idx += 1;
    }
    dwords[33] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    let write_addr = if params.dmv_write_addr != 0 {
        params.dmv_write_addr
    } else {
        dmv_write_buffer.gpu_addr
    };
    write_addr64(&mut dwords, 34, write_addr);
    dwords[36] = mfx_memory_address_attributes_mocs(MFX_MEMORY_OBJECT_CONTROL_UC);
    let mut poc_idx = 0;
    while poc_idx < 34 {
        dwords[37 + poc_idx] = params.poc_list[poc_idx];
        poc_idx += 1;
    }
    MfxAvcDirectmodeStateDwords { dwords }
}

pub(crate) const fn mfx_avc_ref_idx_state_params_dummy_l0() -> MfxAvcRefIdxStateParams {
    MfxAvcRefIdxStateParams {
        ref_pic_list_select: 0,
        reference_list_entry: [0; 8],
    }
}

pub(crate) const fn encode_mfx_avc_ref_idx_state(
    params: MfxAvcRefIdxStateParams,
) -> MfxAvcRefIdxStateDwords {
    let mut dwords = [0u32; MFX_AVC_REF_IDX_STATE_DWORD_COUNT];
    dwords[0] = MFX_AVC_REF_IDX_STATE_DW0;
    dwords[1] = (params.ref_pic_list_select as u32) & 0x01;
    let mut idx = 0;
    while idx < 8 {
        dwords[2 + idx] = params.reference_list_entry[idx];
        idx += 1;
    }
    MfxAvcRefIdxStateDwords { dwords }
}

pub(crate) const fn mfx_avc_slice_state_params_for_single_idr(
    plan: AvcLongFormatIdrPlan,
) -> MfxAvcSliceStateParams {
    let picture = plan.picture;
    let slice = plan.slice;
    let width_in_mbs = picture.pic_width_in_mbs() as u32;
    let frame_height_in_mbs = picture.pic_height_in_mbs() as u32;
    let first_mb = slice.first_mb_in_slice;
    let next_mb = slice.first_mb_in_next_slice;
    let slice_qp = (26 + picture.pic_init_qp_minus26 as i32 + slice.slice_qp_delta as i32) as u8;
    MfxAvcSliceStateParams {
        slice_type: avc_mfx_slice_type(slice.class),
        log2_weight_denom_luma: 0,
        log2_weight_denom_chroma: 0,
        number_of_ref_pictures_l0: 0,
        number_of_ref_pictures_l1: 0,
        slice_alpha_c0_offset_div2: slice.slice_alpha_c0_offset_div2,
        slice_beta_offset_div2: slice.slice_beta_offset_div2,
        slice_quantization_parameter: slice_qp,
        cabac_init_idc: 0,
        disable_deblocking_filter_indicator: slice.disable_deblocking_filter_idc,
        direct_prediction_type: 0,
        weighted_prediction_indicator: 0,
        slice_start_mb_num: first_mb,
        slice_horizontal_position: first_mb % width_in_mbs,
        slice_vertical_position: first_mb / width_in_mbs,
        next_slice_horizontal_position: next_mb % width_in_mbs,
        next_slice_vertical_position: if next_mb as usize == picture.macroblock_count() {
            frame_height_in_mbs
        } else {
            next_mb / width_in_mbs
        },
        slice_id: 0,
        cabac_zero_word_insertion_enable: false,
        emulation_byte_slice_insert_enable: false,
        tail_insertion_present_in_bitstream: false,
        slice_data_insertion_present_in_bitstream: false,
        header_insertion_present_in_bitstream: false,
        is_last_slice: true,
        round_intra: 5,
        round_inter: 2,
        round_inter_enable: false,
    }
}

pub(crate) const fn encode_mfx_avc_slice_state(
    params: MfxAvcSliceStateParams,
) -> MfxAvcSliceStateDwords {
    let mut dwords = [0u32; MFX_AVC_SLICE_STATE_DWORD_COUNT];
    dwords[0] = MFX_AVC_SLICE_STATE_DW0;
    dwords[1] = (params.slice_type as u32) & 0x0f;
    dwords[2] = ((params.log2_weight_denom_luma as u32) & 0x07)
        | (((params.log2_weight_denom_chroma as u32) & 0x07) << 8)
        | (((params.number_of_ref_pictures_l0 as u32) & 0x3f) << 16)
        | (((params.number_of_ref_pictures_l1 as u32) & 0x3f) << 24);
    dwords[3] = (i4_twos(params.slice_alpha_c0_offset_div2) & 0x0f)
        | ((i4_twos(params.slice_beta_offset_div2) & 0x0f) << 8)
        | (((params.slice_quantization_parameter as u32) & 0x3f) << 16)
        | (((params.cabac_init_idc as u32) & 0x03) << 24)
        | (((params.disable_deblocking_filter_indicator as u32) & 0x03) << 27)
        | (((params.direct_prediction_type as u32) & 0x01) << 29)
        | (((params.weighted_prediction_indicator as u32) & 0x03) << 30);
    dwords[4] = (params.slice_start_mb_num & 0x7fff)
        | ((params.slice_horizontal_position & MFX_AVC_SLICE_POSITION_MAX) << 16)
        | ((params.slice_vertical_position & MFX_AVC_SLICE_POSITION_MAX) << 24);
    dwords[5] = (params.next_slice_horizontal_position & MFX_AVC_SLICE_NEXT_POSITION_MAX)
        | ((params.next_slice_vertical_position & MFX_AVC_SLICE_NEXT_POSITION_MAX) << 16);
    dwords[6] = (((params.slice_id as u32) & 0x0f) << 4)
        | (bool_bit(params.cabac_zero_word_insertion_enable) << 12)
        | (bool_bit(params.emulation_byte_slice_insert_enable) << 13)
        | (bool_bit(params.tail_insertion_present_in_bitstream) << 15)
        | (bool_bit(params.slice_data_insertion_present_in_bitstream) << 16)
        | (bool_bit(params.header_insertion_present_in_bitstream) << 17)
        | (bool_bit(params.is_last_slice) << 19);
    dwords[9] = (((params.round_intra as u32) & 0x07) << 24)
        | (1 << 27)
        | (((params.round_inter as u32) & 0x07) << 28)
        | (bool_bit(params.round_inter_enable) << 31);
    MfxAvcSliceStateDwords { dwords }
}

pub(crate) const fn mfd_avc_bsd_object_params_for_single_idr(
    plan: AvcLongFormatIdrPlan,
) -> MfdAvcBsdObjectParams {
    let slice = plan.slice;
    let slice_record = avc_long_format_slice_record(slice);
    MfdAvcBsdObjectParams {
        indirect_bsd_data_length: slice_record.length + slice_record.offset,
        indirect_bsd_data_start_address: slice.slice_data_offset,
        first_mb_byte_offset_of_slice_data_or_slice_header: slice_record.offset,
        first_macroblock_mb_bit_offset: avc_long_format_first_mb_bit_offset(
            slice.slice_data_bit_offset_from_payload,
        ),
        last_slice: true,
        fix_prev_mb_skipped: true,
        intra_predmode_4x4_8x8_luma_error_control: true,
        intra_prediction_error_control: true,
        intra_8x8_4x4_prediction_error_concealment: true,
        i_slice_concealment_mode: true,
    }
}

pub(crate) const fn encode_mfd_avc_bsd_object(
    params: MfdAvcBsdObjectParams,
) -> MfdAvcBsdObjectDwords {
    let mut dwords = [0u32; MFD_AVC_BSD_OBJECT_DWORD_COUNT];
    dwords[0] = MFD_AVC_BSD_OBJECT_DW0;
    dwords[1] = params.indirect_bsd_data_length;
    dwords[2] = params.indirect_bsd_data_start_address & 0x1fff_ffff;
    dwords[3] = bool_bit(params.intra_predmode_4x4_8x8_luma_error_control) << 29;
    dwords[4] = ((params.first_macroblock_mb_bit_offset as u32) & 0x07)
        | (bool_bit(params.last_slice) << 3)
        | (bool_bit(params.fix_prev_mb_skipped) << 7)
        | ((params.first_mb_byte_offset_of_slice_data_or_slice_header & 0xffff) << 16);
    dwords[5] = bool_bit(params.intra_prediction_error_control)
        | (bool_bit(params.intra_8x8_4x4_prediction_error_concealment) << 1)
        | (bool_bit(params.i_slice_concealment_mode) << 31);
    MfdAvcBsdObjectDwords { dwords }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct AvcRowstoreScratchBytes {
    pub deblocking_filter: usize,
    pub bsd_mpc: usize,
    pub intra: usize,
    pub mpr: usize,
}

pub(crate) const AVC_CACHELINE_BYTES: usize = 64;
pub(crate) const AVC_DMV_BYTES_PER_MB: usize = 64;

pub(crate) const fn avc_rowstore_scratch_bytes(pic_width_in_mbs: usize) -> AvcRowstoreScratchBytes {
    AvcRowstoreScratchBytes {
        deblocking_filter: pic_width_in_mbs * 4 * AVC_CACHELINE_BYTES,
        bsd_mpc: pic_width_in_mbs * 2 * AVC_CACHELINE_BYTES,
        intra: pic_width_in_mbs * AVC_CACHELINE_BYTES,
        mpr: pic_width_in_mbs * 2 * AVC_CACHELINE_BYTES,
    }
}

pub(crate) const fn avc_dmv_buffer_bytes(
    pic_width_in_mbs: usize,
    pic_height_in_mbs: usize,
) -> usize {
    pic_width_in_mbs * align_ceil_usize(pic_height_in_mbs, 2) * AVC_DMV_BYTES_PER_MB
}

pub(crate) const fn avc_dmv_buffer_bytes_for_picture(picture: AvcPictureParams) -> usize {
    avc_dmv_buffer_bytes(picture.pic_width_in_mbs(), picture.pic_height_in_mbs())
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct AnnexBNal {
    payload_start: usize,
    payload_end: usize,
}

struct AnnexBNalScanner<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> AnnexBNalScanner<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn next(&mut self) -> Option<AnnexBNal> {
        let (start_code, start_code_len) = find_start_code(self.bytes, self.offset)?;
        let payload_start = start_code + start_code_len;
        let payload_end = find_start_code(self.bytes, payload_start)
            .map(|(next, _)| next)
            .unwrap_or(self.bytes.len());
        self.offset = payload_end;
        Some(AnnexBNal {
            payload_start,
            payload_end,
        })
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ParsedSps {
    seq_parameter_set_id: u32,
    pic_width_in_mbs_minus1: u16,
    pic_height_in_mbs_minus1: u16,
    coded_width: u32,
    coded_height: u32,
    visible_width: u32,
    visible_height: u32,
    log2_max_frame_num_minus4: u8,
    pic_order_cnt_type: u8,
    log2_max_pic_order_cnt_lsb_minus4: u8,
    delta_pic_order_always_zero: bool,
    frame_mbs_only: bool,
    mb_adaptive_frame_field: bool,
    direct_8x8_inference: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ParsedPps {
    seq_parameter_set_id: u32,
    entropy_coding_mode: bool,
    pic_order_present: bool,
    num_slice_groups_minus1: u8,
    slice_group_map_type: u8,
    slice_group_change_rate_minus1: u16,
    num_ref_idx_l0_default_active_minus1: u8,
    num_ref_idx_l1_default_active_minus1: u8,
    weighted_pred: bool,
    weighted_bipred_idc: u8,
    pic_init_qp_minus26: i8,
    deblocking_filter_control_present: bool,
    constrained_intra_pred: bool,
    redundant_pic_cnt_present: bool,
    transform_8x8: bool,
    chroma_qp_index_offset: i8,
    second_chroma_qp_index_offset: i8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ParsedSlice {
    class: AvcSliceClass,
    first_mb_in_slice: u32,
    frame_num: u16,
    first_mb_bit_offset_from_payload: u32,
    disable_deblocking_filter_idc: u8,
    slice_alpha_c0_offset_div2: i8,
    slice_beta_offset_div2: i8,
    slice_qp_delta: i8,
    top_field_order_cnt: i32,
    bottom_field_order_cnt: i32,
}

fn parse_sps(payload: &[u8]) -> Result<ParsedSps, AvcAnnexBPlanError> {
    let rbsp = rbsp_from_ebsp(payload);
    let mut br = H264BitReader::new(&rbsp);
    let profile_idc = br.read_bits(8)? as u8;
    let _constraint_flags = br.read_bits(8)?;
    let _level_idc = br.read_bits(8)?;
    let seq_parameter_set_id = br.read_ue()?;
    if !matches!(profile_idc, 66 | 77 | 88) {
        return Err(AvcAnnexBPlanError::UnsupportedSps);
    }
    let log2_max_frame_num_minus4 = checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSps)?;
    let pic_order_cnt_type = checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSps)?;
    let mut log2_max_pic_order_cnt_lsb_minus4 = 0;
    let mut delta_pic_order_always_zero = false;
    if pic_order_cnt_type == 0 {
        log2_max_pic_order_cnt_lsb_minus4 =
            checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSps)?;
    } else if pic_order_cnt_type == 1 {
        delta_pic_order_always_zero = br.read_bool()?;
        let _offset_for_non_ref_pic = br.read_se()?;
        let _offset_for_top_to_bottom_field = br.read_se()?;
        let num_ref_frames_in_pic_order_cnt_cycle = br.read_ue()?;
        if num_ref_frames_in_pic_order_cnt_cycle > 16 {
            return Err(AvcAnnexBPlanError::UnsupportedSps);
        }
        for _ in 0..num_ref_frames_in_pic_order_cnt_cycle {
            let _ = br.read_se()?;
        }
    } else if pic_order_cnt_type > 2 {
        return Err(AvcAnnexBPlanError::UnsupportedSps);
    }
    let _max_num_ref_frames = br.read_ue()?;
    let _gaps_in_frame_num_value_allowed = br.read_bool()?;
    let pic_width_in_mbs_minus1 = checked_u16(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSps)?;
    let pic_height_in_map_units_minus1 =
        checked_u16(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSps)?;
    let frame_mbs_only = br.read_bool()?;
    let mut mb_adaptive_frame_field = false;
    if !frame_mbs_only {
        mb_adaptive_frame_field = br.read_bool()?;
    }
    let direct_8x8_inference = br.read_bool()?;
    let frame_cropping_flag = br.read_bool()?;
    let mut frame_crop_left_offset = 0u32;
    let mut frame_crop_right_offset = 0u32;
    let mut frame_crop_top_offset = 0u32;
    let mut frame_crop_bottom_offset = 0u32;
    if frame_cropping_flag {
        frame_crop_left_offset = br.read_ue()?;
        frame_crop_right_offset = br.read_ue()?;
        frame_crop_top_offset = br.read_ue()?;
        frame_crop_bottom_offset = br.read_ue()?;
    }
    let frame_height_multiplier = if frame_mbs_only { 1 } else { 2 };
    let pic_height_in_mbs =
        frame_height_multiplier * (usize::from(pic_height_in_map_units_minus1) + 1);
    let pic_height_in_mbs_minus1 =
        checked_u16((pic_height_in_mbs - 1) as u32, AvcAnnexBPlanError::UnsupportedSps)?;
    let coded_width = (u32::from(pic_width_in_mbs_minus1) + 1) * 16;
    let coded_height = (u32::from(pic_height_in_mbs_minus1) + 1) * 16;
    let crop_unit_x = 2u32;
    let crop_unit_y = if frame_mbs_only { 2u32 } else { 4u32 };
    let crop_x = frame_crop_left_offset
        .checked_add(frame_crop_right_offset)
        .and_then(|v| v.checked_mul(crop_unit_x))
        .ok_or(AvcAnnexBPlanError::UnsupportedSps)?;
    let crop_y = frame_crop_top_offset
        .checked_add(frame_crop_bottom_offset)
        .and_then(|v| v.checked_mul(crop_unit_y))
        .ok_or(AvcAnnexBPlanError::UnsupportedSps)?;
    if crop_x >= coded_width || crop_y >= coded_height {
        return Err(AvcAnnexBPlanError::UnsupportedSps);
    }
    Ok(ParsedSps {
        seq_parameter_set_id,
        pic_width_in_mbs_minus1,
        pic_height_in_mbs_minus1,
        coded_width,
        coded_height,
        visible_width: coded_width - crop_x,
        visible_height: coded_height - crop_y,
        log2_max_frame_num_minus4,
        pic_order_cnt_type,
        log2_max_pic_order_cnt_lsb_minus4,
        delta_pic_order_always_zero,
        frame_mbs_only,
        mb_adaptive_frame_field,
        direct_8x8_inference,
    })
}

fn parse_pps(payload: &[u8]) -> Result<ParsedPps, AvcAnnexBPlanError> {
    let rbsp = rbsp_from_ebsp(payload);
    let mut br = H264BitReader::new(&rbsp);
    let _pic_parameter_set_id = br.read_ue()?;
    let seq_parameter_set_id = br.read_ue()?;
    let entropy_coding_mode = br.read_bool()?;
    let pic_order_present = br.read_bool()?;
    let num_slice_groups_minus1 = checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedPps)?;
    let mut slice_group_map_type = 0;
    let mut slice_group_change_rate_minus1 = 0;
    if num_slice_groups_minus1 != 0 {
        slice_group_map_type = checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedPps)?;
        if slice_group_map_type == 3 || slice_group_map_type == 4 || slice_group_map_type == 5 {
            let _slice_group_change_direction_flag = br.read_bool()?;
            slice_group_change_rate_minus1 =
                checked_u16(br.read_ue()?, AvcAnnexBPlanError::UnsupportedPps)?;
        } else {
            return Err(AvcAnnexBPlanError::UnsupportedPps);
        }
    }
    let num_ref_idx_l0_default_active_minus1 =
        checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedPps)?;
    let num_ref_idx_l1_default_active_minus1 =
        checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedPps)?;
    let weighted_pred = br.read_bool()?;
    let weighted_bipred_idc = br.read_bits(2)? as u8;
    let pic_init_qp_minus26 = checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedPps)?;
    let _pic_init_qs_minus26 = br.read_se()?;
    let chroma_qp_index_offset = checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedPps)?;
    let deblocking_filter_control_present = br.read_bool()?;
    let constrained_intra_pred = br.read_bool()?;
    let redundant_pic_cnt_present = br.read_bool()?;
    let mut transform_8x8 = false;
    let mut second_chroma_qp_index_offset = chroma_qp_index_offset;
    if br.has_more_rbsp_data() {
        transform_8x8 = br.read_bool()?;
        let pic_scaling_matrix_present = br.read_bool()?;
        if pic_scaling_matrix_present {
            return Err(AvcAnnexBPlanError::UnsupportedPps);
        }
        second_chroma_qp_index_offset =
            checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedPps)?;
    }
    Ok(ParsedPps {
        seq_parameter_set_id,
        entropy_coding_mode,
        pic_order_present,
        num_slice_groups_minus1,
        slice_group_map_type,
        slice_group_change_rate_minus1,
        num_ref_idx_l0_default_active_minus1,
        num_ref_idx_l1_default_active_minus1,
        weighted_pred,
        weighted_bipred_idc,
        pic_init_qp_minus26,
        deblocking_filter_control_present,
        constrained_intra_pred,
        redundant_pic_cnt_present,
        transform_8x8,
        chroma_qp_index_offset,
        second_chroma_qp_index_offset,
    })
}

fn parse_idr_slice(
    payload: &[u8],
    sps: &ParsedSps,
    pps: &ParsedPps,
) -> Result<ParsedSlice, AvcAnnexBPlanError> {
    let rbsp = rbsp_from_ebsp(payload);
    let mut br = H264BitReader::new(&rbsp);
    let first_mb_in_slice = br.read_ue()?;
    let raw_slice_type = br.read_ue()?;
    let class = avc_slice_class_from_raw(raw_slice_type);
    if !class.is_i_only() {
        return Err(AvcAnnexBPlanError::UnsupportedSlice);
    }
    let _pic_parameter_set_id = br.read_ue()?;
    let frame_num = br.read_bits(usize::from(sps.log2_max_frame_num_minus4) + 4)? as u16;
    if !sps.frame_mbs_only {
        return Err(AvcAnnexBPlanError::UnsupportedSlice);
    }
    let _idr_pic_id = br.read_ue()?;
    let mut top_field_order_cnt = 0i32;
    let mut bottom_field_order_cnt = 0i32;
    if sps.pic_order_cnt_type == 0 {
        let pic_order_cnt_lsb =
            br.read_bits(usize::from(sps.log2_max_pic_order_cnt_lsb_minus4) + 4)? as i32;
        let mut delta_pic_order_cnt_bottom = 0i32;
        if pps.pic_order_present {
            delta_pic_order_cnt_bottom = br.read_se()?;
        }
        top_field_order_cnt = pic_order_cnt_lsb;
        bottom_field_order_cnt = pic_order_cnt_lsb + delta_pic_order_cnt_bottom;
    } else if sps.pic_order_cnt_type == 1 && !sps.delta_pic_order_always_zero {
        let _delta_pic_order_cnt_0 = br.read_se()?;
        if pps.pic_order_present {
            let _delta_pic_order_cnt_1 = br.read_se()?;
        }
    }
    if pps.redundant_pic_cnt_present {
        let _redundant_pic_cnt = br.read_ue()?;
    }
    let _no_output_of_prior_pics_flag = br.read_bool()?;
    let _long_term_reference_flag = br.read_bool()?;
    let slice_qp_delta = checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedSlice)?;
    let mut disable_deblocking_filter_idc = 0;
    let mut slice_alpha_c0_offset_div2 = 0;
    let mut slice_beta_offset_div2 = 0;
    if pps.deblocking_filter_control_present {
        disable_deblocking_filter_idc =
            checked_u8(br.read_ue()?, AvcAnnexBPlanError::UnsupportedSlice)?;
        if disable_deblocking_filter_idc != 1 {
            slice_alpha_c0_offset_div2 =
                checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedSlice)?;
            slice_beta_offset_div2 =
                checked_i8(br.read_se()?, AvcAnnexBPlanError::UnsupportedSlice)?;
        }
    }
    let first_mb_rbsp_bit_offset_from_payload = br.bit_pos();
    let first_mb_ebsp_bit_offset_from_payload =
        ebsp_bit_pos_from_rbsp_bit_pos(payload, first_mb_rbsp_bit_offset_from_payload)
            .ok_or(AvcAnnexBPlanError::InvalidBitstream)?;
    Ok(ParsedSlice {
        class,
        first_mb_in_slice,
        frame_num,
        first_mb_bit_offset_from_payload: first_mb_ebsp_bit_offset_from_payload as u32,
        disable_deblocking_filter_idc,
        slice_alpha_c0_offset_div2,
        slice_beta_offset_div2,
        slice_qp_delta,
        top_field_order_cnt,
        bottom_field_order_cnt,
    })
}

const fn mfx_pipe_mode_select_dw1(params: MfxPipeModeSelectParams) -> u32 {
    (params.standard_select & 0x0f)
        | (((params.codec_select as u32) & 0x01) << 4)
        | (bool_bit(params.pre_deblocking_output_enable) << 8)
        | (bool_bit(params.post_deblocking_output_enable) << 9)
        | (bool_bit(params.stream_out_enable) << 10)
        | (bool_bit(params.deblocker_stream_out_enable) << 12)
        | (((params.decoder_mode_select as u32) & 0x03) << 15)
        | (((params.decoder_short_format_mode as u32) & 0x01) << 17)
}

const fn bool_bit(value: bool) -> u32 {
    if value { 1 } else { 0 }
}

const fn avc_mfx_image_structure(picture_structure: AvcPictureStructure) -> u8 {
    match picture_structure {
        AvcPictureStructure::Frame => MFX_AVC_IMG_STRUCTURE_FRAME,
        AvcPictureStructure::TopField => MFX_AVC_IMG_STRUCTURE_TOP_FIELD,
        AvcPictureStructure::BottomField => MFX_AVC_IMG_STRUCTURE_BOTTOM_FIELD,
    }
}

const fn avc_chroma_format_idc(chroma_format: AvcChromaFormat) -> u8 {
    match chroma_format {
        AvcChromaFormat::Monochrome => 0,
        AvcChromaFormat::Yuv420 => 1,
        AvcChromaFormat::Yuv422 => 2,
        AvcChromaFormat::Yuv444 => 3,
    }
}

const fn avc_mfx_slice_type(slice_class: AvcSliceClass) -> u8 {
    match slice_class {
        AvcSliceClass::P | AvcSliceClass::Sp => MFX_AVC_SLICE_TYPE_P,
        AvcSliceClass::B => MFX_AVC_SLICE_TYPE_B,
        AvcSliceClass::I | AvcSliceClass::Si | AvcSliceClass::Unknown => MFX_AVC_SLICE_TYPE_I,
    }
}

const fn avc_slice_class_from_raw(raw_slice_type: u32) -> AvcSliceClass {
    match raw_slice_type % 5 {
        0 => AvcSliceClass::P,
        1 => AvcSliceClass::B,
        2 => AvcSliceClass::I,
        3 => AvcSliceClass::Sp,
        4 => AvcSliceClass::Si,
        _ => AvcSliceClass::Unknown,
    }
}

const fn i4_twos(value: i8) -> u32 {
    (value as i32 as u32) & 0x0f
}

const fn i5_twos(value: i8) -> u32 {
    (value as i32 as u32) & 0x1f
}

const fn i8_twos(value: i8) -> u32 {
    (value as i32 as u32) & 0xff
}

const fn pack_u16x2(lo: u16, hi: u16) -> u32 {
    (lo as u32) | ((hi as u32) << 16)
}

const fn write_addr64<const N: usize>(dwords: &mut [u32; N], dword_index: usize, gpu_addr: u64) {
    dwords[dword_index] = gpu_addr as u32;
    dwords[dword_index + 1] = (gpu_addr >> 32) as u32;
}

const fn mfx_bsp_rowstore_mocs_attr(mocs: u32) -> u32 {
    (mocs & 0x3f) << 1
}

const fn mfx_memory_address_attributes_mocs(mocs: u32) -> u32 {
    (mocs & 0x3f) << 1
}

fn validate_resource_range(
    range: AvcGpuResourceRange,
    required_bytes: usize,
    name: &'static str,
) -> Result<(), AvcCommandStreamBlocker> {
    if range.gpu_addr & (MFX_GENERAL_STATE_ALIGNMENT - 1) != 0 {
        return Err(AvcCommandStreamBlocker::ResourceUnaligned(name));
    }
    if range.bytes < required_bytes {
        return Err(AvcCommandStreamBlocker::ResourceTooSmall(name));
    }
    Ok(())
}

struct H264BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

impl<'a> H264BitReader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    const fn bit_pos(&self) -> usize {
        self.bit_pos
    }

    fn bits_remaining(&self) -> usize {
        self.bytes
            .len()
            .saturating_mul(8)
            .saturating_sub(self.bit_pos)
    }

    fn read_bool(&mut self) -> Result<bool, AvcAnnexBPlanError> {
        Ok(self.read_bits(1)? != 0)
    }

    fn read_bits(&mut self, count: usize) -> Result<u32, AvcAnnexBPlanError> {
        if count > 32 || self.bits_remaining() < count {
            return Err(AvcAnnexBPlanError::InvalidBitstream);
        }
        let mut value = 0u32;
        for _ in 0..count {
            let byte = self.bytes[self.bit_pos / 8];
            let bit = (byte >> (7 - (self.bit_pos % 8))) & 1;
            value = (value << 1) | u32::from(bit);
            self.bit_pos += 1;
        }
        Ok(value)
    }

    fn read_ue(&mut self) -> Result<u32, AvcAnnexBPlanError> {
        let mut leading_zero_bits = 0usize;
        while self.bits_remaining() > 0 && !self.read_bool()? {
            leading_zero_bits += 1;
            if leading_zero_bits > 31 {
                return Err(AvcAnnexBPlanError::InvalidBitstream);
            }
        }
        let suffix = if leading_zero_bits == 0 {
            0
        } else {
            self.read_bits(leading_zero_bits)?
        };
        Ok((1u32 << leading_zero_bits) - 1 + suffix)
    }

    fn read_se(&mut self) -> Result<i32, AvcAnnexBPlanError> {
        let code_num = self.read_ue()? as i32;
        let magnitude = (code_num + 1) / 2;
        if code_num & 1 == 0 {
            Ok(-magnitude)
        } else {
            Ok(magnitude)
        }
    }

    fn has_more_rbsp_data(&self) -> bool {
        let Some(last_one_bit) = last_one_bit_pos(self.bytes) else {
            return false;
        };
        self.bit_pos < last_one_bit
    }
}

fn find_start_code(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i + 3 <= bytes.len() {
        if i + 4 <= bytes.len()
            && bytes[i] == 0
            && bytes[i + 1] == 0
            && bytes[i + 2] == 0
            && bytes[i + 3] == 1
        {
            return Some((i, 4));
        }
        if bytes[i] == 0 && bytes[i + 1] == 0 && bytes[i + 2] == 1 {
            return Some((i, 3));
        }
        i += 1;
    }
    None
}

fn rbsp_from_ebsp(ebsp: &[u8]) -> Vec<u8> {
    let mut rbsp = Vec::with_capacity(ebsp.len());
    let mut zero_run = 0u8;
    for &byte in ebsp {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        rbsp.push(byte);
        if byte == 0 {
            zero_run = zero_run.saturating_add(1);
        } else {
            zero_run = 0;
        }
    }
    rbsp
}

fn ebsp_bit_pos_from_rbsp_bit_pos(ebsp: &[u8], rbsp_bit_pos: usize) -> Option<usize> {
    let target_rbsp_byte = rbsp_bit_pos / 8;
    let target_bit = rbsp_bit_pos % 8;
    let mut rbsp_byte = 0usize;
    let mut zero_run = 0u8;
    for (ebsp_byte, &byte) in ebsp.iter().enumerate() {
        if zero_run >= 2 && byte == 0x03 {
            zero_run = 0;
            continue;
        }
        if rbsp_byte == target_rbsp_byte {
            return Some(ebsp_byte * 8 + target_bit);
        }
        rbsp_byte += 1;
        if byte == 0 {
            zero_run = zero_run.saturating_add(1);
        } else {
            zero_run = 0;
        }
    }
    if rbsp_bit_pos == rbsp_byte * 8 {
        Some(ebsp.len() * 8)
    } else {
        None
    }
}

fn last_one_bit_pos(bytes: &[u8]) -> Option<usize> {
    let mut bit_pos = bytes.len().saturating_mul(8);
    while bit_pos > 0 {
        bit_pos -= 1;
        let byte = bytes[bit_pos / 8];
        let bit = (byte >> (7 - (bit_pos % 8))) & 1;
        if bit != 0 {
            return Some(bit_pos);
        }
    }
    None
}

fn checked_u8(value: u32, err: AvcAnnexBPlanError) -> Result<u8, AvcAnnexBPlanError> {
    if value > u8::MAX as u32 {
        Err(err)
    } else {
        Ok(value as u8)
    }
}

fn checked_u16(value: u32, err: AvcAnnexBPlanError) -> Result<u16, AvcAnnexBPlanError> {
    if value > u16::MAX as u32 {
        Err(err)
    } else {
        Ok(value as u16)
    }
}

fn checked_i8(value: i32, err: AvcAnnexBPlanError) -> Result<i8, AvcAnnexBPlanError> {
    if value < i8::MIN as i32 || value > i8::MAX as i32 {
        Err(err)
    } else {
        Ok(value as i8)
    }
}

const fn align_ceil_usize(value: usize, alignment: usize) -> usize {
    if alignment == 0 {
        value
    } else {
        value.div_ceil(alignment) * alignment
    }
}

const fn align_ceil_u64(value: u64, alignment: u64) -> u64 {
    if alignment == 0 {
        value
    } else {
        value.div_ceil(alignment) * alignment
    }
}
