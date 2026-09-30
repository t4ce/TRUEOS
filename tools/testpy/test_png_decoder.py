#!/usr/bin/env python3
"""Exercise the production shared PNG decoder and pure expansion on the host.

The adapter replaces only the disabled row worker dispatch with direct calls.
Pillow supplies independent expected pixels for packed palettes, transparency,
RGB, RGBA, grayscale, grayscale-alpha and 16-bit grayscale inputs.
"""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import struct
import zlib
import tomllib
from PIL import Image
from test_clip_position3_uv_texture import ROOT, item


def main():
    with tempfile.TemporaryDirectory(prefix='trueos-png-decoder-') as temporary:
        folder = Path(temporary)
        (folder / 'src').mkdir()
        cases = []
        for mode in ['RGB', 'RGBA', 'L', 'LA', 'P', 'I;16']:
            for width in [1, 3, 17, 67]:
                image = Image.new(mode, (width, 9))
                if mode == 'P':
                    image.putpalette([n for i in range(256) for n in (i, 255-i, (i*17)%256)])
                values = []
                for i in range(width * 9):
                    if mode in ['L', 'P']: value = i % 4
                    elif mode == 'I;16': value = (i*257) % 65536
                    elif mode == 'LA': value = (i % 256, (i*13) % 256)
                    else: value = tuple((i*k) % 256 for k in ([3, 7, 13] if mode == 'RGB' else [3, 7, 13, 19]))
                    values.append(value)
                image.putdata(values)
                file = folder / f'{mode.replace(";", "-")}-{width}.png'
                options = dict(bits=2, transparency=bytes([0, 70, 170, 255])) if mode == 'P' else {}
                image.save(file, **options)
                with Image.open(file) as independent:
                    if mode == 'I;16':
                        raw = bytes(n for v in independent.getdata() for n in (v >> 8, v >> 8, v >> 8, 255))
                    else: raw = independent.convert('RGBA').tobytes()
                expected = file.with_suffix('.rgba')
                expected.write_bytes(raw)
                cases.append((str(file), str(expected)))
        # Adam7 passes with independent per-pass rows and odd dimensions.
        for channels in [3, 4]:
            width, height = 19, 13
            pixels = bytes((i * 37 + i // 5) % 256 for i in range(width * height * channels))
            raw = bytearray()
            for sx, sy, dx, dy in [(0,0,8,8),(4,0,8,8),(0,4,4,8),(2,0,4,4),(0,2,2,4),(1,0,2,2),(0,1,1,2)]:
                for y in range(sy, height, dy):
                    row = b''.join(pixels[(y*width+x)*channels:(y*width+x+1)*channels] for x in range(sx, width, dx))
                    if row: raw.extend(b'\0' + row)
            def chunk(kind, data):
                return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))
            header = struct.pack('>IIBBBBB', width, height, 8, 2 if channels == 3 else 6, 0, 0, 1)
            encoded = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b'')
            file = folder / f'adam7-{channels}.png'; file.write_bytes(encoded)
            with Image.open(file) as independent: expected_pixels = independent.convert('RGBA').tobytes()
            expected = file.with_suffix('.rgba'); expected.write_bytes(expected_pixels)
            cases.append((str(file), str(expected)))
        # Color-key transparency exercises the decoder's EXPAND transform.
        for mode, key in [('RGB', (3, 7, 13)), ('L', 17)]:
            file = folder / f'{mode}-trns.png'
            im = Image.new(mode, (17, 3), key)
            im.save(file, transparency=key)
            with Image.open(file) as independent: raw = independent.convert('RGBA').tobytes()
            expected = file.with_suffix('.rgba'); expected.write_bytes(raw)
            cases.append((str(file), str(expected)))
        (folder / 'Cargo.toml').write_text(f'''[package]
name = "trueos-shared-png-tests"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
png = {{ path = "{ROOT / 'vendor/png-0.18.1'}", default-features = false }}
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
[patch.crates-io]
fdeflate = {{ path = "{ROOT / 'vendor/fdeflate-0.3.7'}" }}
simd-adler32 = {{ path = "{ROOT / 'vendor/simd-adler32-0.3.8'}" }}
crc32fast = {{ path = "{ROOT / 'vendor/crc32fast-1.5.0'}" }}
''')
        pool = 'crates/trueos-graphics/decoder/png_decode_pool.rs'
        functions = '\n'.join(item(pool, name) for name in ['indexed_row_bytes', 'indexed_sample', 'expand_indexed_rows', 'expand_rgb_rows', 'expand_gray_rows', 'expand_gray_alpha_rows'])
        source = f'''extern crate alloc;
#[path = "{ROOT / 'crates/trueos-graphics/decoder/png.rs'}"] mod png_decoder;
mod png_decode_pool {{
use super::png_decoder::PngDecodeError;
use std::ops::Range;
{functions}
pub fn expand_indexed_png_to_rgba(width: u32, height: u32, depth: png::BitDepth, pixels: Vec<u8>, palette: Vec<u8>, trns: Option<Vec<u8>>) -> Result<Vec<u8>, PngDecodeError> {{
    expand_indexed_rows(width as usize, 0..height as usize, depth, &pixels, &palette, trns.as_deref())
}}
pub fn expand_png_output_to_rgba(color: png::ColorType, depth: png::BitDepth, width: u32, height: u32, pixels: Vec<u8>) -> Result<Vec<u8>, PngDecodeError> {{
    if depth != png::BitDepth::Eight {{ return Err(PngDecodeError::Unsupported); }}
    match color {{
        png::ColorType::Rgb => expand_rgb_rows(width as usize, 0..height as usize, &pixels),
        png::ColorType::Grayscale => expand_gray_rows(width as usize, 0..height as usize, &pixels),
        png::ColorType::GrayscaleAlpha => expand_gray_alpha_rows(width as usize, 0..height as usize, &pixels),
        png::ColorType::Rgba => Ok(pixels),
        _ => Err(PngDecodeError::Unsupported),
    }}
}}
}}
#[test] fn color_depth_and_transparency_match_independent_decoder() {{
    for (path, expected) in [{', '.join('(' + json.dumps(a) + ', ' + json.dumps(b) + ')' for a,b in cases)}] {{
        let bytes = std::fs::read(path).unwrap();
        let image = png_decoder::decode_png_rgba(&bytes).unwrap();
        assert_eq!(image.rgba, std::fs::read(expected).unwrap(), "{{path}}");
        assert_eq!(image.rgba.len(), image.width as usize * image.height as usize * 4);
        assert!(png_decoder::decode_png_rgba(&bytes[..bytes.len()/2]).is_err(), "truncated {{path}}");
    }}
    assert!(png_decoder::decode_png_rgba(b"not a png").is_err());
}}
'''
        (folder / 'src/lib.rs').write_text(source)
        env = os.environ.copy()
        env['RUSTUP_TOOLCHAIN'] = tomllib.loads((ROOT / 'rust-toolchain.toml').read_text())['toolchain']['channel']
        env['CARGO_TARGET_DIR'] = str(ROOT / 'bld/png-path/decoder-tests-target')
        subprocess.run(['cargo', 'test', '--offline', '--quiet', '--target', 'x86_64-unknown-linux-gnu'], cwd=folder, env=env, check=True)


if __name__ == '__main__':
    main()
