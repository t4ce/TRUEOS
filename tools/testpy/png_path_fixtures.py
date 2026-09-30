#!/usr/bin/env python3
"""Generate deterministic PNG decode/display fixtures (Pillow + NumPy).

Usage: python3 tools/png_path_fixtures.py bld/png-path/assets
Each Full HD/4K RGB/RGBA image is emitted with every PNG row filter. The
manifest records the raw scanline SHA-256 for byte-exact decoder comparison.
"""
import hashlib
import json
from pathlib import Path
import struct
import sys
import zlib

import numpy as np
from PIL import Image, ImageDraw, ImageFont


def chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


def encode(pixels, filter_type):
    height, width, bpp = pixels.shape
    raw = pixels.reshape(height, width * bpp).astype(np.int16)
    left = np.zeros_like(raw)
    left[:, bpp:] = raw[:, :-bpp]
    above = np.zeros_like(raw)
    above[1:] = raw[:-1]
    corner = np.zeros_like(raw)
    corner[1:, bpp:] = raw[:-1, :-bpp]
    if filter_type == 0:
        prediction = 0
    elif filter_type == 1:
        prediction = left
    elif filter_type == 2:
        prediction = above
    elif filter_type == 3:
        prediction = (left + above) // 2
    else:
        p = left + above - corner
        pa, pb, pc = abs(p - left), abs(p - above), abs(p - corner)
        prediction = np.where((pa <= pb) & (pa <= pc), left, np.where(pb <= pc, above, corner))
    filtered = (raw - prediction).astype(np.uint8)
    rows = np.empty((height, width * bpp + 1), dtype=np.uint8)
    rows[:, 0] = filter_type
    rows[:, 1:] = filtered
    header = struct.pack('>IIBBBBB', width, height, 8, 2 if bpp == 3 else 6, 0, 0, 0)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(rows.tobytes(), 6)) + chunk(b'IEND', b'')


def main():
    output = Path(sys.argv[1] if len(sys.argv) > 1 else 'bld/png-path/assets')
    output.mkdir(parents=True, exist_ok=True)
    records = []
    for width, height in [(1920, 1080), (3840, 2160)]:
        y, x = np.ogrid[:height, :width]
        rgb = np.empty((height, width, 3), dtype=np.uint8)
        rgb[:, :, 0] = x * 255 // (width - 1)
        rgb[:, :, 1] = y * 255 // (height - 1)
        rgb[:, :, 2] = ((x // 48) ^ (y // 48)) % 2 * 100 + 70
        image = Image.fromarray(rgb)
        draw = ImageDraw.Draw(image)
        font = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf', height // 18)
        draw.rectangle((width // 12, height // 3, width * 11 // 12, height * 2 // 3), fill=(12, 20, 34))
        draw.text((width // 10, height * 2 // 5), f'TRUEOS PNG  {width} x {height}', fill='white', font=font)
        draw.text((width // 10, height // 2), 'Shared decode  /  UI4  /  Arrow navigation', fill=(100, 220, 250), font=font.font_variant(size=height // 30))
        for mode in ['RGB', 'RGBA']:
            pixels = np.array(image.convert(mode))
            if mode == 'RGBA':
                pixels[:height // 5, :, 3] = x * 255 // (width - 1)
            expected = hashlib.sha256(pixels.tobytes()).hexdigest()
            for number, name in enumerate(['none', 'sub', 'up', 'average', 'paeth']):
                filename = f'{width}x{height}-{mode.lower()}-{name}.png'
                encoded = encode(pixels, number)
                (output / filename).write_bytes(encoded)
                with Image.open(output / filename) as check:
                    assert check.tobytes() == pixels.tobytes(), filename
                records.append(dict(file=filename, width=width, height=height, mode=mode, filter=name, bytes=len(encoded), raw_sha256=expected))
                print(filename, len(encoded), flush=True)
    (output / 'manifest.json').write_text(json.dumps(records, indent=2) + '\n')
    # Serve the parent directory to open this mixed PNG/JPEG Solara test page.
    root = Path(__file__).resolve().parents[1]
    (output.parent / 'index.html').write_text((root / 'tools/fixtures/png-path.html').read_text().replace('assets/', output.name + '/'))
    (output / 'logo.jpg').write_bytes((root.parent / 'TRUEOS-Blueprints/apps/solara/assets/images/logo.jpg').read_bytes())


if __name__ == '__main__':
    main()
