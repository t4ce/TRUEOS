#!/usr/bin/env python3
"""Exercise production multi-strip upload/publication at mocked GPU boundaries."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
MOCKS = r'''
extern crate alloc;
mod libm { pub fn sqrtf(v:f32)->f32 { v.sqrt() } }
use alloc::vec::Vec;
use std::sync::Mutex;
type PaintResult = Result<usize, (&'static str, bool)>;
#[derive(Default)] struct State {
    fault: u8, uploads: usize, released: usize, dropped: usize,
    renders: usize, publishes: usize, cancels: usize,
}
static STATE: Mutex<State> = Mutex::new(State { fault:0, uploads:0, released:0,
    dropped:0, renders:0, publishes:0, cancels:0 });
fn fault() -> u8 { STATE.lock().unwrap().fault }
fn reset(fault:u8) { *STATE.lock().unwrap() = State { fault, ..State::default() }; }
fn line_cases()->Vec<(usize,&'static microfont::raster_line::Strip)> {
    GLYPHS.iter().enumerate().flat_map(|(cell,&ch)|
        microfont::raster_line::glyph_strips(ch as char).into_iter()
            .flat_map(move |strips| strips.iter().map(move |s|(cell,s)))).collect()
}
fn corner_count(s:&microfont::raster_line::Strip)->usize {
    s.points.windows(3).filter(|p| {
        let dx1=p[1].x() as i32-p[0].x() as i32; let dy1=p[1].y() as i32-p[0].y() as i32;
        let dx2=p[2].x() as i32-p[1].x() as i32; let dy2=p[2].y() as i32-p[1].y() as i32;
        dx1*dy2-dy1*dx2!=0
    }).count()
}
fn patch_counts()->Vec<usize> {
    GLYPHS.iter().filter_map(|&ch| microfont::raster_line::glyph_strips(ch as char))
        .map(|strips| strips.iter().map(corner_count).sum::<usize>()).filter(|&n|n>0).collect()
}
fn upload_count()->usize {line_cases().len()+patch_counts().len()}
fn primitive_count()->usize {line_cases().iter().map(|(_,s)|s.segments()).sum::<usize>()+patch_counts().iter().sum::<usize>()}
#[macro_export] macro_rules! log_important { ($($tt:tt)*) => {}; }
mod ui4 {
    #[derive(Clone,Copy)] pub struct FrameHandle;
    #[derive(Debug,PartialEq)] pub enum FramePoolError { Busy }
    pub fn acquire_frame_buffer(_:FrameHandle)->Result<(),FramePoolError> { Ok(()) }
    pub fn gpgpu_rgba_surface(_:())->Result<(),()> { Ok(()) }
    pub fn cancel_frame_buffer(_:())->Result<(),()> {
        super::STATE.lock().unwrap().cancels+=1; Ok(())
    }
    pub fn publish_resident_scene_frame_buffer(_:(),_:u32)->Result<(),()> {
        super::STATE.lock().unwrap().publishes+=1; Ok(())
    }
}
use ui4::FrameHandle;
mod intel { pub mod render {
    #[derive(Clone,Copy,PartialEq,Debug)] pub enum ResidentScenePrimitiveTopology {
        LineStripAdj, LineStrip, LineList, TriangleList,
    }
    pub enum ResidentSceneFragmentContract { ConstantRgba, RoundedLineEnds([u32;16]) }
    pub struct Mesh;
    impl Drop for Mesh { fn drop(&mut self) { super::super::STATE.lock().unwrap().dropped+=1; } }
    pub struct ResidentSceneDraw<'a> {
        pub mesh:&'a Mesh, pub depth_flags:Option<u32>, pub rgba:[u8;4],
        pub sampled_texture:Option<()>, pub fragment_contract:ResidentSceneFragmentContract,
        pub viewport_translation_px:[f32;2], pub topology:ResidentScenePrimitiveTopology,
        pub point_width_px:u32, pub line_width_px:f32, pub clip_adjacency:bool,
    }
    pub fn create_resident_indexed_mesh(v:&[[f32;3]],i:&[u32],t:ResidentScenePrimitiveTopology)
        -> Result<Mesh,&'static str> {
        let mut s=super::super::STATE.lock().unwrap(); s.uploads+=1;
        if s.fault==1 && s.uploads==2 { return Err("upload"); }
        let cases=super::super::line_cases();
        if s.uploads > cases.len() {
            assert_eq!(t,ResidentScenePrimitiveTopology::TriangleList);
            let n=super::super::patch_counts()[s.uploads-cases.len()-1]*3;
            assert_eq!(v.len(),n); assert_eq!(i,(0..n as u32).collect::<Vec<_>>());
            for triangle in v.chunks_exact(3) {
                let [a,b,c] = [triangle[0],triangle[1],triangle[2]];
                assert!((b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]) > 0.0);
            }
            return Ok(Mesh);
        }
        assert_eq!(t,ResidentScenePrimitiveTopology::LineStripAdj);
        let (cell,strip)=cases[s.uploads-1];
        let count=strip.points.len()+2;
        assert_eq!(i,(0..count as u32).collect::<Vec<_>>());
        assert_eq!(v.len(),count);
        assert_eq!(v[0][2],strip.start as u8 as f32);
        assert_eq!(v[count-1][2],strip.end as u8 as f32);
        for (actual,p) in v[1..count-1].iter().zip(strip.iter_points()) {
            let expected_x=2.0*((cell%13) as f32*96.0+(p.x() as f32+0.5)*16.0)/1248.0-1.0;
            let expected_y=1.0-2.0*((cell/13) as f32*176.0+(p.y() as f32+0.5)*16.0)/880.0;
            assert_eq!(*actual,[expected_x,expected_y,0.5]);
        }
        assert_eq!(&v[0][..2],&v[1][..2]);
        assert_eq!(&v[count-2][..2],&v[count-1][..2]);
        Ok(Mesh)
    }
    pub fn release_resident_triangle_mesh(_:&Mesh)->bool {
        super::super::STATE.lock().unwrap().released+=1; true
    }
    pub struct ResultFrame { pub release_fence:Option<u32> }
    impl ResultFrame { pub fn completion_error(&self)->Option<&'static str> {
        (super::super::fault()==4).then_some("retirement")
    }}
    pub fn render_resident_indexed_scene_frame_premultiplied_direct_to_surface(
        draws:&[ResidentSceneDraw<'_>],clear:Option<[u8;4]>,_:(),depth:bool,
    )->Result<ResultFrame,&'static str> {
        super::super::STATE.lock().unwrap().renders+=1;
        let cases=super::super::line_cases();
        assert_eq!(draws.len(),super::super::upload_count()); assert_eq!(clear,Some([255;4])); assert!(!depth);
        for (draw,(_,strip)) in draws.iter().zip(&cases) {
            assert_eq!(draw.topology,ResidentScenePrimitiveTopology::LineStripAdj);
            assert!(draw.clip_adjacency);assert_eq!(draw.line_width_px,8.0);assert_eq!(draw.rgba,[48,100,216,255]);
            let start=strip.start as u8 as f32;let end=strip.end as u8 as f32;
            if start+end>0.0 {
                let ResidentSceneFragmentContract::RoundedLineEnds(params)=&draw.fragment_contract else {panic!("End shader missing")};
                let p=params.map(f32::from_bits);
                assert_eq!(&p[8..12],&[4.0,8.0 * crate::END_CORNER_FRACTION,start,end]);
                assert_eq!(&p[12..16],&[0.1875,0.375,0.8125,1.0]);
            } else {assert!(matches!(draw.fragment_contract,ResidentSceneFragmentContract::ConstantRgba));}
        }
        assert!(draws[cases.len()..].iter().all(|d| d.topology==ResidentScenePrimitiveTopology::TriangleList
            && !d.clip_adjacency && d.line_width_px==0.0 && d.rgba==[255,96,32,255]));
        match super::super::fault() {
            2=>Err("render-busy"), 3=>Err("uncertain"),
            n=>Ok(ResultFrame { release_fence:if n==5 {None} else {Some(1)} }),
        }
    }
}}
'''
TESTS = r'''
#[test] fn scroll_steps_use_named_presets_and_clamp() {
    assert_eq!(microfont::FontShape::Meso.line_width_px(),9.0);
    assert_eq!(microfont::FontShape::Lean.line_width_px(),7.0);
    assert_eq!(width_after_scroll(8.0,1),8.25);
    assert_eq!(width_after_scroll(8.0,-1),7.75);
    assert_eq!(width_after_scroll(8.0,i32::MAX),9.0);
    assert_eq!(width_after_scroll(8.0,i32::MIN),7.0);
    use microfont::raster_line::{Point,Strip,Boundary::{End,Junction}};
    const A:[Strip;2]=[
        Strip::new(&[Point::new(0,8),Point::new(2,0),Point::new(4,8)],End,End),
        Strip::new(&[Point::new(0,4),Point::new(4,4)],Junction,Junction),
    ];
    let a=&A;
    for width in [7.0,7.25,8.0,9.0] {
        let p=rounded_end_params(&a[0],0,width).unwrap().map(f32::from_bits);
        assert_eq!(p[8],width*0.5);
        assert_eq!(p[9],width*END_CORNER_FRACTION);
    }
}
#[test] fn rounded_params_use_only_end_tags_and_screen_pixel_radius() {
    use microfont::raster_line::{Point,Strip,Boundary::{End,Junction}};
    const A:[Strip;2]=[
        Strip::new(&[Point::new(0,8),Point::new(2,0),Point::new(4,8)],End,End),
        Strip::new(&[Point::new(0,4),Point::new(4,4)],Junction,Junction),
    ];
    let a=&A;
    assert!(rounded_end_params(&a[1],0,8.0).is_none());
    let p=rounded_end_params(&a[0],2,8.0).unwrap().map(f32::from_bits);
    assert_eq!(&p[..2],&[200.0,136.0]);assert_eq!(&p[4..6],&[264.0,136.0]);
    for offset in [2,6] {assert!((p[offset]*p[offset]+p[offset+1]*p[offset+1]-1.0).abs()<1e-6);}
    let row=rounded_end_params(&a[0],17,8.0).unwrap().map(f32::from_bits);
    assert_eq!(&row[..2],&[392.0,312.0]);assert_eq!(&row[4..6],&[456.0,312.0]);
    let digit=rounded_end_params(&a[0],26,8.0).unwrap().map(f32::from_bits);
    assert_eq!(&digit[..2],&[8.0,488.0]);assert_eq!(&digit[4..6],&[72.0,488.0]);
    let lower=rounded_end_params(&a[0],39,8.0).unwrap().map(f32::from_bits);
    assert_eq!(&lower[..2],&[8.0,664.0]);
    let lower2=rounded_end_params(&a[0],52,8.0).unwrap().map(f32::from_bits);
    assert_eq!(&lower2[..2],&[8.0,840.0]);
    assert_eq!(&p[8..12],&[4.0,8.0 * crate::END_CORNER_FRACTION,1.0,1.0]);
}
#[test] fn repeated_endpoint_uses_nearest_distinct_point() {
    use microfont::raster_line::{Point,Strip,Boundary::End};
    const STRIP:Strip=Strip::new(&[Point::new(0,8),Point::new(0,8),Point::new(4,4)],End,End);
    let p=rounded_end_params(&STRIP,0,8.0).unwrap().map(f32::from_bits);
    assert!((p[2]-core::f32::consts::FRAC_1_SQRT_2).abs()<1e-6);
    assert!((p[3]+core::f32::consts::FRAC_1_SQRT_2).abs()<1e-6);
    assert_eq!(&p[10..12],&[1.0,1.0]);
}
#[test] fn bevel_fills_outside_of_a_right_turn_and_scales_with_width() {
    let small=corner_triangle([0.0,8.0],[0.0,0.0],[8.0,0.0],8.0).unwrap();
    assert_eq!(small,[[0.0,0.0],[0.0,-4.0],[-4.0,0.0]]);
    let large=corner_triangle([0.0,8.0],[0.0,0.0],[8.0,0.0],16.0).unwrap();
    assert_eq!(large,[[0.0,0.0],[0.0,-8.0],[-8.0,0.0]]);
    assert!(corner_triangle([0.0,0.0],[0.0,0.0],[1.0,1.0],8.0).is_none());
    assert!(corner_triangle([0.0,0.0],[1.0,1.0],[2.0,2.0],8.0).is_none());
}

#[test] fn atlas_uses_only_present_glyphs_and_keeps_empty_cells() {
    reset(0); assert_eq!(paint_once(FrameHandle,8.0),Ok(primitive_count()));
    let s=STATE.lock().unwrap();
    assert_eq!((s.uploads,s.renders,s.publishes,s.cancels,s.released,s.dropped),(upload_count(),1,1,0,upload_count(),upload_count()));
}
#[test] fn second_upload_failure_releases_first_mesh_without_acquiring_a_writer() {
    reset(1); assert_eq!(paint_once(FrameHandle,8.0),Err(("upload",false)));
    let s=STATE.lock().unwrap(); assert_eq!((s.released,s.dropped,s.renders,s.cancels),(1,1,0,0));
}
#[test] fn admission_busy_releases_both_meshes_and_cancels_writer() {
    reset(2); assert_eq!(paint_once(FrameHandle,8.0),Err(("render-busy",false)));
    let s=STATE.lock().unwrap(); assert_eq!((s.released,s.dropped,s.cancels,s.publishes),(upload_count(),upload_count(),1,0));
}
#[test] fn uncertain_retirement_pins_all_meshes_and_writer() {
    for n in [3,4,5] {
        reset(n); assert!(matches!(paint_once(FrameHandle,8.0),Err((_,true))));
        let s=STATE.lock().unwrap(); assert_eq!((s.released,s.dropped,s.cancels,s.publishes),(0,0,0,0));
    }
}
'''

def main():
    subprocess.run(['python3',str(ROOT/'vendor/microfont/tools/generate_line_glyphs.py')],check=True)
    shared = (ROOT/'src/r/services/microfont_raster_preview.rs').read_text()
    shared = shared[shared.index('pub(super) struct PrimitiveBatch'):shared.index('/// Append one perimeter-ordered')]
    paint = (ROOT/'src/r/services/microfont_line_preview.rs').read_text()
    paint = paint[paint.index('fn triple_pixels('):].replace('super::microfont_raster_preview', 'crate::preview')
    with tempfile.TemporaryDirectory(prefix='microfont-adj-') as temp:
        temp = Path(temp)
        lib = temp/'libmicrofont.rlib'
        subprocess.run(['rustc','--edition=2024','--crate-name','microfont','--crate-type','rlib',
            '--cfg','feature="raster"',str(ROOT/'vendor/microfont/src/lib.rs'),'-o',str(lib)], check=True)
        source = MOCKS+'\nmod preview { use super::*;\n'+shared+'\n}\n'+'''
const GLYPHS:&[u8;65]=b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789   abcdefghijklmnopqrstuvwxyz";
const COLUMNS:usize=13;
const WIDTH_STEP_PX:f32=0.25;
const END_CORNER_FRACTION:f32=0.5;
const JOIN_RGBA:[u8;4]=[255,96,32,255];
const LINE_WIDTH_PX:f32=8.0; const SCALE:u32=16; const WIDTH:u32=1248; const HEIGHT:u32=880;
'''+paint+TESTS
        (temp/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test','-A','dead_code','--extern',f'microfont={lib}',
            str(temp/'test.rs'),'-o',str(temp/'tests')],check=True)
        subprocess.run([str(temp/'tests'),'--test-threads=1'],check=True)

if __name__ == '__main__': main()
