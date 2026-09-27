#!/usr/bin/env python3
"""Verify source/binary/metadata agreement for the fixed-function GPU package."""
import hashlib
import json
import re
import subprocess
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'crates/trueos-shader/wc3_fixed'
meta = json.loads((OUT / 'metadata.json').read_text())
source = b'wc3-fixed-v1:state96:vertex4x4'
for stage in ('vert', 'frag'):
    source += (ROOT / f'tools/wc3-fixed-bake/shaders/fixed.{stage}').read_bytes()
digest = 0xcbf29ce484222325
for byte in source:
    digest = ((digest ^ byte) * 0x100000001b3) & 0xffffffffffffffff
assert int(meta['package_fnv1a64'], 16) == digest
for base in (ROOT, ROOT.parent / 'TRUEOS-Blueprints'):
    api = (base / 'crates/trueos-v/src/vgpu.rs').read_text()
    value, = re.findall(r'SHADER_PACKAGE_WC3_FIXED_FNV1A64: u64 = (0x[0-9a-fA-F_]+);', api)
    assert int(value.replace('_',''),16) == digest
rust = (ROOT / 'crates/trueos-shader/generated_wc3_fixed.rs').read_text()
isa = {}
for stage, name, array in [('vs','fixed.vs.simd8.bin','WC3_FIXED_VS_CODE'),
                           ('ps','fixed.ps.simd16.bin','WC3_FIXED_PS_SIMD16_CODE')]:
    code = (OUT / name).read_bytes()
    assert hashlib.sha256(code).hexdigest() == meta[f'{stage}_sha256']
    assert len(code) == meta[f'{stage}_bytes']
    body, = re.findall(rf'static {array}:.*?= \[(.*?)\];', rust, re.S)
    embedded = b''.join(int(n,16).to_bytes(4,'little') for n in re.findall(r'0x[0-9a-fA-F]+',body))
    assert embedded == code
    decoded = subprocess.check_output(['iga64','-d','-p=12p1',str(OUT/name)], text=True)
    assert 'illegal' not in decoded.lower()
    assert 'EOT' in decoded
    isa[stage] = decoded
vs=meta['vertex_compiler_state'];ps=meta['fragment_compiler_state']
assert [vs[k] for k in ('vf_packing0','urb_read_length','urb_entry_64b','binding_table_entries')] == [0xffff,2,2,2]
assert [ps[k] for k in ('grf_start16','num_varying_inputs','binding_table_entries','scratch_bytes','push_bytes')] == [6,3,4,0,0]
assert meta['sbe'] == {'read_offset_32b':1,'read_length_32b':2,'attributes':3}
print('Fixed GL shader source, package ID, embedded binaries, ISA and captured payloads agree.')

import tempfile
from test_clip_position3_uv_texture import item

# Execute the final packet emission block. Earlier correct state is ineffective
# if a compatibility tail overwrites it immediately before the primitive.
pipeline_source = (ROOT / 'src/intel/render/pipeline.rs').read_text()
tail, = re.findall(r'(log_batch_offset\(cursor, "3DSTATE_PS_BLEND verified-host-tail"\);.*?)\s*log_batch_offset\(cursor, "3DSTATE_BLEND_STATE_POINTERS verified-host-tail"', pipeline_source, re.S)
with tempfile.TemporaryDirectory(prefix='wc3-final-blend-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text('''
const CMD_3DSTATE_PS_BLEND: u32 = 0x784D0000;
fn log_batch_offset(_: usize, _: &str) {}
fn push(words: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), ()> {
    words[*cursor] = value; *cursor += 1; Ok(())
}
fn emit(batch_dwords: &mut [u32], ps_blend_dw1: u32) -> Result<(), ()> {
    let mut cursor = 0;
''' + tail + '''
    Ok(())
}
#[test]
fn final_packet_preserves_opaque_alpha_and_additive_draws() {
    for state in [1 << 30, 0x60000080 | (3 << 14) | (19 << 9),
                  0x60000080 | (1 << 14) | (1 << 9)] {
        let mut words = [0;2];
        emit(&mut words, state).unwrap();
        assert_eq!(words, [CMD_3DSTATE_PS_BLEND, state]);
    }
}
''')
    subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)

# Read the binding indices from the shipped instructions, not the intended GLSL
# layout: the compiler allocates texture and storage bindings independently.
def message_bti(assembly, target):
    descriptors = re.findall(
        rf'send\.{target}\s+[^\n]*?0x[0-9a-fA-F]+\s+0x([0-9a-fA-F]+)', assembly)
    assert descriptors, f'No {target} messages found'
    return {int(value, 16) & 0xff for value in descriptors}

texture_bti, = message_bti(isa['ps'], 'smpl')
state_bti, = message_bti(isa['ps'], 'dc0')
assert message_bti(isa['vs'], 'dc0') == {1}
with tempfile.TemporaryDirectory(prefix='wc3-fixed-binding-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text(item('src/intel/render/pipeline.rs', 'fixed_gl_ps_surface_indices') + f'''
#[test]
fn compiled_fragment_messages_reach_their_surface_records() {{
    let table = fixed_gl_ps_surface_indices();
    assert_eq!(table[{texture_bti}], 2, "sampler must reach RGBA texture");
    assert_eq!(table[{state_bti}], 1, "state reads must reach raw GL state");
}}
''')
    subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)

with tempfile.TemporaryDirectory(prefix='wc3-state-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text(''.join(item('src/gpu/vgpu.rs', name) for name in ('fixed_gl_state_valid','fixed_gl_state_tests','fixed_gl_texture_state_valid','fixed_gl_texture_state_tests')))
    subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)

# Exercise the production state-appending constructor with a recording allocator.
# The allocation includes state; the VF extent must exclude it, while the index
# offset must still remain after it. A helper-only address test cannot catch this.
with tempfile.TemporaryDirectory(prefix='wc3-fixed-mesh-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text('''
enum TriangleVertexFormat { FixedGl }
struct ResidentTriangleMesh {
    vertex_count: u32, vertex_bytes: u32, index_offset: usize, upload: Vec<[f32;16]>,
}
fn create_resident_triangle_mesh_typed(
    vertices: &[[f32;16]], _: &[u32], _: TriangleVertexFormat, _: Option<()>,
) -> Result<ResidentTriangleMesh, &'static str> {
    Ok(ResidentTriangleMesh {
        vertex_count: vertices.len() as u32,
        vertex_bytes: core::mem::size_of_val(vertices) as u32,
        index_offset: core::mem::size_of_val(vertices), upload: vertices.to_vec(),
    })
}
''' + item('src/intel/render/resources.rs', 'create_resident_fixed_gl_mesh')
        + item('src/intel/render/pipeline.rs', 'fixed_gl_state_gpu_addr') + '''
#[test]
fn shader_binding_reaches_uploaded_state_after_unique_vertices() {
    for count in [3, 4, 64] {
        let vertices = vec![[1.;16];count];
        let state = core::array::from_fn(|i| i as f32 + 100.);
        let mesh = create_resident_fixed_gl_mesh(&vertices, &[0,1,2,2,1,0], &state).unwrap();
        assert_eq!(mesh.vertex_count as usize, count);
        assert_eq!(mesh.vertex_bytes as usize, count * 64);
        let offset = (fixed_gl_state_gpu_addr(0x20000000, mesh.vertex_bytes)-0x20000000) as usize;
        assert_eq!(mesh.upload[offset / 64..].iter().flatten().copied().collect::<Vec<_>>(), state);
        assert_eq!(mesh.index_offset, offset + 1536);
    }
}
''')
    subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)

with tempfile.TemporaryDirectory(prefix='wc3-fixed-address-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text(item('src/intel/render/pipeline.rs', 'fixed_gl_state_gpu_addr') + '''
#[test]
fn state_follows_unique_vertex_bytes_in_an_indexed_draw() {
    let vertex_gpu = 0x2000_0000;
    let unique_vertex_bytes = 4 * 64;
    let reused_index_count = 600u64;
    assert_eq!(fixed_gl_state_gpu_addr(vertex_gpu, unique_vertex_bytes), 0x2000_0100);
    assert_ne!(fixed_gl_state_gpu_addr(vertex_gpu, unique_vertex_bytes),
               vertex_gpu + reused_index_count * 64);
}
''')
    subprocess.run(['rustc','--edition=2024','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe)],check=True)
