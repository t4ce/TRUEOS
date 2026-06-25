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

fn main() {
    let bytes = include_bytes!("vid/x31_head_movie_first_frame.h264");
    let plan = parse_annexb_single_idr_plan(bytes).expect("parse x31 first frame");
    validate_long_format_single_idr(plan).expect("validate x31 first frame plan");

    let base = 0x1_0000_0000u64;
    let bitstream_base = base + 0x0100_0000;
    let bitstream_window_bytes = 8 * 1024 * 1024;
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
    let slice_record = avc_long_format_slice_record(plan.slice);

    println!(
        "trueos_avc_recipe_trace repo={} commit={} platform={}",
        UPSTREAM_INTEL_MEDIA_DRIVER_REPO,
        UPSTREAM_INTEL_MEDIA_DRIVER_COMMIT,
        UPSTREAM_AVC_PLATFORM
    );
    println!(
        "sample=x31_head_movie_first_frame.h264 bytes={} coded={}x{} mb={}x{} command_blocks={} command_dwords={}",
        plan.bitstream_bytes,
        plan.picture.coded_width(),
        plan.picture.coded_height(),
        plan.picture.pic_width_in_mbs(),
        plan.picture.pic_height_in_mbs(),
        stream.command_count,
        stream.dwords.len()
    );
    println!(
        "resources dest=0x{:x}+0x{:x} missing_ref=0x{:x}+0x{:x} bitstream=0x{:x}+0x{:x} scratch=0x{:x}",
        bindings.dest_surface.gpu_addr,
        bindings.dest_surface.bytes,
        bindings.missing_reference_surface.gpu_addr,
        bindings.missing_reference_surface.bytes,
        bindings.bitstream.gpu_addr,
        bindings.bitstream.bytes,
        scratch
    );
    println!(
        "slice_record offset={} length={} bsd_start={} bsd_length={}",
        slice_record.offset,
        slice_record.length,
        plan.slice.slice_data_offset,
        slice_record.offset + slice_record.length
    );
    for block in AVC_LONG_FORMAT_SINGLE_IDR_COMMAND_BLOCKS {
        println!(
            "block offset={} dwords={} command={} upstream={}#{}",
            block.offset, block.dword_count, block.command, block.upstream_file, block.upstream_symbol
        );
        let end = block.offset + block.dword_count;
        for (idx, dword) in stream.dwords[block.offset..end].iter().enumerate() {
            println!("  dw[{:<3}] = 0x{:08x}", block.offset + idx, dword);
        }
    }
}
