#!/usr/bin/env python3
"""Verify the sealed figure bake and run its real target/layout checks on host."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
VOXY = ROOT.parent / 'voxy'
GENERATED = ROOT / 'crates/trueos-shader/generated_voxy_figure_gen12.rs'


def main():
    subprocess.run(['python3', str(VOXY / 'shaderbin/bake_figure_native.py'),
                    '--verify', '--equivalent-package', str(VOXY / 'shaderbin/native/figure/tgl'), '--kernel-output', str(GENERATED)], check=True)
    shader = (ROOT / 'src/intel/shader.rs').read_text()
    types = shader[:shader.index('#[path =')]
    broker = (ROOT / 'src/gpu/vgpu.rs').read_text()
    begin = broker.index('fn render_vertex_layout_supported(')
    layout = broker[begin:broker.index('\npub(crate) fn create_render_pipeline(', begin)]
    constants = []
    for path in [ROOT / 'crates/trueos-v/src/vgpu.rs',
                 ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs']:
        constants.append(re.search(r'pub const SHADER_PACKAGE_VOXY_FIGURE_FNV1A64: u64 = (0x[0-9A-F]+);', path.read_text())[1])
    assert constants[0] == constants[1], 'Blueprint and kernel shader package IDs differ'
    source = f'''#![allow(dead_code)]
{types}
#[path = "{GENERATED}"] mod figure;
mod v {{ pub mod vgpu {{ pub const SHADER_PACKAGE_VOXY_FIGURE_FNV1A64: u64 = {constants[0]}; }} }}
{layout}
#[test] fn sealed_physical_target() {{
    assert!(figure::supports(0x8086, 0x4680, 0x0c));
    assert!(figure::supports(0x8086, 0x9a49, 1));
    for (vendor, device, revision) in [
        (0x8086, 0x4680, 1), (0x10de, 0x4680, 0x0c),
        (0x8086, 0x9a49, 0), (0x8086, 0x9a49, 2), (0x8086, 0x9a40, 1)
    ] {{ assert!(!figure::supports(vendor, device, revision)); }}
}}
#[test] fn packed_layout_does_not_relax_other_packages() {{
    let pair = figure::PACKAGE_FNV1A64;
    assert!(render_vertex_layout_supported(pair, 8, 0));
    for (stride, offset) in [(12, 0), (32, 0), (8, 4), (0, 0)] {{
        assert!(!render_vertex_layout_supported(pair, stride, offset));
    }}
    assert!(!render_vertex_layout_supported(0, 8, 0));
    assert!(render_vertex_layout_supported(0, 12, 0));
    assert!(!render_vertex_layout_supported(0, 12, 4));
}}
#[test] fn original_pair_and_native_bytes_match() {{
    let mut hash = 0xcbf29ce484222325u64;
    for data in [b"voxy-figure-trueos-bringup-v1\\0".as_slice(),
        include_bytes!("{VOXY}/shaderbin/figure-vert.trueos-bringup.spv").as_slice(),
        include_bytes!("{VOXY}/shaderbin/figure-frag.trueos-bringup.spv").as_slice()] {{
        for byte in data {{ hash = (hash ^ *byte as u64).wrapping_mul(0x100000001b3); }}
    }}
    assert_eq!(hash, figure::PACKAGE_FNV1A64);
    assert_eq!(hash, v::vgpu::SHADER_PACKAGE_VOXY_FIGURE_FNV1A64);
    for (stage, bytes) in [
        (figure::PIPELINE.vs.code, include_bytes!("{VOXY}/shaderbin/native/figure/adls/figure.vs.simd8.bin").as_slice()),
        (figure::PIPELINE.ps.code, include_bytes!("{VOXY}/shaderbin/native/figure/adls/figure.ps.simd16.bin").as_slice())
    ] {{
        let embedded: Vec<_> = stage.iter().flat_map(|w| w.to_le_bytes()).collect();
        assert_eq!(embedded, bytes);
    }}
    assert_eq!(figure::VERTEX_PUSH_RANGES, [(0, 0, 128, 96), (3, 0, 128, 32)]);
    assert_eq!(figure::FRAGMENT_PUSH_RANGES, [(0, 0, 224, 64)]);
}}
'''
    with tempfile.TemporaryDirectory(prefix='voxy-figure-tgl-') as directory:
        folder = Path(directory)
        path = folder / 'tests.rs'
        path.write_text(source)
        binary = folder / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
