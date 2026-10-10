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
#[path = "{ROOT}/src/shell3/transition.rs"] mod transition;
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
    const BACKGROUND: [u8;4] = crate::update::MATRIX_BACKGROUND;
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
    source += extract.item('src/shell3/show/copy.rs', 'glyph_for_cell')
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
                    glyphs_for_update(view, &update, scale, &mut glyphs, &[], &[false; 2]);
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
            glyphs_for_update(view, &update, 2, &mut glyphs, &[], &[false; 2]);
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
fn blank_cursor_carries_blink_and_only_its_cell_changes_per_phase() {
    let cursor=[MetaFmtStr::new(" ").color(RgbaColor::Terminal {foreground:[0,0,0,255],background:[255,255,255,255],underline:false}).blink()];
    let raw=update::Snapshot::new((12,5),0,[(&[],&[]),(&[],&[]),(&cursor,&[])],12);
    assert!(raw.rendered_lines()[2][0].1.unwrap().blink());
    let on=raw.clone().with_blink_phase(true);let off=raw.with_blink_phase(false);
    assert_eq!(on.blink_phase(),Some(true));assert_eq!(off.blink_phase(),Some(false));
    assert_eq!(on.rendered_lines()[2][0].1.unwrap().background(),Some([255,255,255,255]));
    let changes=update::build_updates(&on,&off,&[]);
    assert_eq!(changes.segments.len(),1);assert_eq!(changes.segments[0].row,SpecialRows::PromtRow);
    assert_eq!(changes.segments[0].offset,0);assert_eq!(changes.segments[0].text," ");
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
fn guarded_matrix_source_matches_visible_cells_clamps_and_keeps_identity() {
    let history=(0..10).map(|i|format!("row{} abcdefghijklmnopqrstuvwxyz",i)).collect::<Vec<_>>();
    let snapshot=update::Snapshot::new((12,8),0,[(&[],&[]),(&[],&[]),(&[],&[])],12)
        .with_matrix_guard(&history,7,99,4).with_matrix_identity(Some("pan".into()),Some(42));
    let area=snapshot.matrix_area().unwrap();let visible=snapshot.rendered_lines();
    assert_eq!((area.offset,area.first,area.columns,area.rows),(5,1,12,5));
    assert_eq!(area.identity,(Some("pan".into()),Some(42)));
    assert_eq!(area.cells.len(),9);assert!(area.cells.iter().all(|r|r.len()==16));
    for y in 0..5 {for x in 0..12 {assert_eq!(area.cell(x as i64,(5+y) as i64),visible[3+y][x]);}}
    let blank=(' ',Some(update::cell_color(None,update::MATRIX_BACKGROUND)));
    assert_eq!(area.cell(-1,5),blank);assert_eq!(area.cell(0,0),blank);
    assert_eq!(area.cell(0,10),blank);assert_ne!(area.cell(13,5).0,' ');
    let empty=update::Snapshot::new((12,8),0,[(&[],&[]),(&[],&[]),(&[],&[])],12)
        .with_matrix_guard(&[],8,99,4);
    assert!(empty.matrix_area().unwrap().cells.is_empty());
    assert_eq!(empty.matrix_area().unwrap().cell(1,1),blank);
}

#[test]
fn horizontal_matrix_source_is_bounded_and_uses_character_coordinates() {
    let history=vec!["§0123456789abcdefghijklmnop".into()];
    let snapshot=update::Snapshot::new((4,4),0,[(&[],&[]),(&[],&[]),(&[],&[])],4)
        .with_matrix_pan(&history,1,10,0,2);
    let area=snapshot.matrix_area().unwrap();
    assert_eq!((area.column_offset,area.first_column),(10,8));
    assert_eq!(area.cells[0].len(),8);
    assert_eq!(snapshot.rendered_lines()[3].iter().map(|c|c.0).collect::<String>(),"9abc");
    assert_eq!(area.cell(10,0),('9',Some(update::cell_color(None,update::MATRIX_BACKGROUND))));
    assert_eq!(area.cell(7,0),(' ',Some(update::cell_color(None,update::MATRIX_BACKGROUND))));
}

#[test]
fn control_cursor_blink_does_not_change_matrix_source_revision() {
    let cursor=[MetaFmtStr::new(" ").blink()];
    let raw=update::Snapshot::new((12,5),0,[(&[],&[]),(&[],&[]),(&cursor,&[])],12)
        .with_matrix_guard(&["row0".into()],7,0,4);
    let on=raw.clone().with_blink_phase(true);let off=raw.with_blink_phase(false);
    assert_eq!(on.matrix_area().unwrap().revision,7);
    assert_eq!(off.matrix_area().unwrap().revision,7);
    assert_eq!(on.matrix_area().unwrap().blink_phase,None);
    assert_eq!(off.matrix_area().unwrap().blink_phase,None);
}

#[test]
fn large_resize_paints_controls_but_keeps_blank_matrix_sparse() {
    let title=[MetaFmtStr::new("TrueOS § 12:34")];let legend=[MetaFmtStr::new("[online peer dl]")];
    let prompt=[MetaFmtStr::new("#")];
    let history=(0..20).map(|index|format!("cmd{} pause stop",index)).collect::<Vec<_>>();
    let snapshot=update::Snapshot::new((640,196),0,[(&title,&legend),(&[],&[]),(&prompt,&[])],640).with_matrix(&history,1);
    let lines=snapshot.rendered_lines();assert_eq!(lines.len(),196);
    assert_eq!(lines[3].iter().take(4).map(|cell|cell.0).collect::<String>(),"cmd0");
    let updates=update::diff_rendered_lines(None,&lines);
    let matrix=updates.iter().filter(|patch|matches!(patch.row,SpecialRows::MatrixRow(_))).collect::<Vec<_>>();
    let cells=matrix.iter().map(|patch|patch.text.chars().count()).sum::<usize>();
    let occupied=lines[3..].iter().flatten().filter(|cell|cell.0!=' ').count();
    assert_eq!(cells,occupied);assert!(cells<400);assert!(cells.div_ceil(64)<=7);
    assert!(matrix.iter().all(|patch|!patch.text.contains(' ')));
    assert_eq!(updates.iter().filter(|patch|!matches!(patch.row,SpecialRows::MatrixRow(_))).map(|patch|patch.text.chars().count()).sum::<usize>(),3*640);
    let empty=update::Snapshot::new((640,196),0,[(&title,&legend),(&[],&[]),(&prompt,&[])],640).with_matrix(&[],2).rendered_lines();
    assert!(update::diff_rendered_lines(None,&empty).iter().all(|patch|!matches!(patch.row,SpecialRows::MatrixRow(_))));
    assert!(update::diff_rendered_lines(Some(&lines),&empty).iter().any(|patch|matches!(patch.row,SpecialRows::MatrixRow(_))&&patch.remove>0));
    // Shorter current row lists must also erase old text, not leave it behind.
    assert!(update::diff_rendered_lines(Some(&lines),&empty[..3]).iter().any(|patch|matches!(patch.row,SpecialRows::MatrixRow(_))&&patch.remove>0));
}

#[test]
fn shared_palette_covers_padding_pan_cells_and_preserves_explicit_styles() {
    let explicit=RgbaColor::Terminal {foreground:[1,2,3,255],background:[4,5,6,255],underline:true};
    let title=[MetaFmtStr::new("X").color(explicit),MetaFmtStr::new("Y").color(RgbaColor::Pink).underline()];
    let prompt=[MetaFmtStr::new(" ").color(RgbaColor::Terminal {foreground:[0,0,0,255],background:[255,255,255,255],underline:false}).blink()];
    let snapshot=update::Snapshot::new((8,5),0,[(&title,&[]),(&[],&[]),(&prompt,&[])],8)
        .with_matrix_guard(&["row".into()],1,0,4);
    let rows=snapshot.rendered_lines();
    assert_eq!(rows[0][0].1,Some(explicit));
    assert_eq!(rows[0][1].1.unwrap().rgba(),RgbaColor::Pink.rgba());
    assert!(rows[0][1].1.unwrap().underline());
    for row in &rows[..3] {for (_,style) in row.iter().skip(2) {assert_eq!(style.unwrap().background(),Some(update::CONTROL_BACKGROUND));}}
    for row in &rows[3..] {for (_,style) in row {assert_eq!(style.unwrap().background(),Some(update::MATRIX_BACKGROUND));}}
    let area=snapshot.matrix_area().unwrap();
    for y in 0..2 {for x in 0..8 {assert_eq!(area.cell(x,y),rows[3+y as usize][x as usize]);}}
    let on=snapshot.clone().with_blink_phase(true).rendered_lines();
    let off=snapshot.with_blink_phase(false).rendered_lines();
    assert_eq!(on[2][0].1.unwrap().background(),Some([255,255,255,255]));
    assert_eq!(off[2][0].1.unwrap().background(),Some(update::CONTROL_BACKGROUND));
    assert_eq!(off[2][0].1.unwrap().rgba(),update::CONTROL_BACKGROUND);
    let app=vec![vec![('A',Some(explicit)),(' ',None)]];
    assert_eq!(update::Snapshot::terminal((8,5),0,app.clone(),1).rendered_lines(),app);
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
