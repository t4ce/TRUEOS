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
        'DIRECT_BLT_BATCH_BYTES', 'DIRECT_BLT_RESULT_BYTES', 'GUC_BLT_UI4_MAX_COPIES', 'BCS0_GGTT_BASE', 'BCS0_GGTT_LIMIT',
        'DIRECT_BLT_PPGTT_PT_COUNT', 'DIRECT_BLT_PPGTT_LIMIT_BYTES', 'DIRECT_BLT_PPGTT_BYTES',
        'DIRECT_BLT_GPU_VA_RESULT_BASE', 'XY_FAST_COPY_BLT_CMD',
        'XY_FAST_COPY_COLOR_DEPTH_32', 'MI_FLUSH_DW',
        'MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE', 'MI_FLUSH_DW_DEST_GGTT', 'MI_FLUSH_DW_TLB_INVALIDATE',
        'MI_ARB_CHECK', 'MI_BATCH_BUFFER_END', 'MI_NOOP',
        'MI_STORE_DATA_IMM_GGTT_DW1', 'BCS_BATCH_COOKIE', 'BCS_RING_COOKIE',
        'DIRECT_BLT_RING_BYTES', 'MI_BATCH_BUFFER_START_GEN8', 'MI_BATCH_GGTT',
        'DIRECT_BLT_GPU_VA_BATCH_BASE', 'DIRECT_BLT_GPU_VA_SRC_BASE',
        'DIRECT_BLT_GPU_VA_DST_BASE', 'DIRECT_BLT_SMOKE_MARKER',
        'DIRECT_BLT_COPY_BYTES', 'GUC_BCS0_MONO_MAX_GLYPHS',
    ))
    source += '\n' + '\n'.join(extract.item(BLT, name) for name in (
        'DirectBltState', 'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy',
        'guc_blt_valid_surface', 'guc_blt_valid_copy', 'guc_blt_map_ui4_surfaces',
        'guc_blt_physical_ranges_overlap', 'guc_blt_gpu_ranges_overlap', 'guc_blt_encode_ui4_copy_batch',
        'guc_blt_append_ring_batch_start', 'boot_bcs0_legacy_ring_words',
        'guc_blt_valid_fill', 'guc_blt_encode_ui4_fill_batch',
        'guc_blt_valid_legacy_copy', 'guc_blt_encode_legacy_copy_batch', 'guc_blt_encode_copy_batch',
        'GucBcs0MonoGlyph', 'guc_blt_valid_mono_glyph', 'guc_blt_encode_mono_batch',
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
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
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
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
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
fn legacy_rgba_copies_use_byte_pitches_nonzero_origins_and_ordered_retirement() {
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
    let mut result = vec![0u32; 1024];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    let src = GucBcs0RgbaSurface { width: 40, height: 22, pitch_bytes: 256, bytes: 8192, ..surface(0x100000) };
    let dst = GucBcs0RgbaSurface { width: 64, height: 33, pitch_bytes: 320, bytes: 12288, ..surface(0x200000) };
    let copy = GucBcs0RgbaCopy { source: src, source_x: 6, source_y: 11,
        destination_x: 12, destination_y: 22, width: 18, height: 11 };
    assert_eq!(guc_blt_encode_legacy_copy_batch(state, dst, &[copy, copy], 0xBC500004), Some((2, 1584)));
    assert_eq!(&batch[12..22], &[0x54F00008, 0x03CC0140, 0x0016000C, 0x0021001E,
        0x200000, 0, 0x000B0006, 256, 0x100000, 0]);
    assert_eq!(&batch[22..32], &batch[12..22]);
    assert_eq!(&batch[32..40], &[0x13004003, 0x01AD0004, 0, 0xBC500004, 0, 0x02800000, 0x05000000, 0]);
    assert!(!guc_blt_valid_legacy_copy(GucBcs0RgbaSurface { pitch_bytes: 32768, bytes: 32768*33, ..dst }, copy));
    assert!(!guc_blt_valid_legacy_copy(dst, GucBcs0RgbaCopy { source: dst, ..copy }));
    let tall = GucBcs0RgbaSurface { height: 32768, bytes: 320*32768, ..dst };
    assert!(!guc_blt_valid_legacy_copy(tall, GucBcs0RgbaCopy { destination_y: 32757, ..copy }));
}
#[test]
fn full_gameboy_width_fits_both_copy_backends_and_retires_after_last_column() {
    let src = GucBcs0RgbaSurface { width: 160, height: 144, pitch_bytes: 640,
        bytes: 92160, ..surface(0x100000) };
    let dst = GucBcs0RgbaSurface { phys: 0x200000, gpu: 0x200000, ..src };
    let copies: Vec<_> = (0..160).map(|x| GucBcs0RgbaCopy { source: src,
        source_x: x, source_y: 0, destination_x: x, destination_y: 0,
        width: 1, height: 144 }).collect();
    assert_eq!(GUC_BLT_UI4_MAX_COPIES, 160);
    for legacy in [false, true] {
        let words = DIRECT_BLT_BATCH_BYTES / 4;
        let mut batch = vec![0xdeadbeefu32; words + 2];
        let mut result = vec![0u32; DIRECT_BLT_RESULT_BYTES / 4];
        let mut state: DirectBltState = unsafe { core::mem::zeroed() };
        state.batch_virt = batch.as_mut_ptr().cast();
        state.result_virt = result.as_mut_ptr().cast();
        assert_eq!(guc_blt_encode_copy_batch(state, dst, &copies, 0xBC500160, legacy),
            Some((160, 92160)));
        for x in 0..160usize {
            let offset = 12 + x * 10;
            assert_eq!(batch[offset], if legacy { 0x54F00008 } else { 0x50800008 });
            assert_eq!(batch[offset + 2], x as u32);
            assert_eq!(batch[offset + 3], (144 << 16) | (x as u32 + 1));
            assert_eq!(batch[offset + 6], x as u32);
        }
        let end = 12 + 160 * 10;
        assert_eq!(&batch[end..end + 8], &[0x13004003, 0x01AD0004, 0,
            0xBC500160, 0, 0x02800000, 0x05000000, 0]);
        assert_eq!(&batch[words..], &[0xdeadbeef, 0xdeadbeef]);
        let mut oversized = copies.clone();
        oversized.push(copies[0]);
        assert_eq!(guc_blt_encode_copy_batch(state, dst, &oversized, 1, legacy), None);
    }
}
#[test]
fn legacy_mono_has_colors_word_rows_aligned_source_slots_and_retirement() {
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
    let mut result = vec![0u32; 1024];
    let mut masks = vec![0u8; 4096];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    state.src_virt = masks.as_mut_ptr();
    let mut glyph = GucBcs0MonoGlyph { x: 6, y: 11, width: 6, height: 11,
        mask: [0;64], foreground: 0xFFB469FF, background: 0xFF808080 };
    glyph.mask[0] = 0xA8;
    glyph.mask[20] = 0x80;
    let dst = GucBcs0RgbaSurface { width: 40, height: 33, pitch_bytes: 256, bytes: 12288, ..surface(0x200000) };
    assert_eq!(guc_blt_encode_mono_batch(state, dst, &[glyph,glyph], 0xBC500005), Some((2,528)));
    assert_eq!(&batch[12..22], &[0x55300008,0x03CC0100,0x000B0006,0x0016000C,
        0x200000,0,0x01AE0000,0,0xFF808080,0xFFB469FF]);
    assert_eq!(batch[28],0x01AE0040);
    assert_eq!(&masks[..64], &glyph.mask);
    assert_eq!(&masks[64..128], &glyph.mask);
    assert_eq!(&batch[32..40], &[0x13004003,0x01AD0004,0,0xBC500005,0,0x02800000,0x05000000,0]);
    assert_eq!(guc_blt_encode_mono_batch(state,dst,&[glyph;64],7),Some((64,64*264)));
    assert_eq!(guc_blt_encode_mono_batch(state,dst,&[glyph;65],7),None);
    assert!(!guc_blt_valid_mono_glyph(dst,&GucBcs0MonoGlyph { width:17,..glyph }));
    assert!(!guc_blt_valid_mono_glyph(dst,&GucBcs0MonoGlyph { height:33,..glyph }));
    assert!(!guc_blt_valid_mono_glyph(dst,&GucBcs0MonoGlyph { x:u32::MAX,..glyph }));
    assert!(!guc_blt_valid_mono_glyph(dst,&GucBcs0MonoGlyph { y:30,..glyph }));
}
#[test]
fn fast_color_has_32bpp_pitch_minus_one_and_ordered_retirement() {
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
    let mut result = vec![0u32; 1024];
    let mut state: DirectBltState = unsafe { core::mem::zeroed() };
    state.batch_virt = batch.as_mut_ptr().cast();
    state.result_virt = result.as_mut_ptr().cast();
    let dst = GucBcs0RgbaSurface { width: 17, height: 3, pitch_bytes: 128, ..surface(0x200000) };
    assert_eq!(guc_blt_encode_ui4_fill_batch(state, dst, 0x80402010, 0xBC500003), Some((1, 204)));
    assert_eq!(&batch[12..23], &[0x51100009, 127, 0, 0x00030011, 0x200000, 0, 0, 0x80402010, 0, 0, 0]);
    assert_eq!(&batch[23..31], &[0x13004003, 0x01AD0004, 0, 0xBC500003, 0, 0x02800000, 0x05000000, 0]);
    assert_eq!(&batch[4..12], &[0x02800101, 0x13044003, 0x01AD001C, 0, 0, 0, 0x02800100, 0]);
    assert!(!guc_blt_valid_fill(GucBcs0RgbaSurface { height: 32768, bytes: 128*32768, ..dst }));
    assert!(!guc_blt_valid_fill(GucBcs0RgbaSurface { bytes: 323, ..dst }));
    assert!(!guc_blt_valid_fill(GucBcs0RgbaSurface { gpu: BCS0_GGTT_BASE, ..dst }));
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
    let mut batch = vec![0u32; DIRECT_BLT_BATCH_BYTES / 4];
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
