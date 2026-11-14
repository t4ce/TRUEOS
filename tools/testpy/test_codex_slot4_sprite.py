#!/usr/bin/env python3
"""Check the production Codex row-run renderer against pixel clipping geometry."""
from pathlib import Path
import json
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]
SLOT4 = "src/ui4/slot4_service.rs"


def main():
    renderer = "\n".join(item(SLOT4, name) for name in (
        "CodexTileRun", "codex_tile_runs", "push_codex_tile_runs", "push_overlay_rect"))
    source = r'''#![allow(dead_code)]
extern crate alloc;
use alloc::vec::Vec;
#[path=@GEOMETRY@] mod geometry;
const ICON:&[u8;64*64*4]=include_bytes!(@ICON@);
mod graphics {pub mod primitives {
    #[derive(Clone,Copy,Debug,Eq,PartialEq)]
    pub struct Rgba8{pub r:u8,pub g:u8,pub b:u8,pub a:u8}
    impl Rgba8{pub fn new(r:u8,g:u8,b:u8,a:u8)->Self{Self{r,g,b,a}}}
}}
mod intel {
    use crate::graphics::primitives::Rgba8;
    pub struct LiveOverlayRect{pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:Rgba8}
    impl LiveOverlayRect{pub fn new(x:u32,y:u32,width:u32,height:u32,color:Rgba8)->Self{Self{x,y,width,height,color}}}
}
mod spirit {pub mod codex_handoff {
    pub struct Visual{pub pose:crate::geometry::Pose,pub opacity:u8,pub hidden:bool}
}}
type Slot4Rects=Vec<intel::LiveOverlayRect>;
@RENDERER@

#[test]
fn cached_runs_match_native_pixels_and_cursor_exclusion_in_every_direction(){
    use geometry::{Point,Pose};
    let runs=codex_tile_runs(ICON);
    assert!(!runs.is_empty());
    assert!(runs.len()<4096);
    let (screen_w,screen_h)=(600u32,480u32);
    for (x,y) in [(-20,-15),(590,470),(50,200),(470,200),(260,45),(260,390),
        (170,90),(440,90),(170,350),(440,350),(260,200)] {
        for opacity in [0,127,233,255] {
            let pose=Pose{icon:Point{x,y},cursor:Point{x:200,y:120}};
            let mut rects=Vec::new();
            push_codex_tile_runs(&mut rects,spirit::codex_handoff::Visual{pose,opacity,hidden:false},&runs,screen_w,screen_h);
            let mut actual=vec![0;screen_w as usize*screen_h as usize*4];
            let mut written=vec![false;screen_w as usize*screen_h as usize];
            for rect in rects {
                assert_eq!(rect.height,1);
                assert!(rect.x+rect.width<=screen_w && rect.y<screen_h);
                for xx in rect.x..rect.x+rect.width {
                    let index=(rect.y*screen_w+xx) as usize;
                    assert!(!written[index],"runs must never blend a pixel twice");
                    written[index]=true;
                    let c=rect.color;
                    actual[index*4..index*4+4].copy_from_slice(&[
                        ((u32::from(c.r)*u32::from(c.a)+127)/255) as u8,
                        ((u32::from(c.g)*u32::from(c.a)+127)/255) as u8,
                        ((u32::from(c.b)*u32::from(c.a)+127)/255) as u8,c.a]);
                }
            }
            let mut tile=vec![0;64*64*4];
            geometry::ui4_part(pose,ICON,opacity,&mut tile,256);
            let mut expected=vec![0;actual.len()];
            for yy in 0..64 {for xx in 0..64 {
                let sx=x+xx;let sy=y+yy;
                if sx>=0 && sy>=0 && sx<screen_w as i32 && sy<screen_h as i32 {
                    let target=((sy as u32*screen_w+sx as u32)*4) as usize;
                    let source=((yy*64+xx)*4) as usize;
                    expected[target..target+4].copy_from_slice(&tile[source..source+4]);
                }
            }}
            assert_eq!(actual,expected,"icon {x},{y} opacity {opacity}");
        }
    }
}

#[test]
fn hidden_sprite_emits_no_interaction_plane_content(){
    let mut rects=Vec::new();
    let runs=codex_tile_runs(ICON);
    let pose=geometry::Pose{icon:geometry::Point{x:10,y:10},cursor:geometry::Point{x:300,y:300}};
    push_codex_tile_runs(&mut rects,spirit::codex_handoff::Visual{pose,opacity:255,hidden:true},&runs,600,480);
    assert!(rects.is_empty());
}
'''.replace("@GEOMETRY@", json.dumps(str(ROOT / "src/spirit/codex_handoff/geometry.rs"))).replace(
        "@ICON@", json.dumps(str(ROOT / "tools/lilly/codex-tile64.rgba"))).replace("@RENDERER@", renderer)
    with tempfile.TemporaryDirectory(prefix="codex-slot4-sprite-") as directory:
        work = Path(directory)
        (work / "tests.rs").write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(work / "tests.rs"), "-o", str(work / "tests")], check=True)
        subprocess.run([str(work / "tests")], check=True)


if __name__ == "__main__":
    main()
