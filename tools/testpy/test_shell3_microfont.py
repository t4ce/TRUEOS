#!/usr/bin/env python3
"""Test production MetaFmt layout/diffs, CPU pixels and legacy mono masks."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = '''#![allow(dead_code, non_upper_case_globals)]
extern crate alloc;
'''
    for name in ('RgbaColor', 'SpecialRows', 'StripSide'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += re.search(r'^impl RgbaColor \{.*?^}', (ROOT/'src/shell3/shell3.rs').read_text(), re.M | re.S).group()
    source += f'''\n#[path = "{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;
use metafmtstr::MetaFmtStr;
#[path = "{ROOT}/src/shell3/update.rs"] mod update;
use update::SegmentUpdate;
const SpecialSeperator: char = '│';
const OPERATOR: char = '§';
{extract.item('src/shell3/shell3.rs', 'matrix_slots_meta')}
mod intel {{
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {{}}
'''
    for name in ('GucBcs0RgbaSurface', 'GucBcs0MonoGlyph'):
        source += extract.item('src/intel/copy/blt.rs', name)
    source += '''
}
#[derive(Clone, Copy)]
struct FrameRgbaView { virt: *const u8, byte_len: usize, width: u32, height: u32, pitch: u32, phys: u64, gpu: u64 }
mod show {
    const BACKGROUND: crate::RgbaColor = crate::RgbaColor::Gray;
    const FOREGROUND: crate::RgbaColor = crate::RgbaColor::White;
    mod cpu { use crate::FrameRgbaView;
'''
    source += extract.item('src/shell3/show/cpu.rs', 'paint_segment').replace('fn paint_segment', 'pub(super) fn paint_segment')
    source += '''
    }
    mod copy { use crate::FrameRgbaView;
'''
    source += extract.item('src/shell3/show/copy.rs', 'glyphs_for_update')
    source += '''
        #[test]
        fn mono_expansion_matches_cpu_pixels_colors_clipping_and_erasure() {
            for width in [20, 96] {
                for text in ["§éq─A", "q", "", "Hello §"] {
                    let mut pixels = vec![0x5Au8; 512 * 33];
                    let mut expanded = pixels.clone();
                    let view = FrameRgbaView { virt: pixels.as_mut_ptr(), byte_len: pixels.len(),
                        width, height: 33, pitch: 512, phys: 4096, gpu: 4096 };
                    let colors = (0..text.chars().count()).map(|i| Some(if i%2==0 { crate::RgbaColor::Pink } else { crate::RgbaColor::Green })).collect();
                    let update = crate::SegmentUpdate { row: crate::SpecialRows::PromtRow, side: crate::StripSide::Left,
                        offset: 1, remove: 8, text: text.into(), colors };
                    super::cpu::paint_segment(view, &update).unwrap();
                    let mut glyphs = Vec::new();
                    glyphs_for_update(view, &update, &mut glyphs);
                    assert!(!glyphs.is_empty());
                    for glyph in &glyphs {
                        assert_eq!(glyph.y, 22);
                        for y in 0..glyph.height { for x in 0..glyph.width {
                            let bit = glyph.mask[(y*2+x/8) as usize] & (0x80 >> (x%8)) != 0;
                            let rgba = if bit { glyph.foreground } else { glyph.background };
                            let offset = ((glyph.y+y)*512+(glyph.x+x)*4) as usize;
                            expanded[offset..offset+4].copy_from_slice(&rgba.to_le_bytes());
                        }}
                    }
                    assert_eq!(expanded, pixels, "text={text} width={width}");
                }
            }
        }
    }
}
#[test]
fn metadata_survives_layout_and_color_only_changes_redraw() {
    let green = [MetaFmtStr::new("§A").color(RgbaColor::Green).bold()];
    let pink = [MetaFmtStr::new("§A").color(RgbaColor::Pink)];
    let right = [MetaFmtStr::new("é").color(RgbaColor::Blue)];
    let line = update::fit_meta_strips(&green, &right, 6);
    assert_eq!(line, [('§',Some(RgbaColor::Green)),('A',Some(RgbaColor::Green)),(' ',None),(' ',None),(' ',None),('é',Some(RgbaColor::Blue))]);
    let previous = [line, vec![], vec![]];
    let current = [update::fit_meta_strips(&pink, &right, 6), vec![], vec![]];
    let patches = update::diff_rendered_lines(Some(&previous), &current);
    assert_eq!(patches.len(), 1);
    assert_eq!((patches[0].offset, patches[0].remove, patches[0].text.as_str()), (0,2,"§A"));
    assert_eq!(patches[0].colors, [Some(RgbaColor::Pink);2]);
    let no_bold = [MetaFmtStr::new("§A").color(RgbaColor::Green)];
    assert_eq!(update::fit_meta_strips(&green, &right, 6), update::fit_meta_strips(&no_bold, &right, 6));
}
#[test]
fn overflowing_strips_keep_their_own_colors_and_neutral_separator() {
    let left = [MetaFmtStr::new("§ABC").color(RgbaColor::Green)];
    let right = [MetaFmtStr::new("éXYZ").color(RgbaColor::Pink)];
    assert_eq!(update::fit_meta_strips(&left, &right, 5), [('§',Some(RgbaColor::Green)),('A',Some(RgbaColor::Green)),('│',None),('é',Some(RgbaColor::Pink)),('X',Some(RgbaColor::Pink))]);
    assert!(update::fit_meta_strips(&left, &right, 0).is_empty());
}
#[test]
fn active_slot_color_reaches_a_color_only_patch() {
    let ids = vec!["id".to_string()];
    let idle = matrix_slots_meta(&ids, None);
    let active = matrix_slots_meta(&ids, Some("id"));
    assert_eq!(idle[0].color, Some(RgbaColor::Pink));
    assert_eq!(active[0].color, Some(RgbaColor::White));
    assert_eq!(active[2].color, Some(RgbaColor::Pink));
    assert_eq!(active[3].color, Some(RgbaColor::Pink));
    let previous = [update::fit_meta_strips(&idle, &[], 8),vec![],vec![]];
    let current = [update::fit_meta_strips(&active, &[], 8),vec![],vec![]];
    assert_eq!(previous[0].iter().map(|c|c.0).collect::<String>(), current[0].iter().map(|c|c.0).collect::<String>());
    assert!(!update::diff_rendered_lines(Some(&previous), &current).is_empty());
    assert_eq!(matrix_slots_meta(&ids, Some("absent")), idle);
}
#[test]
fn each_back_buffer_gets_its_own_color_diff() {
    let white = [vec![('§',None)],vec![],vec![]];
    let pink = [vec![('§',Some(RgbaColor::Pink))],vec![],vec![]];
    assert_eq!(update::diff_rendered_lines(Some(&white), &pink).len(),1);
    assert!(update::diff_rendered_lines(Some(&pink), &pink).is_empty());
    assert_eq!(update::diff_rendered_lines(None, &pink).len(),1);
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
