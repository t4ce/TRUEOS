#!/usr/bin/env python3
"""Check production BCS packet encoders against TGL Vol 2a, without GPU access."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
BLT = 'src/intel/copy/blt.rs'


def main():
    source = '''#![allow(dead_code)]
const WARM_ALIGN: usize = 4096;
const GEN8_PAGE_PRESENT: u64 = 1;
fn dma_flush(_: *mut u8, _: usize) {}
mod blt {
'''
    source += '\n'.join(extract.constant(BLT, name) for name in (
        'DIRECT_BLT_BATCH_BYTES', 'DIRECT_BLT_RESULT_BYTES', 'BCS0_GGTT_BASE', 'BCS0_GGTT_LIMIT',
        'DIRECT_BLT_PPGTT_PT_COUNT', 'DIRECT_BLT_PPGTT_LIMIT_BYTES', 'DIRECT_BLT_PPGTT_BYTES',
        'DIRECT_BLT_GPU_VA_RESULT_BASE', 'XY_FAST_COPY_BLT_CMD',
        'XY_FAST_COPY_COLOR_DEPTH_32', 'MI_FLUSH_DW',
        'MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE', 'MI_FLUSH_DW_DEST_GGTT', 'MI_FLUSH_DW_TLB_INVALIDATE',
        'MI_ARB_CHECK', 'MI_BATCH_BUFFER_END', 'MI_NOOP',
        'MI_STORE_DATA_IMM_GGTT_DW1', 'BCS_BATCH_COOKIE', 'BCS_RING_COOKIE',
        'DIRECT_BLT_RING_BYTES', 'MI_BATCH_BUFFER_START_GEN8', 'MI_BATCH_GGTT',
        'DIRECT_BLT_GPU_VA_BATCH_BASE', 'DIRECT_BLT_GPU_VA_SRC_BASE',
        'DIRECT_BLT_GPU_VA_DST_BASE', 'DIRECT_BLT_SMOKE_MARKER',
    ))
    source += '\n' + '\n'.join(extract.item(BLT, name) for name in (
        'DirectBltState', 'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy',
        'guc_blt_valid_surface', 'guc_blt_valid_copy', 'guc_blt_map_ui4_surfaces',
        'guc_blt_physical_ranges_overlap', 'guc_blt_gpu_ranges_overlap', 'guc_blt_encode_ui4_copy_batch',
        'guc_blt_append_ring_batch_start', 'boot_bcs0_legacy_ring_words',
    ))
    source += r'''
static MAPPINGS: std::sync::Mutex<Vec<(u64, u64)>> = std::sync::Mutex::new(Vec::new());
fn direct_blt_map_ppgtt_region(_: DirectBltState, gpu: u64, _: u64, _: usize, flags: u64) -> bool {
    MAPPINGS.lock().unwrap().push((gpu, flags)); true
}
#[test]
fn device_written_snapshot_source_uses_uc_without_changing_ordinary_sources() {
    let state: DirectBltState = unsafe { core::mem::zeroed() };
    let copy = GucBcs0RgbaCopy { source: surface(0x100000), source_x: 0,
        source_y: 0, destination_x: 0, destination_y: 0, width: 512, height: 2 };
    assert!(guc_blt_map_ui4_surfaces(state, surface(0x200000), &[copy], true));
    assert!(guc_blt_map_ui4_surfaces(state, surface(0x200000), &[copy], false));
    assert_eq!(*MAPPINGS.lock().unwrap(), [(0x200000, 0x1b), (0x100000, 0x1b),
        (0x200000, 0x1b), (0x100000, 3)]);
}
fn surface(phys: u64) -> GucBcs0RgbaSurface {
    GucBcs0RgbaSurface { phys, gpu: phys, bytes: 4096, width: 512,
        height: 2, pitch_bytes: 2048 }
}
#[test]
fn first_rung_contains_entry_store_flush_arbitration_and_end() {
    let mut batch = vec![0u32; 1024];
    let mut result = vec![0u32; 1024];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    assert_eq!(guc_blt_encode_ui4_copy_batch(state, surface(0x200000), &[],
        0xBC500001), Some((0, 0)));
    // PRM: MI_FLUSH_DW opcode 26h, QWord immediate, GGTT bit in address.
    assert_eq!(&batch[..4], &[0x10400002, 0x01AD0010, 0, 0xBC504242]);
    assert_eq!(&batch[4..12], &[0x02800101, 0x13044003, 0x01AD001C,
        0, 0, 0, 0x02800100, 0]);
    assert_eq!(&batch[12..20], &[0x13004003, 0x01AD0004, 0, 0xBC500001,
        0, 0x02800000, 0x05000000, 0]);
}
#[test]
fn linear_rgba_packet_keeps_reserved_bits_zero_and_orders_completion() {
    let mut batch = vec![0u32; 1024];
    let mut result = vec![0u32; 1024];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    let copy = GucBcs0RgbaCopy { source: surface(0x100000), source_x: 0,
        source_y: 0, destination_x: 0, destination_y: 0, width: 512, height: 2 };
    assert_eq!(guc_blt_encode_ui4_copy_batch(state, surface(0x200000), &[copy],
        0xBC500002), Some((1, 4096)));
    assert_eq!(&batch[12..22], &[0x50800008, 0x03000800, 0, 0x00020200,
        0x200000, 0, 0, 2048, 0x100000, 0]);
    assert_eq!(&batch[22..30], &[0x13004003, 0x01AD0004, 0, 0xBC500002,
        0, 0x02800000, 0x05000000, 0]);
}
#[test]
fn ring_entry_writes_before_batch_and_wraps_without_overrun() {
    let mut ring = vec![0xdeadbeefu32; 1026];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.ring_virt = ring.as_mut_ptr().cast();
    assert_eq!(guc_blt_append_ring_batch_start(state, 4064), 0);
    assert_eq!(&ring[1016..1024], &[0x10400002, 0x01AD0008, 0, 0xBC505247,
        0x18800001, 0x01AC0000, 0, 0x02800000]);
    assert_eq!(&ring[1024..], &[0xdeadbeef, 0xdeadbeef]);
    assert_eq!(guc_blt_append_ring_batch_start(state, 0), 32);
    assert_eq!(&ring[..8], &ring[1016..1024]);
}
#[test]
fn legacy_probe_is_one_pixel_between_ring_entry_and_ordered_completion() {
    let words = boot_bcs0_legacy_ring_words();
    assert_eq!(&words[..4], &[0x10400002, 0x01AD0008, 0, 0xBC505247]);
    assert_eq!(&words[4..14], &[0x54F00008, 0x03CC0010, 0, 0x00010001,
        0x01AF0000, 0, 0, 16, 0x01AE0000, 0]);
    assert_eq!(&words[14..], &[0x13004003, 0x01AD0004, 0, 0xC0DEBC50,
        0, 0x02800000]);
}
#[test]
fn disjoint_physical_storage_cannot_alias_gpu_addresses_or_controls() {
    let dst = surface(0x200000);
    let mut copy = GucBcs0RgbaCopy { source: surface(0x100000), source_x: 0,
        source_y: 0, destination_x: 0, destination_y: 0, width: 512, height: 2 };
    assert!(guc_blt_valid_copy(dst, copy));
    copy.source.gpu = dst.gpu;
    assert!(!guc_blt_valid_copy(dst, copy));
    copy.source = surface(BCS0_GGTT_BASE);
    assert!(!guc_blt_valid_surface(copy.source));
    copy.source = surface(0x100000);
    copy.source.phys = u64::MAX & !4095;
    assert!(!guc_blt_valid_surface(copy.source));
    copy.source = dst;
    assert!(!guc_blt_valid_copy(dst, copy));
}
#[test]
fn padded_rows_and_nonzero_origins_preserve_rectangle_geometry() {
    let mut batch = vec![0u32; 1024];
    let mut result = vec![0u32; 1024];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    let mut src = surface(0x100000);
    src.width = 17; src.pitch_bytes = 128;
    let mut dst = surface(0x200000);
    dst.width = 20; dst.pitch_bytes = 256;
    let copy = GucBcs0RgbaCopy { source: src, source_x: 3, source_y: 1,
        destination_x: 5, destination_y: 0, width: 7, height: 1 };
    assert_eq!(guc_blt_encode_ui4_copy_batch(state, dst, &[copy], 9), Some((1, 28)));
    assert_eq!(&batch[12..22], &[0x50800008, 0x03000100, 5, 0x0001000c,
        0x200000, 0, 0x00010003, 128, 0x100000, 0]);
}
#[test]
fn pitch_cannot_spill_into_reserved_bits() {
    let mut s = surface(0x100000);
    s.bytes = 131072;
    s.pitch_bytes = 65532;
    assert!(guc_blt_valid_surface(s));
    s.pitch_bytes = 65536;
    assert!(!guc_blt_valid_surface(s));
}
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-bcs-packets-') as temporary:
        path = Path(temporary)
        (path / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '-o', str(path / 'tests')], cwd=ROOT, check=True)
        subprocess.run([str(path / 'tests')], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
