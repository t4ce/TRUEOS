#!/usr/bin/env python3
"""Exercise Shell3's real glyph painter and BCS patch geometry with MicroFont."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = '''#![allow(dead_code)]
mod intel {
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {}
'''
    for name in ('GucBcs0RgbaSurface', 'GucBcs0RgbaCopy'):
        source += extract.item('src/intel/copy/blt.rs', name)
    source += '''
}
#[derive(Clone, Copy)]
struct FrameRgbaView { virt: *const u8, byte_len: usize, width: u32, height: u32, pitch: u32, phys: u64, gpu: u64 }
enum SpecialRows { TitleRow, StatusRow, PromtRow }
struct SegmentUpdate { row: SpecialRows, offset: usize, remove: usize, text: String }
mod show {
    struct Color([u8;4]);
    impl Color { fn rgba(self)->[u8;4] { self.0 } }
    const BACKGROUND: Color = Color([128,128,128,255]);
    const FOREGROUND: Color = Color([255;4]);
    mod cpu {
        use crate::FrameRgbaView;
'''
    source += extract.item('src/shell3/show/cpu.rs', 'paint_segment')
    source += '''
        #[test]
        fn operator_and_following_cell_survive_with_padding_and_partial_repaint() {
            let mut pixels = vec![0x5Au8; 128 * 33];
            let view = FrameRgbaView { virt: pixels.as_mut_ptr(), byte_len: pixels.len(), width: 24,
                height: 33, pitch: 128, phys: 4096, gpu: 4096 };
            let update = crate::SegmentUpdate { row: crate::SpecialRows::PromtRow,
                offset: 1, remove: 0, text: "§é".into() };
            paint_segment(view, &update).unwrap();
            let mut mask = vec![0u8; 12 * 11];
            microfont::stamp_bytes(&mut mask,12,11,0,0,&[0xF5,0x82],1).unwrap();
            for y in 0..33 { for x in 0..32 {
                let expected = if (22..33).contains(&y) && (6..18).contains(&x) {
                    if mask[(y-22)*12+x-6] == 1 { [255;4] } else { [128,128,128,255] }
                } else { [0x5A;4] };
                assert_eq!(&pixels[y*128+x*4..y*128+x*4+4], &expected);
            }}
            let erase = crate::SegmentUpdate { row: crate::SpecialRows::PromtRow,
                offset: 1, remove: 2, text: "".into() };
            paint_segment(view, &erase).unwrap();
            for y in 22..33 { for x in 6..18 {
                assert_eq!(&pixels[y*128+x*4..y*128+x*4+4], &[128,128,128,255]);
            }}
        }
    }
    mod copy {
        use crate::FrameRgbaView;
'''
    for name in ('copy_for_update', 'bcs_surface'):
        source += extract.item('src/shell3/show/copy.rs', name)
    source += '''
        #[test]
        fn unicode_patch_uses_character_columns_and_clips_right_edge() {
            let view = FrameRgbaView { virt: core::ptr::null(), byte_len: 128*33, width: 20,
                height: 33, pitch: 128, phys: 4096, gpu: 4096 };
            let update = crate::SegmentUpdate { row: crate::SpecialRows::PromtRow,
                offset: 2, remove: 0, text: "§é".into() };
            let copy = copy_for_update(view, view, &update).unwrap();
            assert_eq!((copy.source_x,copy.destination_x,copy.source_y,copy.destination_y,copy.width,copy.height),
                (12,12,22,22,8,11));
        }
    }
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-microfont-') as temporary:
        path = Path(temporary)
        subprocess.run(['rustc', '--edition=2024', '--crate-name', 'microfont', '--crate-type', 'rlib',
                        str(ROOT / 'vendor/microfont/src/lib.rs'), '-o', str(path / 'libmicrofont.rlib')], check=True)
        (path / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '--extern', f'microfont={path / "libmicrofont.rlib"}', '-o', str(path / 'tests')], check=True)
        subprocess.run([str(path / 'tests')], check=True)


if __name__ == '__main__':
    main()
