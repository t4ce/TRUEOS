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
    assert '[(&geometry.quads, 7), (&geometry.lines, 1), (&geometry.points, 0)]' in coverage
    assert 'glyph_cell_pixels' not in coverage
    code = '\n'.join([
        'const VIRGL_CCMD_DRAW_VBO:u8=8;',
        block(virgl, 'fn virgl_cmd0('),
        block(virgl, 'struct VirglCmdBuf '),
        block(virgl, 'impl VirglCmdBuf '),
        block(coverage, 'fn draw_native('),
        block(coverage, 'fn destroy_mask_objects('),
        r'''
#[test] fn native_draw_packets_keep_line_point_and_quad_topologies() {
    for (topology,count) in [(0,1),(1,2),(7,4)] {
        let mut b=VirglCmdBuf::new();draw_native(&mut b,count,topology);
        assert_eq!(b.dwords.len(),13);
        assert_eq!(b.dwords[0],0x000c0008);
        assert_eq!(&b.dwords[1..6],&[0,count,topology,0,1]);
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
