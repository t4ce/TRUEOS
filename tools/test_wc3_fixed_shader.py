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
vs=meta['vertex_compiler_state'];ps=meta['fragment_compiler_state']
assert [vs[k] for k in ('vf_packing0','urb_read_length','urb_entry_64b','binding_table_entries')] == [0xffff,2,2,2]
assert [ps[k] for k in ('grf_start16','num_varying_inputs','binding_table_entries','scratch_bytes','push_bytes')] == [6,3,4,0,0]
assert meta['sbe'] == {'read_offset_32b':1,'read_length_32b':2,'attributes':3}
print('Fixed GL shader source, package ID, embedded binaries, ISA and captured payloads agree.')

import tempfile
from test_clip_position3_uv_texture import item
with tempfile.TemporaryDirectory(prefix='wc3-state-tests-') as tmp:
    src = Path(tmp) / 'tests.rs'; exe = Path(tmp) / 'tests'
    src.write_text(''.join(item('src/gpu/vgpu.rs', name) for name in ('fixed_gl_state_valid','fixed_gl_state_tests','fixed_gl_texture_state_valid','fixed_gl_texture_state_tests')))
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
