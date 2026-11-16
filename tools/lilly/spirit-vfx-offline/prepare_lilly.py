#!/usr/bin/env python3
"""Restore the runtime-mapped LilyDS idle clip to native 256px canvases."""
import argparse
import json
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[3]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--source', type=Path, default=ROOT.parent / 'Lily/LilyDS')
args = parser.parse_args()
catalog = [line.split('|') for line in (ROOT / 'tools/Lilly.catalog').read_text().splitlines()
           if line.startswith('idle.crossed.soft_blink|')][0]
source_map = dict(line.split('|') for line in (ROOT / 'tools/Lilly.sources').read_text().splitlines()
                  if line and not line.startswith('#'))
directory = source_map[catalog[1]]
alignment = json.loads((args.source / 'alignment.json').read_text())
assert alignment['canvas'] == [256, 256]
assert alignment['origin'] == 'top-left'
assert alignment['rect_format'] == ['x', 'y', 'w', 'h']
output = ROOT / 'bld/lilly-preview'
output.mkdir(parents=True, exist_ok=True)
for slot in range(7):
    relative = f'{directory}/frame_{slot + 2:02}.png'
    x, y, w, h = alignment['frames'][relative]
    with Image.open(args.source / relative) as image:
        assert image.size == (w, h), relative
        assert 0 <= x <= 256 - w and 0 <= y <= 256 - h, relative
        canvas = Image.new('RGBA', (256, 256))
        canvas.paste(image.convert('RGBA'), (x, y))
        canvas.save(output / f'frame_{slot + 1:02}.png')
print(f'Prepared {catalog[0]}: {directory}/frame_02..08.png (aligned 256x256)')
