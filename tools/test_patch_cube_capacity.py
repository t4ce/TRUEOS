#!/usr/bin/env python3
"""Check the actual baked cube kernels against the resident draw-state slot."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant


def main():
    source = '#![allow(dead_code, unfulfilled_lint_expectations)]\n'
    source += f'mod intel {{ #[path="{ROOT}/src/intel/shader.rs"] pub mod shader; }}\n'
    source += constant('src/intel/render/constants.rs', 'RESIDENT_SCENE_STATE_SLOT_BYTES')
    source += r'''
#[test]
fn cube_patch_complete_bundle_leaves_a_descriptor_page() {
    use intel::shader;
    let align = |n: usize, a: usize| (n + a - 1) & !(a - 1);
    let pipeline = &shader::patch_cube::PIPELINE;
    let end = |m: shader::ShaderKernelMetadata| (m.code_offset_bytes + m.code_size_bytes) as usize;
    let graphics_end = end(pipeline.vs.meta.kernel).max(end(pipeline.ps.meta.kernel));
    // The upload path also packs its optional adjacency kernels before HS/DS.
    let line = shader::line_adjacency_geometry_shader().meta.kernel;
    let triangle = shader::triangle_adjacency_geometry_shader().meta.kernel;
    let after = if graphics_end > line.code_offset_bytes.min(triangle.code_offset_bytes) as usize {
        align(align(graphics_end, 64) + line.code_size_bytes as usize, 64)
            + triangle.code_size_bytes as usize
    } else {
        graphics_end.max(end(line)).max(end(triangle))
    };
    let ([hs, ds], end) = shader::patch_cube_upload_layout(after, RESIDENT_SCENE_STATE_SLOT_BYTES)
        .expect("baked cube shader plus descriptors must fit the resident slot");
    assert_eq!(hs % 64, 0);
    assert_eq!(ds % 64, 0);
    assert!(hs >= after);
    assert!(ds >= hs + shader::patch_cube::TESS_CONTROL.len() * 4);
    let required = align(end, 4096) + 4096;
    assert!(required <= RESIDENT_SCENE_STATE_SLOT_BYTES);
    assert_eq!(shader::patch_cube_upload_layout(after, required - 1),
               Err("tess-code-and-state-capacity"));
    println!("cube shader code end={end}, required with descriptors={required}, slot={}",
             RESIDENT_SCENE_STATE_SLOT_BYTES);
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-cube-capacity-') as temporary:
        root = Path(temporary)
        (root / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(root / 'tests.rs'),
                        '-o', str(root / 'tests')], check=True)
        subprocess.run([str(root / 'tests'), 'cube_patch_complete', '--nocapture'], check=True)


if __name__ == '__main__':
    main()
