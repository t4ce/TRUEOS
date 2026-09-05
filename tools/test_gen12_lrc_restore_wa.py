#!/usr/bin/env python3
"""Exercise production Gen12 context-restore WA encoding without GPU access."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def direct_context_tests() -> str:
    constants = "src/intel/gpgpu/rcs/constants.rs"
    context = "src/intel/gpgpu/rcs/context.rs"
    source = "\nmod direct_context {\nuse crate::*;\n"
    source += "\n".join(constant(constants, name) for name in (
        "DIRECT_RCS_CONTEXT_BYTES", "DIRECT_RCS_LRC_STATE_OFFSET_DWORDS",
        "DIRECT_RCS_GPU_VA_CONTEXT_BASE", "FONT_RCS_GPU_VA_CONTEXT_BASE",
        "EXECUTION_RCS_GPU_VA_CONTEXT_BASE", "LFM25_RCS_GPU_VA_CONTEXT_BASE",
        "UI4_COMPOSITOR_RCS_GPU_VA_CONTEXT_BASE",
    ))
    source += "\n" + "\n".join(
        item("src/intel/gpgpu/rcs/runtime.rs", name)
        for name in ("DirectRcsState", "DirectRcsGpuVa")
    )
    source += "\n" + "\n".join(item(context, name) for name in (
        "direct_rcs_init_lrc_context_image", "direct_rcs_init_lrc_context_image_with_root",
        "direct_rcs_write_lrc_ring_tail", "direct_rcs_ctx_control_value",
        "direct_rcs_mi_lri_cmd", "direct_rcs_push_nops",
        "direct_rcs_masked_bit_disable", "direct_rcs_masked_bits_update",
    ))
    return source + r"""
#[test]
fn direct_compute_lanes_restore_from_their_own_ggtt_mapping() {
    for base in [DIRECT_RCS_GPU_VA_CONTEXT_BASE, FONT_RCS_GPU_VA_CONTEXT_BASE,
        EXECUTION_RCS_GPU_VA_CONTEXT_BASE, LFM25_RCS_GPU_VA_CONTEXT_BASE,
        UI4_COMPOSITOR_RCS_GPU_VA_CONTEXT_BASE] {
        let mut context = vec![0xfeedfaceu32; DIRECT_RCS_CONTEXT_BYTES / 4];
        // Every field is an integer, raw pointer, or bool, so zero is a valid
        // host fixture; only the real initializer's required fields are set.
        let mut state: DirectRcsState = unsafe { core::mem::zeroed() };
        state.context_virt = context.as_mut_ptr().cast();
        state.ppgtt_phys = 0x12345678000;
        state.gpu_va.context = base;
        assert!(direct_rcs_init_lrc_context_image(state, 0x4680, 0x830000, 64, 0x1001));
        let regs = &context[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS..];
        assert_eq!(&regs[18..20], &[0x21c0, (base + 15 * 4096) as u32 | 1]);
        assert_eq!(&regs[20..24], &[0x21c4, (base + 14 * 4096) as u32 | 1,
            0x21c8, 0x340]);
        assert_eq!(regs[3], direct_rcs_ctx_control_value(false));
        assert_eq!((regs[5], regs[7], regs[9]), (0, 64, 0x830000));
        assert_eq!(&context[14 * 1024..14 * 1024 + 3],
            &[0x11000001, 0x20d8, 0x00400040]);
        assert_eq!(&context[15 * 1024..15 * 1024 + 4],
            &[0x11000001, 0x2580, 0x04010400, MI_BATCH_BUFFER_END]);
        let before = context.clone();
        direct_rcs_write_lrc_ring_tail(state, 128);
        let tail = DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + 7;
        assert_eq!(context[tail], 128);
        assert_eq!(&context[..tail], &before[..tail]);
        assert_eq!(&context[tail + 1..], &before[tail + 1..]);
    }
}

#[test]
fn direct_context_rejects_invalid_mapping_and_leaves_other_generations_unmodified() {
    let mut context = vec![0u32; DIRECT_RCS_CONTEXT_BYTES / 4];
    let mut state: DirectRcsState = unsafe { core::mem::zeroed() };
    state.context_virt = context.as_mut_ptr().cast();
    state.gpu_va.context = DIRECT_RCS_GPU_VA_CONTEXT_BASE + 4;
    assert!(!direct_rcs_init_lrc_context_image(state, 0x4680, 0, 0, 0));
    state.gpu_va.context = DIRECT_RCS_GPU_VA_CONTEXT_BASE;
    assert!(direct_rcs_init_lrc_context_image(state, 0x56a0, 0, 0, 0));
    let regs = &context[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS..];
    assert_eq!((regs[19], regs[21], regs[23]), (0, 0, 0));
    assert!(context[14 * 1024..].iter().all(|&dw| dw == 0));
}
}
"""


def main() -> None:
    constants = "src/intel/render/constants.rs"
    submit = "src/intel/render/submit.rs"
    source = "#![allow(dead_code)]\n" + "\n".join(
        constant("src/intel/render/picasso_carrier.rs" if name.startswith("PICASSO_")
                 else constants, name)
        for name in (
            "RCS_RING_BASE", "RCS_CS_DEBUG_MODE2", "LRC_STATE_OFFSET_DWORDS",
            "MI_LOAD_REGISTER_IMM", "MI_LRI_CS_MMIO", "MI_LRI_FORCE_POSTED",
            "MI_BATCH_BUFFER_END", "MI_NOOP", "GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT",
            "CTX_CTRL_INHIBIT_SYN_CTX_SWITCH", "CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT",
            "RING_MI_MODE_STOP_RING", "WARM_CONTEXT_BYTES", "GPU_VA_CONTEXT_BASE",
            "PICASSO_RENDER1_GGTT_CONTEXT_BASE", "PICASSO_RENDER2_GGTT_CONTEXT_BASE",
            "PICASSO_RENDER3_GGTT_CONTEXT_BASE", "PICASSO_RENDER4_GGTT_CONTEXT_BASE",
        )
    )
    source += "\n" + "\n".join(
        item(submit, name)
        for name in (
            "device_is_gfx12", "device_is_gfx125", "masked_bit_enable",
            "masked_bit_disable", "masked_bits_update", "rcs_ctx_control_value",
            "mi_lri_num_regs", "mi_lri_context_cmd", "push_mi_nops",
        )
    )
    source += "\n" + item("src/intel/render/lrc.rs", "init_gen12_rcs_restore_wa")
    source += "\n" + item("src/intel/render/lrc.rs", "init_gen12_lrc_context_image")
    source += "\n" + item("src/intel/render/lrc.rs", "write_gen12_lrc_ring_tail")
    source += direct_context_tests()
    source += r"""
#[derive(Copy, Clone)]
struct RenderWarmState { context_virt: *mut u8, context_len: usize, device_id: u16 }
use intel::dma_flush;
mod intel {
    pub fn dma_flush(_: *mut u8, _: usize) {}
    pub mod render { pub(crate) use crate::init_gen12_rcs_restore_wa; }
}

#[test]
fn every_render_lane_uses_its_owned_ggtt_restore_batch() {
    for base in [GPU_VA_CONTEXT_BASE, PICASSO_RENDER1_GGTT_CONTEXT_BASE,
        PICASSO_RENDER2_GGTT_CONTEXT_BASE, PICASSO_RENDER3_GGTT_CONTEXT_BASE,
        PICASSO_RENDER4_GGTT_CONTEXT_BASE] {
        let mut context = vec![0xfeedfaceu32; WARM_CONTEXT_BYTES / 4];
        let warm = RenderWarmState { context_virt: context.as_mut_ptr().cast(),
            context_len: context.len() * 4, device_id: 0x4680 };
        assert!(init_gen12_lrc_context_image(warm, 0x830000, 64, 0x1001,
            0x12345678000, base));
        let regs = &context[LRC_STATE_OFFSET_DWORDS..];
        assert_eq!(&regs[18..20], &[0x21c0, (base + 15 * 4096) as u32 | 1]);
        assert_eq!(&regs[20..24], &[0x21c4, (base + 14 * 4096) as u32 | 1,
            0x21c8, 0x340]);
        assert_eq!(regs[3], rcs_ctx_control_value(false));
        assert_eq!(regs[5], 0); // HEAD remains initially zero.
        assert_eq!(regs[7], 64);
        assert_eq!(regs[9], 0x830000); // Ring address is independent of the WA GGTT mapping.
        let batch = &context[14 * 1024..14 * 1024 + 16];
        assert_eq!(&batch[..3], &[0x11000001, 0x20d8, 0x00400040]);
        assert!(batch[3..].iter().all(|&dw| dw == 0)); // No BBE in length-delimited BB.
        assert!(context[14 * 1024 + 16..15 * 1024].iter().all(|&dw| dw == 0));
        assert_eq!(&context[15 * 1024..15 * 1024 + 4],
            &[0x11000001, 0x2580, 0x04010400, MI_BATCH_BUFFER_END]);
        assert!(context[15 * 1024 + 4..].iter().all(|&dw| dw == 0));
        // A later submission publishes only TAIL, preserving both immutable
        // workaround pages and the GPU-authored saved HEAD.
        let before = context.clone();
        assert!(write_gen12_lrc_ring_tail(warm, 128));
        let tail = LRC_STATE_OFFSET_DWORDS + 7;
        assert_eq!(context[tail], 128);
        assert_eq!(&context[..tail], &before[..tail]);
        assert_eq!(&context[tail + 1..], &before[tail + 1..]);
    }
}

#[test]
fn restore_batch_writes_only_beyond_hardware_save_image() {
    let mut context = vec![0xfeedface; WARM_CONTEXT_BYTES / 4];
    assert!(init_gen12_rcs_restore_wa(&mut context, GPU_VA_CONTEXT_BASE, 0x4680).is_some());
    assert!(context[..14 * 1024].iter().all(|&dw| dw == 0xfeedface));
    assert!(context[14 * 1024 + 16..15 * 1024].iter().all(|&dw| dw == 0xfeedface));
    assert!(context[15 * 1024 + 16..].iter().all(|&dw| dw == 0xfeedface));
}

#[test]
fn reject_invalid_batch_storage_and_unencodable_ggtt_address() {
    for size in [14 * 1024 + 15, 15 * 1024 + 16, 16 * 1024 - 1] {
        let mut short = vec![0xfeedface; size];
        assert_eq!(init_gen12_rcs_restore_wa(&mut short, GPU_VA_CONTEXT_BASE, 0x4680), None);
        assert!(short.iter().all(|&dw| dw == 0xfeedface));
    }
    let mut context = vec![0xfeedface; WARM_CONTEXT_BYTES / 4];
    for address in [GPU_VA_CONTEXT_BASE + 4, 0xfffff000, u64::MAX - 4095] {
        assert_eq!(init_gen12_rcs_restore_wa(&mut context, address, 0x4680), None);
        assert!(context.iter().all(|&dw| dw == 0xfeedface));
    }
}

#[test]
fn unaffected_generations_do_not_get_gen12_restore_workaround() {
    for id in [0x56a0, 0x5690, 0x56c2, 0xffff] {
        let mut context = vec![0xfeedface; WARM_CONTEXT_BYTES / 4];
        assert_eq!(init_gen12_rcs_restore_wa(&mut context, GPU_VA_CONTEXT_BASE, id), Some((0, 0, 0)));
        assert!(context.iter().all(|&dw| dw == 0xfeedface));
    }
}

#[test]
fn post_restore_masks_only_3d_preemption_and_preserves_compute_granularity() {
    let mut context = vec![0u32; 16 * 1024];
    assert_eq!(init_gen12_rcs_restore_wa(&mut context, GPU_VA_CONTEXT_BASE, 0x4680),
        Some(((GPU_VA_CONTEXT_BASE + 14 * 4096) as u32 | 1, 0x340,
              (GPU_VA_CONTEXT_BASE + 15 * 4096) as u32 | 1)));
    let write = context[15 * 1024 + 2];
    let mask = write >> 16;
    assert_eq!(mask, 0x401);
    for old in 0..=u16::MAX as u32 {
        let after = (old & !mask) | (write & mask);
        assert_eq!(after & 0x401, 0x400);
        assert_eq!(after & !0x401, old & !0x401);
        assert_eq!(after & 6, old & 6); // Media/GPGPU preemption bits2:1.
    }
}
"""
    with tempfile.TemporaryDirectory(prefix="trueos-lrc-restore-wa-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "tests.rs"
        executable = directory / "tests"
        rust_source.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source),
                        "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
