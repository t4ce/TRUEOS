#!/usr/bin/env python3
"""Exercise production native VF admission and fetch descriptors on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(path, name):
    source = (ROOT / path).read_text()
    match = re.search(rf"^(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:fn|struct|enum|mod)\s+{name}\b", source, re.M)
    if match is None:
        raise ValueError(f"missing {name}")
    start = match.start()
    while start > 0:
        previous = source.rfind('\n', 0, start - 1) + 1
        if not source[previous:start].lstrip().startswith('#['):
            break
        start = previous
    opening = source.index('{', match.end())
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def main():
    state = 'src/intel/render/state.rs'
    pipeline = 'src/intel/render/pipeline.rs'
    records = [item(state, name) for name in [
        'TriangleVertexFormat', 'TriangleStorageBufferBinding', 'TriangleSampledTextureBinding',
        'TriangleVfInstancingState', 'TriangleNativeDrawContract', 'TrianglePbrMaterial',
        'TriangleIndexBufferPrep', 'TriangleDrawPrep', 'TriangleFrontEndContract',
    ]]
    constants = (ROOT / 'src/intel/render/constants.rs').read_text()
    for name in ['SURFACE_FORMAT_R32G32B32_FLOAT', 'SURFACE_FORMAT_R8_UNORM',
                 'SURFACE_FORMAT_R32G32_FLOAT', 'SURFACE_FORMAT_R32G32B32A32_FLOAT']:
        records.append(re.search(rf'^const {name}:.*?;', constants, re.M).group())
    records += [item(pipeline, name) for name in [
        'instance_palette_vertex_elements', 'instance_palette_vertex_buffer_dw1',
        'native_raster_dw1', 'validate_triangle_native_draw_contract',
    ]]
    records += [item('src/intel/render/resources.rs', 'configure_picasso_retained_uv_diagnostic'),
                item(pipeline, 'retained_native_matrix_draw_contract_tests')]
    adapters = '''#![allow(dead_code)]
#[derive(Copy,Clone,Debug,Eq,PartialEq)] enum ChurnHardwareAdmission { ValidatedProduction }
extern crate self as trueos_helio_runtime;
extern crate self as trueos_helio_artifact;
pub struct DrawIndexedIndirectArgs; impl DrawIndexedIndirectArgs {pub const BYTE_LEN:usize=20;}
pub mod churn {
  pub struct GpuInstanceData; impl GpuInstanceData {pub const BYTE_LEN:usize=208;}
  pub struct GpuCameraUniforms; impl GpuCameraUniforms {pub const BYTE_LEN:usize=368;}
 }
pub mod churn_forward {pub const VERTEX_STRIDE:u32=24;}
const RETAINED_UV_POSITION_BYTE_OFFSET:u32=0;
const RETAINED_UV_TEXCOORD_BYTE_OFFSET:u32=24;
'''
    with tempfile.TemporaryDirectory(prefix='trueos-instance-palette-') as temp:
        source = Path(temp) / 'tests.rs'
        binary = Path(temp) / 'tests'
        source.write_text(adapters + '\n'.join(records))
        subprocess.run(['rustc','--edition=2024','--test',str(source),'-o',str(binary)], cwd=ROOT, check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
