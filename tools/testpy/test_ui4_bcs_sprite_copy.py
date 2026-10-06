#!/usr/bin/env python3
"""Run host tests against the production UI4 BCS sprite conversion helpers."""
from pathlib import Path
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT
source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{collections::BTreeMap, vec::Vec};
mod libm { pub fn roundf(x:f32)->f32 {x.round()} }
mod intel {
    pub mod gpgpu {
        #[derive(Clone,Copy,Debug,Default)] pub struct GpgpuRgba8Surface {
            pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32
        }
        impl GpgpuRgba8Surface {
            pub fn new(phys:u64,gpu:u64,bytes:usize,width:u32,height:u32,pitch_bytes:u32)->Option<Self> {
                Some(Self {phys,gpu,bytes,width,height,pitch_bytes})
            }
        }
        #[derive(Clone,Copy,Debug,Default)] pub struct GpgpuSpriteQuadWorklistDesc {
            pub c0_x:f32,pub c0_y:f32,pub c0_u:f32,pub c0_v:f32,
            pub c1_x:f32,pub c1_y:f32,pub c1_u:f32,pub c1_v:f32,
            pub c2_x:f32,pub c2_y:f32,pub c2_u:f32,pub c2_v:f32,
            pub c3_x:f32,pub c3_y:f32,pub c3_u:f32,pub c3_v:f32,
            pub color_rgba:u32,pub flags:u32,
        }
        pub const SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER:u32=1;
        pub const SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC:u32=2;
    }
    #[derive(Clone,Copy,Debug,Default)] pub struct GucBcs0RgbaSurface {
        pub phys:u64,pub gpu:u64,pub bytes:usize,pub width:u32,pub height:u32,pub pitch_bytes:u32
    }
    #[derive(Clone,Copy,Debug,Default)] pub struct GucBcs0RgbaCopy {
        pub source:GucBcs0RgbaSurface,pub source_x:u32,pub source_y:u32,
        pub destination_x:u32,pub destination_y:u32,pub width:u32,pub height:u32
    }
    #[derive(Clone,Copy,Debug,Default)] pub struct GucBcs0RgbaFill {
        pub x:u32,pub y:u32,pub width:u32,pub height:u32,pub color:u32
    }
    #[derive(Clone,Copy)] pub enum GucBcs0CopyCompletion {Pending,Complete,Failed,InvalidSubmission}
    pub type GucBcs0CopySubmission=GucBcs0CopyCompletion;
    pub fn poll_guc_bcs0_rgba_copies(s:GucBcs0CopySubmission)->GucBcs0CopyCompletion {s}
}
use intel::gpgpu::{GpgpuRgba8Surface,GpgpuSpriteQuadWorklistDesc,
    SPRITE_QUAD_WORKLIST_FLAG_SRC_OVER,SPRITE_QUAD_WORKLIST_FLAG_PREMUL_SRC};
const SPRITE_QUAD_FLAG_SRC_OVER:u32=1;
const SPRITE_QUAD_FLAG_PREMUL_COMPOSITOR:u32=1<<30;
const SPRITE_QUAD_FLAG_BCS0_COPY:u32=1<<31;
const SPRITE_QUAD_VALID_FLAGS:u32=SPRITE_QUAD_FLAG_SRC_OVER|SPRITE_QUAD_FLAG_PREMUL_COMPOSITOR|SPRITE_QUAD_FLAG_BCS0_COPY;
const MAX_BCS0_CACHED_ALPHA_RUNS:usize=65536;
const ERROR_INVALID:i32=-1;
const ALPHA_BLEND_WORKLIST_FLAG_COPY:u32=1;
const ALPHA_BLEND_WORKLIST_FLAG_SRC_OVER:u32=2;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_RGB:u32=4;
const ALPHA_BLEND_WORKLIST_FLAG_TINT_ALPHA:u32=8;
#[derive(Clone,Copy,Debug)] struct GpgpuAlphaBlendWorklistDesc {src_xy:u32,dst_xy:u32,size:u32,flags:u32,color_rgba:u32}
#[derive(Default)] struct GpgpuWorklistSubmitStats;
enum Ui4SpriteSceneCompletion {Pending,Complete {stats:GpgpuWorklistSubmitStats,release:bool},Failed}
fn gpgpu_rgba8_release(_:GpgpuRgba8Surface)->bool {true}
'''
items = [
    ('src/ui4/blueprint_text.rs','TrueosUi4SpriteQuad'),
    ('src/ui4/blueprint_text.rs','AlphaRectConversion'),
    ('src/ui4/blueprint_text.rs','NonzeroAlphaRun'),
    ('src/ui4/blueprint_text.rs','rounded_sprite_coordinate'),
    ('src/ui4/blueprint_text.rs','alpha_rect_descriptor'),
    ('src/ui4/blueprint_text.rs','cache_premultiplied_alpha_runs'),
    ('src/ui4/blueprint_text.rs','rgba8_is_premultiplied'),
    ('src/ui4/blueprint_text.rs','bcs0_rgba_surface'),
    ('src/ui4/blueprint_text.rs','bcs0_solid_rect'),
    ('src/ui4/blueprint_text.rs','bcs0_sprite_copies'),
    ('src/ui4/blueprint_text.rs','valid_sprite_quad'),
    ('src/ui4/blueprint_text.rs','sprite_scene_uses_bcs0_clear'),
    ('src/ui4/blueprint_text.rs','sprite_scene_needs_clear'),
    ('src/ui4/blueprint_text.rs','sprite_source_is_premultiplied'),
    ('src/ui4/blueprint_text.rs','gpgpu_sprite_quad_descriptor'),
    ('src/ui4/blueprint_text.rs','sprite_overlay_tests'),
]
source += '\n'.join(extract.item(path,name) for path,name in items)
source += '''
mod bcs_poll {
    pub use crate::intel::{GucBcs0CopyCompletion,GucBcs0CopySubmission};
    pub fn poll_guc_bcs0_rgba_copies(s:GucBcs0CopySubmission)->GucBcs0CopyCompletion {s}
}
'''
# Poll the exact production completion adapter with a minimal private-module shim.
poll = extract.item('src/intel/gpgpu/operations/ui4.rs','poll_ui4_bcs0_sprite_copy')
poll = poll.replace('crate::intel::poll_guc_bcs0_rgba_copies', 'crate::bcs_poll::poll_guc_bcs0_rgba_copies')
source += poll
source += '''
#[test] fn release_is_only_minted_after_bcs_retirement() {
    use intel::GucBcs0CopyCompletion as C;
    let dst=GpgpuRgba8Surface {width:2560,height:1440,..Default::default()};
    assert!(matches!(poll_ui4_bcs0_sprite_copy(C::Pending,dst),Ui4SpriteSceneCompletion::Pending));
    for s in [C::Failed,C::InvalidSubmission] {
        assert!(matches!(poll_ui4_bcs0_sprite_copy(s,dst),Ui4SpriteSceneCompletion::Failed));
    }
    assert!(matches!(poll_ui4_bcs0_sprite_copy(C::Complete,dst),Ui4SpriteSceneCompletion::Complete {release:true,..}));
}
'''
with tempfile.TemporaryDirectory(prefix='ui4-bcs-sprite-') as directory:
    path=Path(directory)/'tests.rs'; path.write_text(source)
    binary=Path(directory)/'tests'
    subprocess.run(['rustc','--edition=2024','--test',str(path),'-o',str(binary)],check=True)
    subprocess.run([str(binary)],check=True)
