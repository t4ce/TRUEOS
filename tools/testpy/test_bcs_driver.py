#!/usr/bin/env python3
"""Exercise production BCS plan encoding and sparse driver publication on the host.

Only CPU backing, cache publication and the mapper mutex are replaced. Command
encoding, control mappings, sparse mapping integration and ownership predicates
are extracted from the actual driver. This does not execute GPU commands.

Run from any directory with ``python3 tools/testpy/test_bcs_driver.py``.
"""

from pathlib import Path
import subprocess
import tempfile

from test_bcs_plan import harness_source as planner_harness
from test_clip_position3_uv_texture import ROOT, constant, item


def harness_source() -> str:
    driver = "src/intel/copy/blt.rs"
    declarations = [
        constant(driver, name)
        for name in (
            "DIRECT_BLT_PPGTT_BYTES",
            "DIRECT_BLT_RING_BYTES",
            "DIRECT_BLT_CONTEXT_BYTES",
            "DIRECT_BLT_BATCH_BYTES",
            "DIRECT_BLT_RESULT_BYTES",
            "DIRECT_BLT_COPY_BYTES",
            "DIRECT_BLT_GPU_VA_RING_BASE",
            "DIRECT_BLT_GPU_VA_CONTEXT_BASE",
            "DIRECT_BLT_GPU_VA_BATCH_BASE",
            "DIRECT_BLT_GPU_VA_RESULT_BASE",
            "DIRECT_BLT_GPU_VA_SRC_BASE",
            "DIRECT_BLT_GPU_VA_DST_BASE",
            "BCS_BATCH_COOKIE",
            "MI_NOOP",
            "MI_BATCH_BUFFER_END",
            "MI_STORE_DATA_IMM_GGTT_DW1",
            "MI_FLUSH_DW",
            "MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE",
            "MI_FLUSH_DW_DEST_GGTT",
            "MI_FLUSH_DW_TLB_INVALIDATE",
            "MI_ARB_CHECK",
        )
    ]
    declarations.extend(
        item(driver, name)
        for name in (
            "DirectBltState",
            "guc_blt_begin_batch",
            "guc_blt_finish_batch",
            "guc_blt_encode_plan_batch",
            "guc_blt_map_surface_mappings",
            "guc_blt_control_mappings",
            "guc_blt_map_ui4_surfaces",
            "direct_blt_init_ppgtt",
            "direct_blt_map_ppgtt_region",
            "guc_blt_hardware_retired",
            "guc_blt_saved_head_reached_tail",
            "guc_blt_state_reuse_permitted",
            "guc_blt_saved_context_tests",
        )
    )
    shim = r'''
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use super::bcs_mappings::{BcsMapping, BcsMappings};
use super::bcs_plan::{BcsBatch, BcsPlan, BcsOp, CopyBackend, SourceCache,
    BCS_PLAN_ENVELOPE_DWORDS};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
static GUC_BLT_MAPPINGS: Mutex<BcsMappings> = Mutex::new(BcsMappings::new());
'''
    hardware = r'''
thread_local! {
    static FLUSHES: std::cell::RefCell<Vec<(usize, usize)>> = const {
        std::cell::RefCell::new(Vec::new())
    };
    static FENCES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
fn dma_flush_cache_lines(pointer: *mut u8, bytes: usize) {
    FLUSHES.with(|flushes| flushes.borrow_mut().push((pointer as usize, bytes)));
}
fn dma_flush_fence() { FENCES.with(|fences| fences.set(fences.get() + 1)); }
fn dma_flush(pointer: *mut u8, bytes: usize) {
    dma_flush_cache_lines(pointer, bytes);
    dma_flush_fence();
}
fn clear_publication() {
    FLUSHES.with(|flushes| flushes.borrow_mut().clear());
    FENCES.with(|fences| fences.set(0));
}
'''
    source = planner_harness().replace(
        "mod blt {\n",
        constant("src/intel/mod.rs", "GEN8_PAGE_PRESENT")
        + "\n"
        + hardware
        + "\nmod blt {\n"
        + shim,
        1,
    )
    delimiter = "\n}\n#[path = "
    assert source.count(delimiter) == 1, "planner harness module boundary changed"
    return source.replace(
        delimiter,
        "\n" + "\n".join(declarations) + DRIVER_TESTS + delimiter,
        1,
    )


DRIVER_TESTS = r'''
#[cfg(test)]
mod integration_tests {
    use super::*;
    const GUARD: u32 = 0xa55a_c33c;
    const TABLE_GUARD: u64 = 0x9669_a55a_5aa5_6996;

    struct Fixture {
        table: Vec<u64>,
        batch: Vec<u32>,
        result: Vec<u32>,
    }
    impl Fixture {
        fn new() -> Self {
            GUC_BLT_MAPPINGS.lock().invalidate();
            super::super::clear_publication();
            Self {
                table: vec![TABLE_GUARD; DIRECT_BLT_PPGTT_BYTES / 8 + 2],
                batch: vec![GUARD; DIRECT_BLT_BATCH_BYTES / 4 + 2],
                result: vec![GUARD; DIRECT_BLT_RESULT_BYTES / 4 + 2],
            }
        }
        fn state(&mut self) -> DirectBltState {
            DirectBltState {
                ring_phys: 0x100000, ring_virt: core::ptr::null_mut(),
                context_phys: 0x200000, context_virt: core::ptr::null_mut(),
                batch_phys: 0x300000, batch_virt: unsafe { self.batch.as_mut_ptr().add(1).cast() },
                result_phys: 0x400000, result_virt: unsafe { self.result.as_mut_ptr().add(1).cast() },
                src_phys: 0x500000, src_virt: core::ptr::null_mut(),
                dst_phys: 0x600000, dst_virt: core::ptr::null_mut(),
                ppgtt_phys: 0x10000000, ppgtt_virt: unsafe { self.table.as_mut_ptr().add(1).cast() },
            }
        }
        fn leaf(&self, gpu: u64) -> u64 {
            self.table[1 + 3 * 4096 / 8 + (gpu / 4096) as usize]
        }
        fn table_publication(&self) -> Vec<(usize, usize)> {
            let base = self.table.as_ptr() as usize + 8;
            super::super::FLUSHES.with(|flushes| flushes.borrow().iter()
                .filter_map(|&(pointer, bytes)| {
                    (pointer >= base && pointer < base + DIRECT_BLT_PPGTT_BYTES)
                        .then_some((pointer.wrapping_sub(base), bytes))
                }).collect())
        }
        fn assert_guards(&self) {
            assert_eq!(self.table[0], TABLE_GUARD);
            assert_eq!(self.table.last(), Some(&TABLE_GUARD));
            assert_eq!(self.batch[0], GUARD);
            assert_eq!(self.batch.last(), Some(&GUARD));
            assert_eq!(self.result[0], GUARD);
            assert_eq!(self.result.last(), Some(&GUARD));
        }
    }
    fn surface(gpu: u64) -> GucBcs0RgbaSurface {
        GucBcs0RgbaSurface {
            phys: gpu + 0x100000000, gpu, bytes: 64 * 256,
            width: 64, height: 64, pitch_bytes: 256,
        }
    }
    fn copy(source: GucBcs0RgbaSurface) -> GucBcs0RgbaCopy {
        GucBcs0RgbaCopy {
            source, source_x: 0, source_y: 0, destination_x: 0, destination_y: 0,
            width: 16, height: 16,
        }
    }
    fn operation(source: GucBcs0RgbaSurface, backend: CopyBackend) -> BcsOp {
        BcsOp::Copy { copy: copy(source), backend, source_cache: SourceCache::WriteBack }
    }

    #[test]
    fn repeated_atlas_copies_have_one_mapping_and_warm_repeat_does_not_publish_the_table() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let glyph = copy(surface(0x3000000));
        assert!(guc_blt_map_ui4_surfaces(state, destination, &vec![glyph; 160], false, None));
        assert_eq!(f.table_publication(), vec![(0, DIRECT_BLT_PPGTT_BYTES)]);
        let before = f.table.clone();
        super::super::clear_publication();
        // Changing descriptor count to one must retain the identical unique
        // mapping set; per-glyph mapping metadata would cause another update.
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[glyph], false, None));
        assert!(f.table_publication().is_empty());
        assert_eq!(f.table, before);
        super::super::FENCES.with(|fences| assert_eq!(fences.get(), 0));
        assert_eq!(f.leaf(destination.gpu), destination.phys | 0x1b);
        assert_eq!(f.leaf(glyph.source.gpu), glyph.source.phys | 3);
        f.assert_guards();
    }

    #[test]
    fn changing_sources_and_destination_publishes_only_old_and_new_leaf_cache_lines() {
        let mut f = Fixture::new();
        let state = f.state();
        let first_destination = surface(0x5000000);
        let first = copy(surface(0x3000000));
        assert!(guc_blt_map_ui4_surfaces(state, first_destination, &[first], false, None));
        super::super::clear_publication();
        let destination = surface(0x7000000);
        let next = copy(surface(0x4000000));
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[next], false, None));
        let published = f.table_publication();
        assert!(!published.is_empty());
        assert!(published.iter().all(|&(offset, bytes)| offset % 64 == 0 && bytes % 64 == 0));
        assert!(published.iter().map(|&(_, bytes)| bytes).sum::<usize>() <= 256);
        super::super::FENCES.with(|fences| assert_eq!(fences.get(), 1));
        assert_eq!(f.leaf(first_destination.gpu), 0);
        assert_eq!(f.leaf(first.source.gpu), 0);
        assert_eq!(f.leaf(destination.gpu), destination.phys | 0x1b);
        assert_eq!(f.leaf(next.source.gpu), next.source.phys | 3);
        f.assert_guards();
    }

    #[test]
    fn recycled_source_va_updates_backing_and_the_per_source_cache_contract() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let first = copy(surface(0x3000000));
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[first], false, None));
        let next = GucBcs0RgbaCopy {
            source: GucBcs0RgbaSurface { phys: first.source.phys + 0x100000, ..first.source },
            ..first
        };
        super::super::clear_publication();
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[next], false, Some(&[true])));
        assert_eq!(f.leaf(next.source.gpu), next.source.phys | 0x1b);
        assert!(f.table_publication().iter().all(|&(_, bytes)| bytes < DIRECT_BLT_PPGTT_BYTES));
        super::super::clear_publication();
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[next], false, Some(&[false])));
        assert_eq!(f.leaf(next.source.gpu), next.source.phys | 3);
        f.assert_guards();
    }

    #[test]
    fn invalid_aliases_and_cache_flags_do_not_change_the_published_mapping_set() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let first = copy(surface(0x3000000));
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[first], false, None));
        let before = f.table.clone();
        super::super::clear_publication();
        let conflict = GucBcs0RgbaCopy {
            source: GucBcs0RgbaSurface { phys: first.source.phys + 4096, ..first.source },
            ..first
        };
        assert!(!guc_blt_map_ui4_surfaces(state, destination, &[first, conflict], false, None));
        assert!(!guc_blt_map_ui4_surfaces(state, destination, &[first, first], false, Some(&[false, true])));
        assert!(!guc_blt_map_ui4_surfaces(state, destination, &[first], false, Some(&[])));
        assert!(f.table_publication().is_empty());
        assert_eq!(f.table, before);
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[first], false, None));
        assert!(f.table_publication().is_empty());
        f.assert_guards();
    }

    #[test]
    fn legacy_full_init_invalidates_sparse_metadata_and_mono_scratch_stays_mapped() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let first = copy(surface(0x3000000));
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[first], false, None));
        assert_eq!(f.leaf(DIRECT_BLT_GPU_VA_SRC_BASE), state.src_phys | 3);
        assert!(direct_blt_init_ppgtt(state));
        assert_eq!(f.leaf(first.source.gpu), 0);
        super::super::clear_publication();
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[first], false, None));
        assert_eq!(f.table_publication(), vec![(0, DIRECT_BLT_PPGTT_BYTES)]);
        assert_eq!(f.leaf(first.source.gpu), first.source.phys | 3);
        assert_eq!(f.leaf(DIRECT_BLT_GPU_VA_SRC_BASE), state.src_phys | 3);
        super::super::clear_publication();
        assert!(guc_blt_map_ui4_surfaces(state, destination, &[], false, None));
        assert_eq!(f.leaf(first.source.gpu), 0);
        assert_eq!(f.leaf(DIRECT_BLT_GPU_VA_SRC_BASE), state.src_phys | 3);
        f.assert_guards();
    }

    fn assert_envelope(f: &Fixture, state: DirectBltState, body_dwords: usize, marker: u32) {
        let encoded = &f.batch[1..f.batch.len() - 1];
        assert_eq!(&encoded[..4], &[
            MI_STORE_DATA_IMM_GGTT_DW1, (DIRECT_BLT_GPU_VA_RESULT_BASE + 16) as u32, 0, BCS_BATCH_COOKIE,
        ]);
        assert_eq!(encoded[4], MI_ARB_CHECK | (1 << 8) | 1);
        assert_eq!(encoded[5], MI_FLUSH_DW | MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE | MI_FLUSH_DW_TLB_INVALIDATE);
        assert_eq!(encoded[6], (DIRECT_BLT_GPU_VA_RESULT_BASE + 24) as u32 | MI_FLUSH_DW_DEST_GGTT);
        assert_eq!(encoded[10], MI_ARB_CHECK | (1 << 8));
        let footer = 12 + body_dwords;
        assert_eq!(&encoded[footer..footer + 8], &[
            MI_FLUSH_DW | MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE,
            DIRECT_BLT_GPU_VA_RESULT_BASE as u32 | MI_FLUSH_DW_DEST_GGTT,
            (DIRECT_BLT_GPU_VA_RESULT_BASE >> 32) as u32,
            marker, 0, MI_ARB_CHECK, MI_BATCH_BUFFER_END, MI_NOOP,
        ]);
        assert!(encoded[footer + 8..].iter().all(|&word| word == 0));
        super::super::FLUSHES.with(|flushes| assert_eq!(&*flushes.borrow(), &[
            (state.batch_virt as usize, (footer + 8) * 4), (state.result_virt as usize, 4),
        ]));
        f.assert_guards();
    }

    #[test]
    fn mixed_plan_keeps_entry_tlb_boundary_ordered_body_and_post_sync_completion() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let clear = GucBcs0RgbaFill { x: 0, y: 0, width: 64, height: 64, color: 0x80402010 };
        let plan = BcsPlan::new(destination, &[
            BcsOp::Fill(clear), operation(surface(0x3000000), CopyBackend::Legacy),
            BcsOp::Fill(GucBcs0RgbaFill { x: 32, width: 16, ..clear }),
        ]).unwrap();
        assert_eq!(plan.batches().len(), 1);
        let batch = &plan.batches()[0];
        assert_eq!(guc_blt_encode_plan_batch(state, batch, 0xbc501234), Some((3, batch.copied_bytes())));
        assert_envelope(&f, state, batch.encoded_dwords(), 0xbc501234);
        let words = &f.batch[1 + 12..];
        assert_eq!((words[0] >> 22) & 0x7f, 0x44);
        assert_eq!((words[11] >> 22) & 0x7f, 0x53);
        assert_eq!((words[21] >> 22) & 0x7f, 0x44);
    }

    #[test]
    fn barrier_heavy_maximum_chunk_fits_actual_driver_buffer_with_guards() {
        let mut f = Fixture::new();
        let state = f.state();
        let destination = surface(0x5000000);
        let ops: Vec<_> = (0..136).map(|index| operation(
            surface(if index % 2 == 0 { 0x3000000 } else { 0x4000000 }), CopyBackend::PreferFast,
        )).collect();
        let plan = BcsPlan::new(destination, &ops).unwrap();
        assert_eq!(plan.batches().len(), 2);
        let first = &plan.batches()[0];
        assert_eq!(first.operation_count(), 135);
        assert_eq!(first.explicit_flushes(), 134);
        assert_eq!(first.encoded_dwords() + BCS_PLAN_ENVELOPE_DWORDS, 2040);
        assert!(guc_blt_encode_plan_batch(state, first, 0xbc50abcd).is_some());
        assert_envelope(&f, state, first.encoded_dwords(), 0xbc50abcd);
        super::super::clear_publication();
        let second = &plan.batches()[1];
        assert_eq!(second.operation_count(), 1);
        assert_eq!(second.explicit_flushes(), 0);
        assert!(guc_blt_encode_plan_batch(state, second, 0xbc50abce).is_some());
        assert_envelope(&f, state, second.encoded_dwords(), 0xbc50abce);
    }
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-bcs-driver-") as temporary:
        directory = Path(temporary)
        source, executable = directory / "tests.rs", directory / "tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            check=True,
            cwd=ROOT,
        )
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
