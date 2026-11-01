#!/usr/bin/env python3
"""Run the production V2 topology decoder and overlay tests without a kernel boot."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def block(source, start):
    begin = source.index(start)
    opening = source.index('{', begin)
    depth = 1
    end = opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[begin:end]


def main():
    wire = (ROOT / 'crates/trueos-v/src/vgpu.rs').read_text()
    broker = (ROOT / 'src/r/io/vgpu_cabi.rs').read_text()
    gpu = (ROOT / 'src/gpu/vgpu.rs').read_text()
    primary = (ROOT / 'src/intel/render/primary.rs').read_text()
    constants = '\n'.join(re.findall(
        r'^pub const (?:PRIMITIVE_TOPOLOGY_\w+|MAX_INDEXED_BATCH_V2_DRAWS|'
        r'MAX_INDEXED_DRAW_POINT_WIDTH_PX|INDEXED_BATCH_V2_LOAD_COLOR):[^;]+;', wire, re.M))
    wire_types = '\n'.join(
        '#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]\n#[repr(C)]\n' + block(wire, f'pub struct {name} {{')
        for name in ['IndexedBatchDrawV2']
    )
    wire_types += '\n#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n#[repr(C)]\n'
    wire_types += block(wire, 'pub struct IndexedDrawBatchV2 {')
    wire_types += '\n' + block(wire, 'impl Default for IndexedDrawBatchV2 {')
    topology = '#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n' + block(primary, 'pub(crate) enum ResidentScenePrimitiveTopology {')
    descriptor = '#[derive(Copy, Clone, Debug)]\n' + block(gpu, 'pub(crate) struct Ui4IndexedBatchDrawDescriptor {')
    harness = f'''#![allow(dead_code)]
extern crate alloc;
mod v {{ pub mod vgpu {{ {constants}\n{wire_types} }} }}
mod intel {{ pub mod render {{ {topology} }} }}
mod vgpu {{ {descriptor} }}
{block(broker, 'fn broker_primitive_topology(')}
{block(broker, 'fn decode_indexed_batch_v2(')}
{block(broker, 'mod indexed_batch_v2_tests {')}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-vgpu-overlay-') as temporary:
        directory = Path(temporary)
        source = directory / 'overlay.rs'
        binary = directory / 'tests'
        source.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
