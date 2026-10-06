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
const MIN_COLUMNS: usize = 20;
const MIN_ROWS: usize = 5;
{extract.item('src/shell3/shell3.rs', 'matrix_slots_meta')}
mod intel {{
    pub fn dma_cache_flush_range(_: *const u8, _: usize) {{}}
    pub fn active_scanout_dimensions() -> Option<(u32,u32)> {{ Some((2560,1440)) }}
'''
    for name in ('GucBcs0RgbaSurface', 'GucBcs0MonoGlyph'):
        source += extract.item('src/intel/copy/blt.rs', name)
    source += '''
}
#[derive(Clone, Copy)]
struct FrameRgbaView { virt: *const u8, byte_len: usize, width: u32, height: u32, pitch: u32, phys: u64, gpu: u64 }
mod ui4 { pub fn set_window_placement(_:u32,_:u32,_:crate::show::Placement)->Result<(),()> { Ok(()) } }
mod show {
    const BACKGROUND: crate::RgbaColor = crate::RgbaColor::Gray;
    const FOREGROUND: crate::RgbaColor = crate::RgbaColor::White;
'''
    source += """
    const OWNER: u32 = 1;
    #[derive(Clone, Copy)] pub(crate) struct Placement { width:u32,height:u32 }
    fn window_resize_state(_:u32,_:u32)->Result<(Placement,u64),()> {
        Ok((Placement {width:480,height:55},1))
    }
    struct Ui4Surface { width:u32,height:u32,window:u32,scale:u32,broker_resized:bool,
        frame_contents:[Option<u8>;2], clear_buffers:[bool;2] }
    struct Show { surface:Option<Ui4Surface>,font_scale:u32 }
    impl Show {
"""
    source += re.search(r'    pub\(crate\) fn set_font_scale\(.*?^    }', (ROOT/'src/shell3/show/show.rs').read_text(), re.M | re.S).group()
    source += "}\n"
    source += extract.item('src/shell3/show/show.rs', 'show_extent')
    source += """
    #[test] fn toggle_keeps_extent_except_minimum_and_clears_both_histories() {
        for (width,height,expected) in [(480,55,(480,110)),(120,55,(240,110)),(601,301,(601,301))] {
            let surface = Ui4Surface {width,height,window:7,scale:1,broker_resized:false,
                frame_contents:[Some(1),Some(1)],clear_buffers:[false;2]};
            let mut show = Show {surface:Some(surface),font_scale:1};
            assert_eq!(show.set_font_scale(2,80,5).unwrap(),expected);
            let surface = show.surface.as_ref().unwrap();
            assert_eq!(surface.window,7);
            assert_eq!(surface.frame_contents,[None,None]);
            assert_eq!(surface.clear_buffers,[true;2]);
            assert_eq!(surface.scale,2);
        }
        let mut show = Show {surface:None,font_scale:1};
        assert_eq!(show.set_font_scale(2,80,5).unwrap(),(480,110));
    }
    mod cpu { use crate::FrameRgbaView;
"""
    source += extract.item('src/shell3/show/cpu.rs', 'paint_segment').replace('fn paint_segment', 'pub(super) fn paint_segment')
    source += '''
    }
    mod copy { use crate::FrameRgbaView;
'''
    source += extract.item('src/shell3/show/copy.rs', 'glyphs_for_update')
    source += '''
        #[test]
        fn mono_expansion_matches_cpu_pixels_colors_clipping_and_erasure() {
            for scale in [1, 2] {
            for height in [55, 66] {
            for width in [20, 96] {
                for (row, text) in [(crate::SpecialRows::PromtRow,"§éq─A"), (crate::SpecialRows::PromtRow,"q"), (crate::SpecialRows::PromtRow,""), (crate::SpecialRows::PromtRow,"Hello §"), (crate::SpecialRows::MatrixRow(0),"⠁⠂⡀⢀⣿⠀"), (crate::SpecialRows::MatrixRow(1),"net")] {
                    let mut pixels = vec![0x5Au8; 512 * 66];
                    let mut expanded = pixels.clone();
                    let view = FrameRgbaView { virt: pixels.as_mut_ptr(), byte_len: pixels.len(),
                        width, height, pitch: 512, phys: 4096, gpu: 4096 };
                    let colors = (0..text.chars().count()).map(|i| Some(match i%4 {0=>crate::RgbaColor::Pink,1=>crate::RgbaColor::Green,2=>crate::RgbaColor::Underlined {foreground:crate::RgbaColor::Pink.rgba()},_=>crate::RgbaColor::Terminal {foreground:[255,160,90,255],background:[20,45,60,255],underline:true}})).collect();
                    let update = crate::SegmentUpdate { row, side: crate::StripSide::Left,
                        offset: 1, remove: 8, text: text.into(), colors };
                    super::cpu::paint_segment(view, &update, scale).unwrap();
                    let mut glyphs = Vec::new();
                    glyphs_for_update(view, &update, scale, &mut glyphs);
                    let expected_y = match row {crate::SpecialRows::MatrixRow(index)=>(index as u32 + 3)*11*scale,_=>22*scale};
                    assert_eq!(glyphs.is_empty(),expected_y>=height);
                    for glyph in &glyphs {
                        assert_eq!(glyph.y, expected_y);
                        for y in 0..glyph.height { for x in 0..glyph.width {
                            let bit = glyph.mask[(y*2+x/8) as usize] & (0x80 >> (x%8)) != 0;
                            let rgba = if bit { glyph.foreground } else { glyph.background };
                            let offset = ((glyph.y+y)*512+(glyph.x+x)*4) as usize;
                            expanded[offset..offset+4].copy_from_slice(&rgba.to_le_bytes());
                        }}
                    }
                    assert_eq!(expanded, pixels, "text={text} width={width} height={height} scale={scale}");
                }
            }
            }
            }
        }
        #[test]
        fn doubled_section_sign_replicates_each_source_pixel_into_four_pixels() {
            let view = FrameRgbaView { virt: core::ptr::null(), byte_len: 240*110*4,
                width: 240, height: 110, pitch: 960, phys: 4096, gpu: 4096 };
            let update = crate::SegmentUpdate { row: crate::SpecialRows::TitleRow,
                side: crate::StripSide::Left, offset: 0, remove: 1, text: "§".into(), colors: vec![] };
            let mut glyphs = Vec::new();
            glyphs_for_update(view, &update, 2, &mut glyphs);
            let glyph = &glyphs[0];
            assert_eq!((glyph.width, glyph.height), (12,22));
            let bits = microfont::font_pixels(microfont::glyph_byte('§'));
            for y in 0..22 { for x in 0..12 {
                let bit = (y/2)*6+x/2;
                let expected = bit < 64 && bits & (1u64 << (63-bit)) != 0;
                let actual = glyph.mask[y*2+x/8] & (0x80 >> (x%8)) != 0;
                assert_eq!(actual, expected, "pixel={x},{y}");
            }}
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
    let hover = [MetaFmtStr::new("§A").color(RgbaColor::Green).underline()];
    let line=update::fit_meta_strips(&hover,&right,6);
    assert_eq!(line[0].1.unwrap().rgba(),RgbaColor::Green.rgba());
    assert!(line[0].1.unwrap().underline());assert_eq!(line[0].1.unwrap().background(),None);
    assert!(!line[5].1.unwrap().underline());
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

#[test]
fn matrix_transcripts_render_newest_first_and_clear_when_selection_changes() {
    let title=[MetaFmtStr::new("TrueOS §")];let prompt=[MetaFmtStr::new("#")];
    let make=|rows,lines:&[String],generation|update::Snapshot::new((12,rows),0,[(&title,&[]),(&[],&[]),(&prompt,&[])],12).with_matrix(lines,generation);
    let history=vec!["online".into(),"pause".into(),"stop".into()];
    let snapshot=make(5,&history,1);let lines=snapshot.rendered_lines();
    assert_eq!(lines.len(),5);assert_eq!(lines[3].iter().map(|cell|cell.0).collect::<String>(),"online      ");
    assert_eq!(lines[4].iter().map(|cell|cell.0).collect::<String>(),"pause       ");
    let grown=make(6,&history,1).rendered_lines();
    assert_eq!(grown[3].iter().map(|cell|cell.0).collect::<String>(),"online      ");
    let blank=make(5,&[],1);let patches=update::build_updates(&snapshot,&blank,&[]);
    assert_eq!(patches.segments.len(),2);
    assert_eq!(patches.segments[0].row,SpecialRows::MatrixRow(0));
    assert_eq!(patches.segments[1].row,SpecialRows::MatrixRow(1));
    assert!(patches.segments.iter().all(|patch|patch.text.chars().all(|ch|ch==' ')));
    let diff=update::diff_rendered_lines(Some(&lines),&blank.rendered_lines());assert_eq!(diff,patches.segments);
}

#[test]
fn matrix_fills_current_resize_height_and_paints_only_occupied_text() {
    let title=[MetaFmtStr::new("TrueOS § 12:34")];let legend=[MetaFmtStr::new("[online peer dl]")];
    let prompt=[MetaFmtStr::new("#")];
    let history=(0..20).map(|index|format!("cmd{} pause stop",index)).collect::<Vec<_>>();
    let snapshot=update::Snapshot::new((640,196),0,[(&title,&legend),(&[],&[]),(&prompt,&[])],640).with_matrix(&history,1);
    let lines=snapshot.rendered_lines();assert_eq!(lines.len(),196);
    assert_eq!(lines[3].iter().take(5).map(|cell|cell.0).collect::<String>(),"cmd0 ");
    assert!(lines[22].iter().map(|cell|cell.0).collect::<String>().starts_with("cmd19"));
    assert!(lines[23..].iter().flatten().all(|cell|cell.0==' '));
    let resized=|rows|update::Snapshot::new((640,rows),1,[(&title,&legend),(&[],&[]),(&prompt,&[])],640).with_matrix(&history,1).rendered_lines();
    assert_eq!(resized(25).len(),25);
    let shrunk=resized(12);assert_eq!(shrunk.len(),12);
    assert!(shrunk[3].iter().map(|cell|cell.0).collect::<String>().starts_with("cmd0 "));
    let updates=update::diff_rendered_lines(None,&lines);
    let cells=updates.iter().map(|patch|patch.text.chars().count()).sum::<usize>();
    let occupied=lines.iter().flatten().filter(|cell|cell.0!=' ').count();
    assert_eq!(cells,occupied);assert!(cells<400);assert!(cells.div_ceil(64)<=7);
    assert!(updates.iter().all(|patch|!patch.text.contains(' ')));
    let empty=update::Snapshot::new((640,196),0,[(&title,&legend),(&[],&[]),(&prompt,&[])],640).with_matrix(&[],2).rendered_lines();
    assert!(update::diff_rendered_lines(None,&empty).iter().all(|patch|!matches!(patch.row,SpecialRows::MatrixRow(_))));
    assert!(update::diff_rendered_lines(Some(&lines),&empty).iter().any(|patch|matches!(patch.row,SpecialRows::MatrixRow(_))&&patch.remove>0));
    // Shorter current row lists must also erase old text, not leave it behind.
    assert!(update::diff_rendered_lines(Some(&lines),&empty[..3]).iter().any(|patch|matches!(patch.row,SpecialRows::MatrixRow(_))&&patch.remove>0));
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
