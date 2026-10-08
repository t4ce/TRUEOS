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
    begin = broker.index('fn figure_position_transforms_finite(')
    layout = broker[begin:broker.index('\npub(crate) fn create_render_pipeline(', begin)]
    pipeline = (ROOT / 'src/intel/render/pipeline.rs').read_text()
    packets = pipeline[pipeline.index('fn figure_uniform_gpu_base('):]
    mocs = re.search(r'^const RENDER_MOCS: u32 = .*?;', (ROOT / 'src/intel/render/constants.rs').read_text(), re.M)[0]
    constants = []
    for path in [ROOT / 'crates/trueos-v/src/vgpu.rs',
                 ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-v/src/vgpu.rs']:
        constants.append(re.search(r'pub const SHADER_PACKAGE_VOXY_FIGURE_FNV1A64: u64 = (0x[0-9A-F]+);', path.read_text())[1])
    assert constants[0] == constants[1], 'Blueprint and kernel shader package IDs differ'
    source = f'''#![allow(dead_code)]
{types}
#[path = "{GENERATED}"] mod figure;
mod v {{ pub mod vgpu {{ pub const SHADER_PACKAGE_VOXY_FIGURE_FNV1A64: u64 = {constants[0]}; pub const VOXY_FIGURE_STATE_BYTES: usize = 2720; }} }}
{layout}
{mocs}
{packets}
#[test] fn original_hidden_bone_normals_are_admitted() {{
    let mut state = [0u8; 2720];
    for offset in (672..2720).step_by(128) {{
        state[offset + 64..offset + 68].copy_from_slice(&f32::NAN.to_le_bytes());
    }}
    assert!(figure_position_transforms_finite(&state));
    for offset in [128, 640, 672, 672 + 15 * 128] {{
        state[offset..offset + 4].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert!(!figure_position_transforms_finite(&state));
        state[offset..offset + 4].fill(0);
    }}
}}
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
#[test] fn constant_ranges_survive_high_gpu_addresses_and_reject_bad_alignment() {{
    let base = 0x1_0000_0000u64;
    let [vs, ps] = figure_constant_packets(base, 64).unwrap();
    assert_eq!(vs[0], 0x7815_0409);
    assert_eq!(vs[2], 3 | (1 << 16));
    assert_eq!((vs[7], vs[8]), (64 + 128, 1));
    assert_eq!((vs[9], vs[10]), (64 + 512 + 128, 1));
    assert_eq!(ps[0], 0x7817_0409);
    assert_eq!(ps[2], 2 << 16);
    assert_eq!((ps[9], ps[10]), (64 + 224, 1));
    for (address, bytes) in [(0, 64), (1, 64), (base, 8), (base, 0), (u64::MAX & !31, 64)] {{
        assert!(figure_constant_packets(address, bytes).is_err());
    }}
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
