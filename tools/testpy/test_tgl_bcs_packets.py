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
fn dma_flush(_: *mut u8, _: usize) {}
mod blt {
'''
    source += '\n'.join(extract.constant(BLT, name) for name in (
        'DIRECT_BLT_BATCH_BYTES', 'DIRECT_BLT_RESULT_BYTES',
        'DIRECT_BLT_PPGTT_PT_COUNT', 'DIRECT_BLT_PPGTT_LIMIT_BYTES',
        'DIRECT_BLT_GPU_VA_RESULT_BASE', 'XY_FAST_COPY_BLT_CMD',
        'XY_FAST_COPY_COLOR_DEPTH_32', 'MI_FLUSH_DW',
        'MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE', 'MI_FLUSH_DW_DEST_GGTT',
        'MI_ARB_CHECK', 'MI_BATCH_BUFFER_END', 'MI_NOOP',
        'MI_STORE_DATA_IMM_GGTT_DW1', 'BCS_BATCH_COOKIE', 'BCS_RING_COOKIE',
        'DIRECT_BLT_RING_BYTES', 'MI_BATCH_BUFFER_START_GEN8', 'MI_BATCH_GGTT',
        'DIRECT_BLT_GPU_VA_BATCH_BASE',
    ))
    source += '\n' + '\n'.join(extract.item(BLT, name) for name in (
        'DirectBltState', 'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy',
        'guc_blt_valid_surface', 'guc_blt_valid_copy',
        'guc_blt_physical_ranges_overlap', 'guc_blt_encode_ui4_copy_batch',
        'guc_blt_append_ring_batch_start',
    ))
    source += r'''
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
    assert_eq!(&batch[..4], &[0x10400002, 0x00B50010, 0, 0xBC504242]);
    assert_eq!(&batch[4..12], &[0x13004003, 0x00B50004, 0, 0xBC500001,
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
    assert_eq!(&batch[4..14], &[0x50800008, 0x03000800, 0, 0x00020200,
        0x200000, 0, 0, 2048, 0x100000, 0]);
    assert_eq!(&batch[14..22], &[0x13004003, 0x00B50004, 0, 0xBC500002,
        0, 0x02800000, 0x05000000, 0]);
}
#[test]
fn ring_entry_writes_before_batch_and_wraps_without_overrun() {
    let mut ring = vec![0xdeadbeefu32; 1026];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.ring_virt = ring.as_mut_ptr().cast();
    assert_eq!(guc_blt_append_ring_batch_start(state, 4064), 0);
    assert_eq!(&ring[1016..1024], &[0x10400002, 0x00B50008, 0, 0xBC505247,
        0x18800001, 0x00B40000, 0, 0x02800000]);
    assert_eq!(&ring[1024..], &[0xdeadbeef, 0xdeadbeef]);
    assert_eq!(guc_blt_append_ring_batch_start(state, 0), 32);
    assert_eq!(&ring[..8], &ring[1016..1024]);
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
