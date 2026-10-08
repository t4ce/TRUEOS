#!/usr/bin/env python3
"""Host-test the production partial foreground clear and command bounds."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

SOURCE = 'src/ui4/blueprint_text.rs'
HARNESS = r'''
#![allow(dead_code)]
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FrameCadence { Immutable, Dirty, Streaming }
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum FrameBuffering { Single, Double, Triple }
#[derive(Clone,Copy,Debug)] struct DamageRect { x:u32,y:u32,width:u32,height:u32 }
const SPRITE_QUAD_FLAG_SRC_OVER:u32=1;
const SPRITE_QUAD_FLAG_BCS0_COPY:u32=1<<31;
mod libm {pub fn roundf(value:f32)->f32 {value.round()}}
mod intel {
    #[derive(Debug)] pub struct GucBcs0RgbaFill {pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:u32}
}
#[derive(Clone,Copy)] struct GpgpuRgba8Surface {width:u32,height:u32}
mod production {use super::*;
ITEMS
}
use production::*;
fn region()->DamageRect {DamageRect{x:12,y:8,width:40,height:20}}
#[test] fn dirty_foreground_uses_double_buffering() {
    assert_eq!(blueprint_frame_buffering(FrameCadence::Dirty),FrameBuffering::Double);
    assert!(!valid_sprite_region(FrameCadence::Dirty,true,7,7,100,100,region()));
    assert!(valid_sprite_region(FrameCadence::Dirty,false,7,7,100,100,region()));
    for cadence in [FrameCadence::Immutable,FrameCadence::Streaming] {
        assert!(!valid_sprite_region(cadence,false,7,7,100,100,region()));
    }
    assert!(!valid_sprite_region(FrameCadence::Dirty,false,0x80000007,7,100,100,region()));
}
#[test] fn accepts_exact_edges_rejects_empty_outside_and_overflow() {
    let valid=|r| valid_sprite_region(FrameCadence::Dirty,false,7,7,100,100,r);
    assert!(valid(DamageRect{x:0,y:0,width:100,height:100}));
    assert!(valid(DamageRect{x:99,y:99,width:1,height:1}));
    for r in [
        DamageRect{width:0,..region()},DamageRect{height:0,..region()},
        DamageRect{x:90,..region()},DamageRect{y:90,..region()},
        DamageRect{x:u32::MAX,width:2,..region()},
        DamageRect{y:u32::MAX,height:2,..region()},
    ] {assert!(!valid(r),"{r:?}");}
}
#[test] fn clear_quad_covers_only_requested_region() {
    let q=sprite_frame_clear_quad(0,region());
    assert_eq!(q.sprite_id,0);assert_eq!(q.color_rgba,0);assert_eq!(q.flags,0);
    assert_eq!([(q.c0_x,q.c0_y),(q.c1_x,q.c1_y),(q.c2_x,q.c2_y),(q.c3_x,q.c3_y)],[(12.,8.),(52.,8.),(52.,28.),(12.,28.)]);
    assert!(sprite_region_contains_quad(region(),q));
}
#[test] fn zero_alpha_bcs_clear_is_exact_region_fill() {
    let mut q=sprite_frame_clear_quad(0,region());q.flags=SPRITE_QUAD_FLAG_BCS0_COPY;
    let fill=bcs0_solid_rect(q,GpgpuRgba8Surface{width:100,height:100}).unwrap();
    assert_eq!((fill.x,fill.y,fill.width,fill.height,fill.color),(12,8,40,20,0));
}
#[test] fn rejects_commands_outside_region_or_with_nonfinite_coordinates() {
    let q=sprite_frame_clear_quad(0,region());
    assert!(sprite_region_contains_quad(region(),q));
    for outside in [
        TrueosUi4SpriteQuad{c0_x:11.,..q},TrueosUi4SpriteQuad{c1_x:53.,..q},
        TrueosUi4SpriteQuad{c2_y:29.,..q},TrueosUi4SpriteQuad{c3_y:7.,..q},
        TrueosUi4SpriteQuad{c0_x:f32::NAN,..q},TrueosUi4SpriteQuad{c1_y:f32::INFINITY,..q},
    ] {assert!(!sprite_region_contains_quad(region(),outside));}
    let inside=sprite_frame_clear_quad(0,DamageRect{x:14,y:10,width:10,height:10});
    assert!(sprite_region_contains_quad(region(),inside));
}
'''


def main():
    functions = '\n'.join(item(SOURCE, name).replace('fn ', 'pub(super) fn ', 1) if name != 'TrueosUi4SpriteQuad' else item(SOURCE, name) for name in (
        'TrueosUi4SpriteQuad', 'blueprint_frame_buffering', 'valid_sprite_region',
        'sprite_region_contains_quad', 'sprite_frame_clear_quad',
        'rounded_sprite_coordinate', 'bcs0_solid_rect',
    ))
    with tempfile.TemporaryDirectory(prefix='ui4-sprite-region-') as directory:
        source=Path(directory)/'tests.rs'; source.write_text(HARNESS.replace('ITEMS',functions))
        binary=Path(directory)/'tests'
        subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
