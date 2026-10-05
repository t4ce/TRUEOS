#!/usr/bin/env python3
"""Check production render register packets and request invalidation without a GPU."""
from pathlib import Path
import json
import subprocess
import tempfile

import test_clip_position3_uv_texture as production

ROOT = Path(__file__).resolve().parents[2]
production.ROOT = ROOT
constant, item = production.constant, production.item


def ring_request_checks() -> list[str]:
    constants = "src/intel/render/constants.rs"
    submit = "src/intel/render/submit.rs"
    carrier = "src/intel/render/picasso_carrier.rs"
    declarations = [constant(constants, name) for name in (
        "RENDER_RING_ENTRY_DWORDS", "RENDER_RING_ENTRY_BYTES", "MI_NOOP",
        "MI_BATCH_BUFFER_START_GEN8", "MI_BATCH_PPGTT", "MI_ARB_CHECK",
        "MI_ARB_CHECK_PRE_PARSER_DISABLE_MASK", "MI_ARB_CHECK_PRE_PARSER_DISABLE",
        "PIPE_CONTROL_CMD", "PIPE_CONTROL_VF_CACHE_INVALIDATE",
        "PIPE_CONTROL_L3_READ_ONLY_CACHE_INVALIDATE_HEADER",
        "PIPE_CONTROL_COMMAND_CACHE_INVALIDATE", "PIPE_CONTROL_TLB_INVALIDATE",
        "PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE", "PIPE_CONTROL_DEST_GGTT",
        "PIPE_CONTROL_CS_STALL", "PIPE_CONTROL_INVALIDATE_BITS",
        "PIPE_CONTROL_REQUEST_INVALIDATE_BITS", "RESULT_DEBUG_SENTINEL",
        "RESULT_SLOT_REQUEST_INVALIDATE_DWORD", "RESULT_SLOT_SCENE_FRAME_DWORD",
        "RCS_EXEC_RESULT_REQUEST_INVALIDATE_DONE_LO", "RCS_EXEC_RESULT_REQUEST_INVALIDATE_DONE_HI",
    )]
    declarations += [item(submit, name) for name in (
        "device_is_gfx12", "device_is_gfx125", "append_ring_batch_start",
    )]
    declarations += [item("src/intel/render/state.rs", "RenderWarmState"),
                     item("src/intel/render/pipeline.rs", "render_pipe_control_packet"),
                     item(carrier, "picasso_carrier_ring_entry"),
                     item(carrier, "PicassoCarrierSubmitProofSnapshot")]
    declarations += [constant("src/intel/render/pipeline.rs", "RESULT_SLOT_DEPTH_STATE_WA_DWORD")]
    declarations += [constant("src/intel/render/primary.rs", name) for name in (
        "RESULT_SLOT_SECONDARY_RETURN_DWORD", "PICASSO_PIPELINE_STATS_BEGIN_DWORD",
    )]
    # Check actual callers as well as the emitter: the Picasso result address
    # belongs to its carrier GGTT lease, independently of the batch's PPGTT VA.
    regular = item(submit, "submit_warm_render_batch")
    picasso = item(carrier, "submit_picasso_render1_batch")
    assert "GPU_VA_BATCH_BASE, false, GPU_VA_RESULT_BASE" in regular
    assert "batch_gpu, true, picasso_render1_result_ggtt(lease)" in picasso
    assert (ROOT / submit).read_text().count("append_ring_batch_start(") == 2
    assert (ROOT / carrier).read_text().count("append_ring_batch_start(") == 1
    declarations += [r"""
mod intel {
    use std::cell::RefCell;
    thread_local! { pub static FLUSHES: RefCell<Vec<(usize, usize)>> = const { RefCell::new(Vec::new()) }; }
    pub fn dma_flush(address: *mut u8, bytes: usize) {
        FLUSHES.with(|flushes| flushes.borrow_mut().push((address as usize, bytes)));
    }
}

fn fixture(device_id: u16, ring: &mut [u32], result: &mut [u32]) -> RenderWarmState {
    let mut warm: RenderWarmState = unsafe { core::mem::zeroed() };
    warm.device_id = device_id;
    warm.ring_virt = ring.as_mut_ptr().cast();
    warm.ring_len = ring.len() * 4;
    warm.result_virt = result.as_mut_ptr().cast();
    warm.result_len = result.len() * 4;
    intel::FLUSHES.with(|flushes| flushes.borrow_mut().clear());
    warm
}

#[test]
fn gen12_request_invalidates_before_batch_fetch_in_both_address_spaces() {
    for device in [0x4680, 0x5690] {
        for (ppgtt, result_base) in [(false, 0x840000u64), (true, 0xA54000)] {
            let mut ring = vec![0xfeedface; 1024];
            let mut result = vec![0xbadc0ffe; 1024];
            let warm = fixture(device, &mut ring, &mut result);
            assert_eq!(append_ring_batch_start(warm, 128, 0x1750000, ppgtt, result_base), Some(192));
            assert_eq!(&ring[32..48], &[
                0x02800101, 0x7A000404, 0x21144C1C,
                (result_base + 112) as u32, 0, 0xC0DE7751, 0xC0DE7752, 0x02800100,
                if ppgtt { 0x18800101 } else { 0x18800001 },
                0x1750000, 0, 0, 0, 0, 0, 0,
            ]);
            assert!(ring[..32].iter().chain(&ring[48..]).all(|&word| word == 0xfeedface));
            assert_eq!(&result[28..30], &[0xC0DE7700; 2]);
            assert!(result[..28].iter().chain(&result[30..]).all(|&word| word == 0xbadc0ffe));
            intel::FLUSHES.with(|flushes| assert_eq!(&*flushes.borrow(), &[
                (warm.result_virt as usize + 112, 8), (warm.ring_virt as usize + 128, 64),
            ]));
            let proof = picasso_carrier_ring_entry(warm, 128).unwrap();
            assert_eq!(proof.len(), 16);
            assert_eq!(&proof[8..11], &ring[40..43]);
            assert_eq!(proof, ring[32..48]);
        }
    }
}

#[test]
fn legacy_request_retains_batch_start_without_gen12_commands_or_result_write() {
    let mut ring = vec![0xfeedface; 1024];
    let mut result = vec![0xbadc0ffe; 1024];
    let mut warm = fixture(0x1912, &mut ring, &mut result);
    warm.result_virt = core::ptr::null_mut();
    warm.result_len = 0;
    assert_eq!(append_ring_batch_start(warm, 0, 0x12345678000, true, u64::MAX), Some(64));
    assert_eq!(&ring[..3], &[0x18800101, 0x45678000, 0x123]);
    assert!(ring[3..16].iter().all(|&word| word == 0));
    assert!(ring[16..].iter().all(|&word| word == 0xfeedface));
    assert!(result.iter().all(|&word| word == 0xbadc0ffe));
    intel::FLUSHES.with(|flushes| assert_eq!(&*flushes.borrow(), &[(warm.ring_virt as usize, 64)]));
}

#[test]
fn request_wraps_only_after_complete_cache_line_and_reads_full_proof() {
    let mut ring = vec![0xfeedface; 1024];
    let mut result = vec![0xbadc0ffe; 1024];
    let warm = fixture(0x4680, &mut ring, &mut result);
    assert_eq!(RENDER_RING_ENTRY_BYTES, 64);
    assert_eq!(4096 % RENDER_RING_ENTRY_BYTES, 0);
    assert_eq!(append_ring_batch_start(warm, 4032, 0x1750000, false, 0x840000), Some(0));
    assert!(ring[..1008].iter().all(|&word| word == 0xfeedface));
    assert_eq!(picasso_carrier_ring_entry(warm, 4032).unwrap(), ring[1008..]);
    assert_eq!(picasso_carrier_ring_entry(warm, 4048), None);
    assert_eq!(picasso_carrier_ring_entry(warm, 4096), None);
    let mut short = warm;
    short.ring_len = 16;
    assert_eq!(picasso_carrier_ring_entry(short, 0), None);
}

#[test]
fn invalid_request_inputs_leave_ring_result_and_cache_publication_untouched() {
    let mut ring = vec![0xfeedface; 1024];
    let mut result = vec![0xbadc0ffe; 1024];
    let warm = fixture(0x4680, &mut ring, &mut result);
    let mut cases = vec![(warm, 16, 0x1750000, false, 0x840000),
        (warm, 4096, 0x1750000, false, 0x840000),
        (warm, 0, 0x1750001, false, 0x840000),
        (warm, 0, u64::MAX - 3, true, 0x840000),
        (warm, 0, 1u64 << 48, true, 0x840000),
        (warm, 0, 1u64 << 32, false, 0x840000),
        (warm, 0, 0x1750000, false, 0x840004),
        (warm, 0, 0x1750000, false, u64::MAX - 7),
        (warm, 0, 0x1750000, false, 0xFFFFFF90),
    ];
    for size in [0, 32, 64, 65, 4095, u32::MAX as usize + 1] {
        let mut invalid = warm; invalid.ring_len = size;
        cases.push((invalid, 0, 0x1750000, false, 0x840000));
    }
    for pointer in [core::ptr::null_mut(), unsafe { warm.ring_virt.add(1) }] {
        let mut invalid = warm; invalid.ring_virt = pointer;
        cases.push((invalid, 0, 0x1750000, false, 0x840000));
    }
    let mut invalid = warm; invalid.result_len = 119;
    cases.push((invalid, 0, 0x1750000, false, 0x840000));
    for pointer in [core::ptr::null_mut(), unsafe { warm.result_virt.add(1) }] {
        let mut invalid = warm; invalid.result_virt = pointer;
        cases.push((invalid, 0, 0x1750000, false, 0x840000));
    }
    for (state, tail, batch, ppgtt, result_base) in cases {
        assert_eq!(append_ring_batch_start(state, tail, batch, ppgtt, result_base), None);
        assert!(ring.iter().all(|&word| word == 0xfeedface));
        assert!(result.iter().all(|&word| word == 0xbadc0ffe));
        intel::FLUSHES.with(|flushes| assert!(flushes.borrow().is_empty()));
    }
}

#[test]
fn request_accepts_exact_result_bounds_and_resets_previous_cookie() {
    let mut ring = vec![0xfeedface; 32];
    let mut result = vec![0xbadc0ffe; 30];
    result[28] = RCS_EXEC_RESULT_REQUEST_INVALIDATE_DONE_LO;
    result[29] = RCS_EXEC_RESULT_REQUEST_INVALIDATE_DONE_HI;
    let warm = fixture(0x4680, &mut ring, &mut result);
    assert_eq!(append_ring_batch_start(warm, 0, (1u64 << 48) - 4, true, 0xFFFFFF88), Some(64));
    assert_eq!(&ring[3..5], &[0xFFFFFFF8, 0]);
    assert_eq!(&ring[9..11], &[0xFFFFFFFC, 0xFFFF]);
    assert_eq!(&result[28..30], &[RESULT_DEBUG_SENTINEL; 2]);
}

#[test]
fn request_cookie_has_independent_aligned_qword_storage_and_direct_ggtt_addressing() {
    assert_eq!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD, 28);
    assert_eq!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD % 2, 0);
    assert!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD >= RESULT_SLOT_SCENE_FRAME_DWORD + 2);
    assert!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD + 2 <= RESULT_SLOT_SECONDARY_RETURN_DWORD);
    assert!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD + 2 <= RESULT_SLOT_DEPTH_STATE_WA_DWORD);
    assert!(RESULT_SLOT_REQUEST_INVALIDATE_DWORD + 2 <= PICASSO_PIPELINE_STATS_BEGIN_DWORD);
    assert_eq!(PIPE_CONTROL_REQUEST_INVALIDATE_BITS & (1 << 21), 0); // no Store Data Index
    assert_ne!(PIPE_CONTROL_REQUEST_INVALIDATE_BITS & (1 << 29), 0); // command cache
    assert_ne!(PIPE_CONTROL_REQUEST_INVALIDATE_BITS & (1 << 18), 0); // TLB
    assert_ne!(PIPE_CONTROL_REQUEST_INVALIDATE_BITS & (1 << 20), 0); // CS Stall
    assert_ne!(PIPE_CONTROL_REQUEST_INVALIDATE_BITS & (1 << 14), 0); // QWord write
}
"""]
    return declarations


def main() -> None:
    pipeline = "src/intel/render/pipeline.rs"
    constants = "src/intel/render/constants.rs"
    declarations = [constant(constants, name) for name in (
        "MI_LOAD_REGISTER_IMM", "MI_LRI_FORCE_POSTED",
        "GEN12_L3ALLOC", "GEN12_L3ALLOC_ADL_DEFAULT",
        "MI_LOAD_REGISTER_MEM", "MI_LRI_CS_MMIO", "RCS_RING_BASE",
        "RCS_3DPRIM_BASE_VERTEX", "RCS_3DPRIM_INSTANCE_COUNT",
        "RCS_3DPRIM_START_INSTANCE", "RCS_3DPRIM_START_VERTEX", "RCS_3DPRIM_VERTEX_COUNT",
        "RCS_3DPRIM_XP_BASE_VERTEX", "RCS_3DPRIM_XP_DRAW_ID",
        "TRIANGLE_VS_URB_START", "TRIANGLE_VS_URB_ENTRIES",
    )]
    declarations += [item(pipeline, name) for name in (
        "render_batch_lri_packet", "render_batch_lri_tests",
        "encode_draw_indexed_indirect_register_loads", "draw_indexed_indirect_encoder_tests",
    )]
    declarations += ring_request_checks()
    metadata = json.loads((ROOT / "picasso/picasso-retained-pbr-forward/metadata.json").read_text())
    units = metadata["vs_state"]["urb_entry_64b"]
    slots = metadata["vs_state"]["vue_slots"]
    declarations += [f"const PBR_ENTRY_BYTES: u32 = {units * 64};",
                     f"const PBR_OUTPUT_BYTES: u32 = {slots * 16};", r"""
#[test]
fn pbr_artifact_fits_requested_partition_but_not_reset_partition() {
    let packet = render_batch_lri_packet(GEN12_L3ALLOC, GEN12_L3ALLOC_ADL_DEFAULT).unwrap();
    let requested_bytes = ((packet[2] >> 1) & 0x7F) * 4 * 4096;
    let reset_bytes = ((0xD0000020u32 >> 1) & 0x7F) * 4 * 4096;
    let draw_bytes = TRIANGLE_VS_URB_START * 8192 + TRIANGLE_VS_URB_ENTRIES * PBR_ENTRY_BYTES;
    assert!(PBR_OUTPUT_BYTES <= PBR_ENTRY_BYTES);
    assert_eq!((draw_bytes, requested_bytes, reset_bytes), (490496, 524288, 262144));
    assert!(draw_bytes <= requested_bytes);
    assert!(draw_bytes > reset_bytes);
}
"""]
    with tempfile.TemporaryDirectory(prefix="trueos-render-lri-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text("#![allow(dead_code, unfulfilled_lint_expectations)]\n" + "\n".join(declarations))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
