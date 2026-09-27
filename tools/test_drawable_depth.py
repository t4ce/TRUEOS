#!/usr/bin/env python3
"""Compile production drawable-depth layout and wire validation on the host."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[1]
api = 'crates/trueos-v/src/vgpu.rs'
depth = 'src/intel/render/drawable_depth.rs'
source = '''extern crate alloc;
mod intel { pub fn align_up(v: usize, a: usize) -> Option<usize> { v.checked_add(a - 1).map(|n| n & !(a - 1)) } }
'''
source += '\n'.join(constant('src/intel/render/constants.rs', n) for n in ['RESIDENT_SCENE_TARGET_WIDTH', 'RESIDENT_SCENE_TARGET_HEIGHT'])
source += '\n'.join(item(depth, n) for n in ['drawable_depth_bytes', 'drawable_depth_compare', 'drawable_depth_tests'])
source += '\n'.join(constant(api, n) for n in ['INDEXED_DRAW_LOAD_COLOR', 'INDEXED_DRAW_DRAWABLE_DEPTH', 'INDEXED_DRAW_DEPTH_TEST', 'INDEXED_DRAW_DEPTH_WRITE', 'INDEXED_DRAW_CLEAR_DEPTH', 'INDEXED_DRAW_GEOMETRY_CLEAR', 'INDEXED_DRAW_DEPTH_COMPARE_SHIFT', 'INDEXED_DRAW_DEPTH_COMPARE_MASK', 'INDEXED_DRAW_FLAGS_ALL'])
source += item(api, 'indexed_draw_flags_valid')
source += '''
#[test]
fn legacy_and_owned_depth_flags_have_distinct_contracts() {
    assert!(indexed_draw_flags_valid(0));
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_CLEAR_DEPTH));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DEPTH_TEST));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_DEPTH_WRITE));
    for comparison in 0..8 {
        let flags = INDEXED_DRAW_LOAD_COLOR | INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_DEPTH_TEST
            | (comparison << INDEXED_DRAW_DEPTH_COMPARE_SHIFT);
        assert!(indexed_draw_flags_valid(flags));
        assert!(indexed_draw_flags_valid(flags | INDEXED_DRAW_DEPTH_WRITE));
    }
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_DRAWABLE_DEPTH));
    let clear = INDEXED_DRAW_GEOMETRY_CLEAR | INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH;
    assert!(indexed_draw_flags_valid(clear));
    assert!(indexed_draw_flags_valid(clear | INDEXED_DRAW_LOAD_COLOR));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_TEST));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_WRITE));
    assert!(!indexed_draw_flags_valid(clear | INDEXED_DRAW_DEPTH_COMPARE_MASK));
    for extra in [1 << 6, 1 << 7, 1 << 11, 1 << 31] {
        assert!(!indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | extra));
    }
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH));
    assert!(indexed_draw_flags_valid(INDEXED_DRAW_DRAWABLE_DEPTH | INDEXED_DRAW_CLEAR_DEPTH | INDEXED_DRAW_LOAD_COLOR));
}
'''
# Host and Blueprint consumers must agree on the complete wire flag definition.
peer = ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs'
if peer.exists():
    kernel_text = (ROOT / api).read_text()
    peer_text = peer.read_text()
    begin = 'pub const INDEXED_DRAW_LOAD_COLOR'
    end = 'pub const MAX_INDEXED_BATCH_DRAWS'
    assert kernel_text.split(begin)[1].split(end)[0].strip() == peer_text.split(begin)[1].split(end)[0].strip()
with tempfile.TemporaryDirectory(prefix='trueos-depth-') as tmp:
    src = Path(tmp) / 'depth.rs'
    exe = Path(tmp) / 'depth-tests'
    src.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
