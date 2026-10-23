#!/usr/bin/env python3
"""Compile the production native draw and cleanup encoders on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def block(source, marker):
    start = source.index(marker)
    end = source.index('{', start) + 1
    depth = 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]


def main():
    virgl = (ROOT / 'src/virtio_gpu_logo/virgl.rs').read_text()
    coverage = (ROOT / 'src/virtio_gpu_logo/coverage.rs').read_text()
    # The glyph renderer must consume the native primitives directly.
    assert 'PIPE_PRIM_TRIANGLES' not in coverage
    assert '[(&geometry.lines, 1), (&geometry.points, 0)]' in coverage
    assert 'draw_native(&mut commands, chunk.len() as u32, 0)' in coverage
    assert 'glyph_cell_pixels' not in coverage
    code = '\n'.join([
        'const VIRGL_CCMD_DRAW_VBO:u8=8;',
        'const VIRGL_CCMD_CREATE_OBJECT:u8=1; const VIRGL_OBJECT_RASTERIZER:u8=2; const VIRGL_OBJ_RS_SIZE:u32=9;',
        'mod libm {pub fn ceilf(v:f32)->f32 {v.ceil()}}',
        block(virgl, 'fn virgl_cmd0('),
        block(virgl, 'fn fui('),
        block(virgl, 'struct VirglCmdBuf '),
        block(virgl, 'impl VirglCmdBuf '),
        block(coverage, 'fn draw_native('),
        block(coverage, 'fn destroy_mask_objects('),
        block(coverage, 'fn rasterizer('),
        '#[derive(Clone,Copy)] '+block(coverage, 'pub(crate) struct NativeMaskVertex '),
        block(coverage, 'struct QuadPointVertex '),
        block(coverage, 'fn quad_points('),
        r'''
#[test] fn native_draw_packets_keep_line_and_point_topologies() {
    for (topology,count) in [(0,1),(1,2)] {
        let mut b=VirglCmdBuf::new();draw_native(&mut b,count,topology);
        assert_eq!(b.dwords.len(),13);
        assert_eq!(b.dwords[0],0x000c0008);
        assert_eq!(&b.dwords[1..6],&[0,count,topology,0,1]);
    }
}
#[test] fn native_line_envelope_preserves_perpendicular_width_at_diagonal_slopes() {
    let mut b=VirglCmdBuf::new();rasterizer(&mut b,99,16.0,18.0);
    assert_eq!(b.dwords[2]&(1<<26),1<<26);
    assert_eq!(f32::from_bits(b.dwords[3]),18.0);
    assert_eq!(f32::from_bits(b.dwords[6]),18.0);
}
fn source_vertex(p:[f32;2])->NativeMaskVertex {
    NativeMaskVertex {position:[p[0]/50.0-1.0,p[1]/50.0-1.0,0.0,1.0],
        geometry:[0.0;4],style:[0.0;4]}
}
fn covered(p:&QuadPointVertex,at:[f32;2])->bool {
    p.planes.iter().all(|edge|edge[0]*at[0]+edge[1]*at[1]+edge[2]>=-0.0001)
}
#[test] fn each_canonical_convex_quad_becomes_one_native_point_with_exact_edges() {
    // The second shape is the non-parallelogram cut in the canonical ╳ seed.
    for quad in [[[0.0,0.0],[6.0,0.0],[6.0,11.0],[0.0,11.0]],
                 [[5.0,0.0],[6.0,0.0],[3.5,5.5],[3.0,4.4]]] {
        let (points,envelope)=quad_points(&quad.map(source_vertex),100,100).unwrap();
        assert_eq!(points.len(),1);assert!(envelope>=7.0);
        let p=&points[0];
        let center=[quad.iter().map(|v|v[0]).sum::<f32>()/4.0,
                    quad.iter().map(|v|v[1]).sum::<f32>()/4.0];
        assert!(covered(p,center));assert!(!covered(p,[-1.0,-1.0]));
        for corner in quad {assert!(covered(p,corner));}
    }
}
#[test] fn cleanup_destroys_objects_instead_of_binding_dead_handles() {
    let mut b=VirglCmdBuf::new();
    destroy_mask_objects(&mut b,&[(4,101),(8,202)]);
    assert_eq!(b.dwords,vec![0x00010403,101,0x00010803,202]);
    assert!(b.dwords.chunks_exact(2).all(|packet|packet[0]&255==3));
}
''',
    ])
    with tempfile.TemporaryDirectory(prefix='virgl-microfont-wire-') as directory:
        directory = Path(directory)
        source = directory / 'tests.rs'
        executable = directory / 'tests'
        source.write_text(code)
        subprocess.run(['rustc', '--edition=2024', '--test', '-A', 'dead_code',
                        str(source), '-o', str(executable)], check=True)
        subprocess.run([str(executable)], check=True)


if __name__ == '__main__':
    main()
