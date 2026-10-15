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
    meta = json.loads((root / 'catalog.json').read_text())
    if 'parts' in meta:
        assert meta['part_count'] == len(meta['parts']) == 4
        expected = 512 if meta['set'] == 'gui-han' else 514
        assert meta['glyph_count'] == len(meta['glyphs']) == expected
        children = []
        for index, part in enumerate(meta['parts'], 1):
            child = validate(root / part['directory'])
            assert child[0]['part'] == index
            assert child[0]['path'] == part['path']
            assert child[0]['mesh_sha256'] == part['mesh_sha256']
            for key in ['recipe', 'detail', 'scale', 'width', 'height']:
                assert child[0][key] == meta[key]
            children.append(child)
        glyphs = sorted((g for child in children for g in child[0]['glyphs']),
                        key=lambda g: g['slot'])
        assert glyphs == meta['glyphs']
        assert [g['slot'] for g in glyphs] == list(range(expected))
        assert len({part['path'] for part in meta['parts']}) == 4
        if meta['set'] == 'gui-han':
            codes = [ord(g['char']) for g in glyphs]
            assert codes == sorted(set(codes))
            assert all(g['kind'] == 'han' and g['width'] == 6 for g in glyphs)
        print(f"{meta['set']}: all {expected} glyphs occur exactly once across four images")
        return meta, sum(c[1] for c in children), sum(c[2] for c in children)
    data = (root / 'draft.mesh').read_bytes()
    assert data[:8] == b'MOONMSH1'
    assert hashlib.sha256(data).hexdigest() == meta['mesh_sha256']
    nv, ni = struct.unpack_from('<II', data, 8)
    expected = meta['glyph_count'] if 'part' in meta else (512 if meta.get('set') == 'gui-han' else 514)
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
    if 'part' in meta:
        assert meta['scale'] == 8 and meta['width'] <= 2560 and meta['height'] <= 1440
        vertex_offset = index_offset = 0
        tiles = set()
        for glyph in meta['glyphs']:
            column, row, span = glyph['tile']
            for x in range(column, column + span):
                assert (x, row) not in tiles
                tiles.add((x, row))
            start, stop = vertex_offset, vertex_offset + glyph['vertices']
            index_stop = index_offset + glyph['gpuTriangles'] * 3
            assert all(start <= index < stop for index in indices[index_offset:index_stop])
            x0, x1 = column * meta['tile_width'], (column + span) * meta['tile_width']
            y0, y1 = row * meta['tile_height'], (row + 1) * meta['tile_height']
            for x, y, _ in verts[start:stop]:
                px, py = (x + 1) * meta['width'] / 2, (1 - y) * meta['height'] / 2
                assert x0 - .0002 <= px <= x1 + .0002
                assert y0 - .0002 <= py <= y1 + .0002
            vertex_offset, index_offset = stop, index_stop
        assert vertex_offset == nv and index_offset == ni
    print(f"{root}: detail={meta['detail']}, {nv} vertices, {ni//3} triangles; mesh checks pass")
    return meta, nv, ni


if __name__ == '__main__':
    first = validate(sys.argv[1] if len(sys.argv) > 1 else 'src/intel/gpu_font/moon')
    if len(sys.argv) > 2:
        second = validate(sys.argv[2])
        assert second[0]['detail'] > first[0]['detail']
        assert second[1] > first[1] and second[2] > first[2]
        print('Increasing detail changes actual mesh density.')
