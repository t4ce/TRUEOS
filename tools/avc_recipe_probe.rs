extern crate alloc;

#[path = "../src/intel/xelp_media_avc_decode_recipe.rs"]
mod xelp_media_avc_decode_recipe;

use xelp_media_avc_decode_recipe::*;

fn range(gpu_addr: u64, bytes: usize) -> AvcGpuResourceRange {
    AvcGpuResourceRange { gpu_addr, bytes }
}

fn align_up(value: usize, align: usize) -> usize {
    (value + align - 1) & !(align - 1)
}

fn bit(value: bool) -> u32 {
    u32::from(value)
}

fn addr_lo(dwords: &[u32], offset: usize) -> u32 {
    dwords[offset]
}

fn addr_hi(dwords: &[u32], offset: usize) -> u32 {
    dwords[offset + 1]
}

fn main() {
    let bytes = include_bytes!("vid/x31_head_movie_first_frame.h264");
    let plan = parse_annexb_single_idr_plan(bytes).expect("parse x31 first frame");
    validate_long_format_single_idr(plan).expect("validate x31 first frame plan");

    let base = 0x1_0000_0000u64;
    let bitstream_base = base + 0x0100_0000;
    let bitstream_window_bytes = 8 * 1024 * 1024;
    let bitstream_upper_bound =
        bitstream_base + align_up(plan.bitstream_bytes, MFX_INDIRECT_OBJECT_BASE_ALIGNMENT as usize) as u64;
    let missing_reference =
        base + avc_missing_reference_surface_offset(plan.resources.dest_surface.byte_len) as u64;
    let scratch = base + 0x0200_0000;
    let align = MFX_GENERAL_STATE_ALIGNMENT as usize;
    let intra = scratch;
    let deblock = intra + align_up(plan.resources.rowstore.intra, align) as u64;
    let bsd_mpc = deblock + align_up(plan.resources.rowstore.deblocking_filter, align) as u64;
    let mpr = bsd_mpc + align_up(plan.resources.rowstore.bsd_mpc, align) as u64;
    let dmv_write = mpr + align_up(plan.resources.rowstore.mpr, align) as u64;
    let dmv_reference =
        dmv_write + align_up(plan.resources.dmv_write_buffer_bytes, align) as u64;
    let bindings = AvcPacketResourceBindings {
        dest_surface: range(base, plan.resources.dest_surface.byte_len),
        missing_reference_surface: range(
            missing_reference,
            plan.resources.dest_surface.byte_len,
        ),
        bitstream: range(bitstream_base, bitstream_window_bytes),
        intra_rowstore: range(intra, plan.resources.rowstore.intra),
        deblocking_filter_rowstore: range(deblock, plan.resources.rowstore.deblocking_filter),
        bsd_mpc_rowstore: range(bsd_mpc, plan.resources.rowstore.bsd_mpc),
        mpr_rowstore: range(mpr, plan.resources.rowstore.mpr),
        dmv_write_buffer: range(dmv_write, plan.resources.dmv_write_buffer_bytes),
        dmv_reference_buffer: range(
            dmv_reference,
            plan.resources.dmv_reference_buffer_bytes,
        ),
    };
    let stream =
        build_long_format_single_idr_command_stream(plan, bindings).expect("build command stream");
    assert!(validate_long_format_single_idr_command_stream_shape(&stream));
    assert!(validate_long_format_single_idr_command_blocks());
    assert_eq!(plan.picture.pic_width_in_mbs(), 120);
    assert_eq!(plan.picture.pic_height_in_mbs(), 68);
    assert!(plan.picture.macroblock_count() <= MFX_AVC_IMG_FRAME_SIZE_MAX as usize);
    assert!(u32::from(plan.picture.pic_width_in_mbs_minus1) <= MFX_AVC_IMG_DIMENSION_MAX);
    assert!(u32::from(plan.picture.pic_height_in_mbs_minus1) <= MFX_AVC_IMG_DIMENSION_MAX);
    assert_eq!(plan.picture.coded_width(), 1920);
    assert_eq!(plan.picture.coded_height(), 1088);
    assert_eq!(plan.resources.dest_surface.pitch_bytes, 2048);
    assert_eq!(plan.resources.dest_surface.uv_offset, 2048 * 1280);
    assert_eq!(plan.resources.dest_surface.byte_len, 2048 * 2048);
    assert_eq!(plan.slice.first_mb_in_slice, 0);
    assert_eq!(plan.slice.first_mb_in_next_slice, 120 * 68);
    assert_eq!(plan.resources.reference_surface_count, 0);
    assert_eq!(bytes[plan.slice.slice_data_offset as usize] & 0x1f, 5);
    assert_eq!(plan.slice.slice_data_bit_offset_from_payload, 26);
    assert_eq!(
        plan.slice.first_mb_byte_offset,
        avc_long_format_first_mb_byte_offset(plan.slice.slice_data_bit_offset_from_payload)
    );
    assert_eq!(
        plan.slice.slice_data_bit_offset,
        avc_long_format_first_mb_bit_offset(plan.slice.slice_data_bit_offset_from_payload)
    );
    assert!(plan.slice.first_mb_byte_offset >= 1);
    let slice_record = avc_long_format_slice_record(plan.slice);
    assert_eq!(slice_record.offset, plan.slice.first_mb_byte_offset);
    assert_eq!(
        slice_record.length + slice_record.offset,
        plan.slice.slice_data_size
    );
    assert_eq!(stream.dwords.len(), AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_DWORDS);
    assert_eq!(stream.command_count, AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT);
    assert_eq!(
        AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_BLOCKS.len(),
        AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_COUNT
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_FORCE_WAKEUP], MI_FORCE_WAKEUP_DW0);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_WAIT_BEFORE_PIPE_MODE],
        MFX_WAIT_SYNC_DW0
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_WAIT_AFTER_PIPE_MODE],
        MFX_WAIT_SYNC_DW0
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_MODE],
        MFX_PIPE_MODE_SELECT_DW0
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_PIPE_MODE + 1], 0x0002_0202);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 3] & 0x03,
        MFX_TILEMODE_TILEYS_64K
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 2],
        ((1920 - 1) << 4) | ((1088 - 1) << 18)
    );
    assert_eq!(
        (stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 3] >> 3) & 0x1_ffff,
        2047
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 4], 1280);
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_SURFACE_STATE + 5], 1280);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 1],
        bitstream_base as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 2],
        (bitstream_base >> 32) as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 4],
        bitstream_upper_bound as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 5],
        (bitstream_upper_bound >> 32) as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_IND_OBJ_BASE_ADDR_STATE + 3],
        MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 1],
        0
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 2], 0);
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 3], 0);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 4],
        base as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 5],
        (base >> 32) as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 6],
        MFX_MEMORY_OBJECT_CONTROL_UC
    );
    assert_eq!(
        addr_lo(&stream.dwords, AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 13),
        intra as u32
    );
    assert_eq!(
        addr_hi(&stream.dwords, AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 13),
        (intra >> 32) as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 15],
        MFX_MEMORY_OBJECT_CONTROL_UC
    );
    assert_eq!(
        addr_lo(&stream.dwords, AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 16),
        deblock as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 18],
        MFX_MEMORY_OBJECT_CONTROL_UC
    );
    for ref_idx in 0..16 {
        let ref_offset = AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 19 + ref_idx * 2;
        assert_eq!(addr_lo(&stream.dwords, ref_offset), missing_reference as u32);
        assert_eq!(
            addr_hi(&stream.dwords, ref_offset),
            (missing_reference >> 32) as u32
        );
    }
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_PIPE_BUF_ADDR_STATE + 51],
        MFX_MEMORY_OBJECT_CONTROL_UC
    );
    assert_eq!(
        addr_lo(&stream.dwords, AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE + 1),
        bsd_mpc as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE + 3],
        MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
    );
    assert_eq!(
        addr_lo(&stream.dwords, AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE + 4),
        mpr as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_BSP_BUF_BASE_ADDR_STATE + 6],
        MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_AVC_PICID_STATE + 1], 1);
    for idx in 2..MFD_AVC_PICID_STATE_DWORD_COUNT {
        assert_eq!(stream.dwords[AVC_CMD_OFFSET_AVC_PICID_STATE + idx], 0);
    }
    for ref_idx in 0..16 {
        let ref_offset = AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 1 + ref_idx * 2;
        assert_eq!(addr_lo(&stream.dwords, ref_offset), dmv_reference as u32);
        assert_eq!(
            addr_hi(&stream.dwords, ref_offset),
            (dmv_reference >> 32) as u32
        );
    }
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 33],
        MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
    );
    assert_eq!(
        addr_lo(&stream.dwords, AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 34),
        dmv_write as u32
    );
    assert_eq!(
        addr_hi(&stream.dwords, AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 34),
        (dmv_write >> 32) as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 36],
        MFX_MEMORY_ADDRESS_ATTRIBUTES_UC
    );
    for poc_idx in 0..34 {
        let expected = match poc_idx {
            MFX_AVC_DMV_DEST_TOP => plan.picture.top_field_order_cnt as u32,
            MFX_AVC_DMV_DEST_BOTTOM => plan.picture.bottom_field_order_cnt as u32,
            _ => 0,
        };
        assert_eq!(
            stream.dwords[AVC_CMD_OFFSET_AVC_DIRECTMODE_STATE + 37 + poc_idx],
            expected
        );
    }
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE],
        MFX_AVC_REF_IDX_STATE_DW0
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + 1], 0);
    for idx in 2..MFX_AVC_REF_IDX_STATE_DWORD_COUNT {
        assert_eq!(stream.dwords[AVC_CMD_OFFSET_AVC_REF_IDX_STATE + idx], 0);
    }
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 5],
        MFX_AVC_IMG_STATE_DW5_DEFAULT
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 1],
        120 * 68
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 1] & !MFX_AVC_IMG_FRAME_SIZE_MAX,
        0
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 2],
        (120 - 1) | ((68 - 1) << 16)
    );
    let img_dw3_expected = ((plan.picture.weighted_bipred_idc as u32) << 10)
        | (bit(plan.picture.weighted_pred) << 12)
        | (((plan.picture.chroma_qp_index_offset as i32 as u32) & 0x1f) << 16)
        | (((plan.picture.second_chroma_qp_index_offset as i32 as u32) & 0x1f) << 24);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 3],
        img_dw3_expected
    );
    let img_dw4_expected = bit(plan.picture.field_pic)
        | (bit(plan.picture.mb_adaptive_frame_field && !plan.picture.field_pic) << 1)
        | (bit(plan.picture.frame_mbs_only) << 2)
        | (bit(plan.picture.transform_8x8) << 3)
        | (bit(plan.picture.direct_8x8_inference) << 4)
        | (bit(plan.picture.constrained_intra_pred) << 5)
        | (bit(!plan.picture.reference_pic) << 6)
        | (bit(plan.picture.entropy_coding_mode) << 7)
        | (1 << 10);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 4],
        img_dw4_expected
    );
    let img_dw13_expected = ((plan.picture.pic_init_qp_minus26 as i32 as u32) & 0xff)
        | (((u32::from(plan.picture.num_ref_idx_l0_active_minus1) + 1) & 0x3f) << 8)
        | (((u32::from(plan.picture.num_ref_idx_l1_active_minus1) + 1) & 0x3f) << 16);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 13],
        img_dw13_expected
    );
    let img_dw14_expected = bit(plan.picture.pic_order_present)
        | (bit(plan.picture.delta_pic_order_always_zero) << 1)
        | (((plan.picture.pic_order_cnt_type as u32) & 0x03) << 2)
        | (((plan.picture.slice_group_map_type as u32) & 0x07) << 8)
        | (bit(plan.picture.redundant_pic_cnt_present) << 11)
        | (((plan.picture.num_slice_groups_minus1 as u32) & 0x07) << 12)
        | (bit(plan.picture.deblocking_filter_control_present) << 15)
        | (((plan.picture.log2_max_frame_num_minus4 as u32) & 0xff) << 16)
        | (((plan.picture.log2_max_pic_order_cnt_lsb_minus4 as u32) & 0xff) << 24);
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 14],
        img_dw14_expected
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 15],
        u32::from(plan.picture.frame_num) << 16
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE + 1] & 0x0f,
        MFX_AVC_SLICE_TYPE_I as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE + 4],
        plan.slice.first_mb_in_slice
    );
    assert_eq!(stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE + 5] & 0xff, 0);
    assert_eq!(
        (stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE + 5] >> 16)
            & MFX_AVC_SLICE_NEXT_POSITION_MAX,
        plan.picture.pic_height_in_mbs() as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_SLICE_STATE + 6] & (1 << 19),
        1 << 19
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT + 1],
        slice_record.length + slice_record.offset
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT + 2],
        plan.slice.slice_data_offset
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT + 4] & 0x07,
        plan.slice.slice_data_bit_offset as u32
    );
    assert_eq!(
        stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT + 4],
        (plan.slice.slice_data_bit_offset as u32)
            | (1 << 3)
            | (1 << 7)
            | (plan.slice.first_mb_byte_offset << 16)
    );
    assert_eq!(
        (stream.dwords[AVC_CMD_OFFSET_AVC_BSD_OBJECT + 4] >> 16) & 0xffff,
        plan.slice.first_mb_byte_offset
    );
    println!(
        "avc_recipe_probe: ok bytes={} coded={}x{} mb={}x{} poc={}/{} slice_off={} payload_bit={} first_mb_byte={} first_mb_bit={} img13=0x{:08x} img14=0x{:08x} command_dwords={}",
        plan.bitstream_bytes,
        plan.picture.coded_width(),
        plan.picture.coded_height(),
        plan.picture.pic_width_in_mbs(),
        plan.picture.pic_height_in_mbs(),
        plan.picture.top_field_order_cnt,
        plan.picture.bottom_field_order_cnt,
        plan.slice.slice_data_offset,
        plan.slice.slice_data_bit_offset_from_payload,
        plan.slice.first_mb_byte_offset,
        plan.slice.slice_data_bit_offset,
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 13],
        stream.dwords[AVC_CMD_OFFSET_AVC_IMG_STATE + 14],
        stream.dwords.len(),
    );
}
