#!/usr/bin/env python3
"""Exercise production sprite crop conversion and BCS display-release gating."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract
ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
source = '''#![allow(dead_code)]
#[derive(Clone,Copy)] struct GpgpuRgba8Surface {width:u32,height:u32}
#[derive(Clone,Copy,Debug)] struct GpgpuAlphaBlendWorklistDesc {src_xy:u32,dst_xy:u32,size:u32,flags:u32,color_rgba:u32}
mod libm { pub fn roundf(x:f32)->f32 {x.round()} }
const SPRITE_QUAD_FLAG_SRC_OVER:u32=1;
const ALPHA_BLEND_WORKLIST_FLAG_COPY:u32=1;
const ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER:u32=2;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB:u32=4;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA:u32=8;
'''
for name in ['TrueosUi4SpriteQuad','AlphaRectConversion','rounded_sprite_coordinate','alpha_rect_descriptor']:
    source += extract.item('src/ui4/blueprint_text.rs',name)
source += '''
#[derive(Default)] struct GpgpuWorklistSubmitStats;
enum Ui4SpriteSceneCompletion {Pending,Complete {stats:GpgpuWorklistSubmitStats,release:bool},Failed}
fn gpgpu_rgba8_release(_:GpgpuRgba8Surface)->bool {true}
mod intel {
    #[derive(Clone,Copy)] pub enum GucBcs0CopyCompletion {Pending,Complete,Failed,InvalidSubmission}
    pub type GucBcs0CopySubmission=GucBcs0CopyCompletion;
    pub fn poll_guc_bcs0_rgba_copies(s:GucBcs0CopySubmission)->GucBcs0CopyCompletion {s}
}
'''
source += extract.item('src/intel/gpgpu/operations/ui4.rs','poll_ui4_bcs0_sprite_copy')
source += '''
#[test] fn crop_clipping_and_copy_flags_preserve_fast_copy_contract() {
    let src=GpgpuRgba8Surface {width:3840,height:2160};
    let dst=GpgpuRgba8Surface {width:2560,height:1440};
    let q=TrueosUi4SpriteQuad {
        sprite_id:1,c0_x:0.0,c0_y:0.0,c0_u:640.0/3840.0,c0_v:360.0/2160.0,
        c1_x:2560.0,c1_y:0.0,c1_u:3200.0/3840.0,c1_v:360.0/2160.0,
        c2_x:2560.0,c2_y:1440.0,c2_u:3200.0/3840.0,c2_v:1800.0/2160.0,
        c3_x:0.0,c3_y:1440.0,c3_u:640.0/3840.0,c3_v:1800.0/2160.0,
        color_rgba:u32::MAX,flags:0,
    };
    let AlphaRectConversion::Exact(r)=alpha_rect_descriptor(q,src,dst) else {panic!("crop rejected")};
    assert_eq!((r.src_xy,r.dst_xy,r.size,r.flags),(640|360<<16,0,2560|1440<<16,ALPHA_BLEND_WORKLIST_FLAG_COPY));
    for (color,flags) in [(0x80ffffff,0),(u32::MAX,SPRITE_QUAD_FLAG_SRC_OVER),(0xffffff80,0)] {
        let AlphaRectConversion::Exact(r)=alpha_rect_descriptor(TrueosUi4SpriteQuad {color_rgba:color,flags,..q},src,dst) else {panic!()};
        assert_ne!(r.flags,ALPHA_BLEND_WORKLIST_FLAG_COPY,"tinted/blended quads cannot blit");
    }
    assert!(matches!(alpha_rect_descriptor(TrueosUi4SpriteQuad {c1_x:1280.0,c2_x:1280.0,..q},src,dst),AlphaRectConversion::Unsupported));
}
#[test] fn release_is_only_minted_after_bcs_retirement() {
    use intel::GucBcs0CopyCompletion as C;
    let dst=GpgpuRgba8Surface {width:2560,height:1440};
    assert!(matches!(poll_ui4_bcs0_sprite_copy(C::Pending,dst),Ui4SpriteSceneCompletion::Pending));
    for s in [C::Failed,C::InvalidSubmission] {
        assert!(matches!(poll_ui4_bcs0_sprite_copy(s,dst),Ui4SpriteSceneCompletion::Failed));
    }
    assert!(matches!(poll_ui4_bcs0_sprite_copy(C::Complete,dst),Ui4SpriteSceneCompletion::Complete {release:true,..}));
}
'''
with tempfile.TemporaryDirectory(prefix='ui4-bcs-sprite-') as directory:
    path=Path(directory)/'tests.rs';path.write_text(source)
    binary=Path(directory)/'tests'
    subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True)
