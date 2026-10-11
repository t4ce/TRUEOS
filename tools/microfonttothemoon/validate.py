#!/usr/bin/env python3
"""Validate the embedded GPU mesh and optionally compare another detail level."""
import hashlib
import json
import math
from pathlib import Path
import struct
import sys


def validate(root):
    root = Path(root)
    data = (root / 'draft.mesh').read_bytes()
    meta = json.loads((root / 'catalog.json').read_text())
    assert data[:8] == b'MOONMSH1'
    assert hashlib.sha256(data).hexdigest() == meta['mesh_sha256']
    nv, ni = struct.unpack_from('<II', data, 8)
    expected = 512 if meta.get('set') == 'gui-han' else 514
    assert len(meta['glyphs']) == expected
    if expected == 512:
        codes = [ord(g['char']) for g in meta['glyphs']]
        assert codes == sorted(set(codes))
        assert all(g['kind'] == 'han' and g['width'] == 6 for g in meta['glyphs'])
    assert len(data) == 16 + nv * 12 + ni * 4 and ni % 3 == 0
    verts = list(struct.iter_unpack('<fff', data[16:16 + nv * 12]))
    indices = [v[0] for v in struct.iter_unpack('<I', data[16 + nv * 12:])]
    assert all(math.isfinite(x) for v in verts for x in v)
    assert all(-1 <= x <= 1 and -1 <= y <= 1 for x, y, z in verts)
    assert min(indices) >= 0 and max(indices) < nv
    for i in range(0, ni, 3):
        a, b, c = (verts[indices[i + k]] for k in range(3))
        area = (b[0]-a[0])*(c[1]-a[1]) - (b[1]-a[1])*(c[0]-a[0])
        assert area > 0, (i, area)
    assert all(abs(g['spacing'] - 1/meta['detail']) < 1e-9
               for g in meta['glyphs'] if g['route'] == 'field')
    assert sum(g['vertices'] for g in meta['glyphs']) == nv
    assert sum(g['gpuTriangles'] for g in meta['glyphs']) == ni // 3
    print(f"detail={meta['detail']}: {nv} vertices, {ni//3} triangles; SHA, bounds, winding, indices, spacing pass")
    return meta, nv, ni


if __name__ == '__main__':
    first = validate(sys.argv[1] if len(sys.argv) > 1 else 'src/intel/gpu_font/moon')
    if len(sys.argv) > 2:
        second = validate(sys.argv[2])
        assert second[0]['detail'] > first[0]['detail']
        assert second[1] > first[1] and second[2] > first[2]
        print('Increasing detail changes actual mesh density.')
