extern crate alloc;

use std::io::{self, Read};

#[path = "../src/intel/media/h264_cmd.rs"]
mod h264_cmd;

fn range(gpu_addr: u64, bytes: usize) -> h264_cmd::AvcGpuResourceRange {
    h264_cmd::AvcGpuResourceRange { gpu_addr, bytes }
}

fn command_probe(mut plan: h264_cmd::AvcLongFormatIdrPlan, class: h264_cmd::AvcSliceClass) {
    let surface_bytes = plan.resources.dest_surface.byte_len;
    let base = 0x1_0000_0000u64;
    let mut refs = [None; 16];
    for (index, poc) in [0i32, 8].into_iter().enumerate() {
        refs[index] = Some(h264_cmd::AvcReferenceFrameBinding {
            frame_store_id: index as u8,
            frame_num: index as u16,
            top_field_order_cnt: poc,
            bottom_field_order_cnt: poc,
            surface_gpu_addr: base + (index * surface_bytes) as u64,
            dmv_gpu_addr: base + 0x0800_0000 + (index * 0x10_0000) as u64,
        });
    }
    let active_l0 = usize::from(plan.slice.num_ref_idx_l0_active_minus1) + 1;
    let active_l1 = if matches!(class, h264_cmd::AvcSliceClass::B) {
        usize::from(plan.slice.num_ref_idx_l1_active_minus1) + 1
    } else {
        0
    };
    assert!(active_l0 <= 2 && active_l1 <= 2);
    let mut l0 = [0u8; 16];
    let mut l1 = [0u8; 16];
    l0[0] = 0;
    l0[1] = 1;
    l1[0] = 1;
    l1[1] = 0;
    let references = h264_cmd::AvcReferenceState {
        refs,
        ref_count: 2,
        l0,
        l0_count: active_l0,
        l1,
        l1_count: active_l1,
    };
    plan.resources.reference_surface_count = 2;
    let reference_surfaces =
        core::array::from_fn(|index| range(base + (index * surface_bytes) as u64, surface_bytes));
    let bindings = h264_cmd::AvcPacketResourceBindings {
        dest_surface: range(base + 0x0200_0000, surface_bytes),
        missing_reference_surface: range(base + 0x0300_0000, surface_bytes),
        reference_surfaces,
        bitstream: range(base + 0x0400_0000, 8 * 1024 * 1024),
        intra_rowstore: range(base + 0x0500_0000, plan.resources.rowstore.intra),
        deblocking_filter_rowstore: range(
            base + 0x0510_0000,
            plan.resources.rowstore.deblocking_filter,
        ),
        bsd_mpc_rowstore: range(base + 0x0520_0000, plan.resources.rowstore.bsd_mpc),
        mpr_rowstore: range(base + 0x0530_0000, plan.resources.rowstore.mpr),
        dmv_write_buffer: range(base + 0x0600_0000, plan.resources.dmv_write_buffer_bytes),
        dmv_reference_buffer: range(base + 0x0700_0000, plan.resources.dmv_reference_buffer_bytes),
    };
    let stream =
        h264_cmd::build_long_format_single_picture_command_stream(plan, bindings, references)
            .expect("build inter command stream");
    assert!(h264_cmd::validate_long_format_single_picture_command_stream_shape(&stream));
    if matches!(class, h264_cmd::AvcSliceClass::B) {
        assert_eq!(stream.ref_idx_state_count, 2);
        assert_eq!(
            stream.dwords[h264_cmd::AVC_CMD_OFFSET_AVC_SLICE_STATE],
            h264_cmd::MFX_AVC_REF_IDX_STATE_DW0
        );
        assert_eq!(stream.dwords[h264_cmd::AVC_CMD_OFFSET_AVC_SLICE_STATE + 1], 1);
    } else {
        assert_eq!(stream.weight_offset_state_count, 1);
        assert_eq!(
            stream.dwords[h264_cmd::AVC_CMD_OFFSET_AVC_SLICE_STATE],
            h264_cmd::MFX_AVC_WEIGHTOFFSET_STATE_DW0
        );
    }
}

fn start_codes(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut cursor = 0usize;
    while cursor + 3 <= bytes.len() {
        let len = if bytes[cursor..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if bytes[cursor..].starts_with(&[0, 0, 1]) {
            3
        } else {
            cursor += 1;
            continue;
        };
        out.push((cursor, len));
        cursor += len;
    }
    out
}

fn main() {
    let mut bytes = Vec::new();
    io::stdin().read_to_end(&mut bytes).expect("read Annex-B");
    let starts = start_codes(&bytes);
    let mut sps = Vec::new();
    let mut pps = Vec::new();
    let mut access_unit = Vec::new();
    let mut pictures = Vec::new();

    for (index, (start, prefix)) in starts.iter().copied().enumerate() {
        let end = starts
            .get(index + 1)
            .map(|entry| entry.0)
            .unwrap_or(bytes.len());
        let nal_type = bytes[start + prefix] & 0x1f;
        let nal = &bytes[start..end];
        match nal_type {
            7 => sps = nal.to_vec(),
            8 => pps = nal.to_vec(),
            9 => {
                if !access_unit.is_empty() {
                    pictures.push(core::mem::take(&mut access_unit));
                }
                access_unit.extend_from_slice(nal);
            }
            _ => access_unit.extend_from_slice(nal),
        }
    }
    if !access_unit.is_empty() {
        pictures.push(access_unit);
    }

    let mut b_slices = 0usize;
    let mut probed_b_commands = false;
    let mut probed_weighted_p_commands = false;
    for (index, picture) in pictures.iter().enumerate() {
        let mut frame = Vec::with_capacity(sps.len() + pps.len() + picture.len());
        frame.extend_from_slice(&sps);
        frame.extend_from_slice(&pps);
        frame.extend_from_slice(picture);
        let plan = h264_cmd::parse_annexb_single_picture_plan(&frame)
            .unwrap_or_else(|error| panic!("picture {} parse failed: {:?}", index, error));
        if matches!(plan.slice.class, h264_cmd::AvcSliceClass::B) {
            b_slices += 1;
            if !probed_b_commands
                && plan.slice.num_ref_idx_l0_active_minus1 == 0
                && plan.slice.num_ref_idx_l1_active_minus1 == 0
            {
                command_probe(plan, h264_cmd::AvcSliceClass::B);
                probed_b_commands = true;
            }
        } else if matches!(plan.slice.class, h264_cmd::AvcSliceClass::P)
            && plan.picture.weighted_pred
            && plan.slice.num_ref_idx_l0_active_minus1 == 0
            && !probed_weighted_p_commands
        {
            command_probe(plan, h264_cmd::AvcSliceClass::P);
            probed_weighted_p_commands = true;
        }
        println!(
            "decode={} class={} frame_num={} poc_lsb={} refs_l0={} refs_l1={} direct_spatial={}",
            index,
            plan.slice.class.label(),
            plan.picture.frame_num,
            plan.picture.pic_order_cnt_lsb,
            plan.slice.num_ref_idx_l0_active_minus1 + 1,
            plan.slice.num_ref_idx_l1_active_minus1 + 1,
            plan.slice.direct_spatial_mv_pred as u8,
        );
    }
    assert!(b_slices != 0, "input contained no parsed B slices");
    assert!(probed_b_commands, "no simple B command probe candidate");
    assert!(probed_weighted_p_commands, "no weighted P command probe candidate");
    println!("pictures={} b_slices={} result=ok", pictures.len(), b_slices);
}
