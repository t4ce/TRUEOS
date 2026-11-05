#!/usr/bin/env python3
"""Compare production PNG decoding with independently constructed pixel data.

Exercise every legal PNG colour/depth combination, all filters, Adam7, split
IDAT chunks and zlib block strategies. Both png and fdeflate use the kernel's
vendored sources. Only the disabled kernel row scheduler is replaced on host.
Run with ``python3 tools/testpy/test_png_decoder.py``; no network is used.
"""

import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import zlib

from test_hull_image_policy import DECODERS, ROOT, chunk, production_item


PASSES = ((0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8),
          (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2))
CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
FORMATS = ((0, 1), (0, 2), (0, 4), (0, 8), (0, 16),
           (2, 8), (2, 16), (3, 1), (3, 2), (3, 4), (3, 8),
           (4, 8), (4, 16), (6, 8), (6, 16))


def packed(samples: list[int], depth: int) -> bytes:
    if depth == 16:
        return b"".join(struct.pack(">H", sample) for sample in samples)
    if depth == 8:
        return bytes(samples)
    result = bytearray((len(samples) * depth + 7) // 8)
    for position, sample in enumerate(samples):
        result[position * depth // 8] |= sample << (8 - depth - position * depth % 8)
    return bytes(result)


def paeth(left: int, above: int, upper_left: int) -> int:
    prediction = left + above - upper_left
    distances = (abs(prediction - left), abs(prediction - above), abs(prediction - upper_left))
    return (left, above, upper_left)[distances.index(min(distances))]


def filtered(row: bytes, previous: bytes, bpp: int, kind: int) -> bytes:
    result = bytearray([kind])
    for position, sample in enumerate(row):
        left = row[position - bpp] if position >= bpp else 0
        above = previous[position] if previous else 0
        upper_left = previous[position - bpp] if previous and position >= bpp else 0
        predictor = (0, left, above, (left + above) // 2, paeth(left, above, upper_left))[kind]
        result.append((sample - predictor) & 255)
    return bytes(result)


def fixture(width: int, height: int, color: int, depth: int, *, interlaced: bool = False,
            filter_kind: int = 0, strategy: int = zlib.Z_DEFAULT_STRATEGY,
            level: int = 6, split: int = 0, transparent: bool = False) -> tuple[bytes, bytes]:
    channels = CHANNELS[color]
    maximum = (1 << depth) - 1
    samples = [[[((x * 37 + y * 61 + channel * 97) ^ (x * y * 3)) & maximum
                 for channel in range(channels)] for x in range(width)] for y in range(height)]
    # This uses all indexed entries, with partial tRNS coverage; missing alpha
    # entries must remain opaque. Keep low-bit-depth samples across row padding.
    palette = bytes(component for index in range(min(1 << depth, 256))
                    for component in ((index * 43) & 255, (index * 71) & 255, (index * 131) & 255))
    transparency = bytes((index * 51) & 255 for index in range(min(1 << depth, 256) // 2))
    transparent_sample = samples[0][0]
    metadata = b""
    if color == 3:
        metadata = chunk(b"PLTE", palette)
        if transparent:
            metadata += chunk(b"tRNS", transparency)
    elif transparent and color in (0, 2):
        metadata = chunk(b"tRNS", b"".join(struct.pack(">H", value) for value in transparent_sample))

    expected = bytearray()
    for row in samples:
        for pixel in row:
            values = [value >> 8 if depth == 16 else value * 255 // maximum for value in pixel]
            if color == 0:
                expected.extend([values[0]] * 3 + [0 if transparent and pixel == transparent_sample else 255])
            elif color == 2:
                expected.extend(values + [0 if transparent and pixel == transparent_sample else 255])
            elif color == 3:
                index = pixel[0]
                alpha = transparency[index] if transparent and index < len(transparency) else 255
                expected.extend(palette[index * 3:index * 3 + 3] + bytes([alpha]))
            elif color == 4:
                expected.extend([values[0]] * 3 + [values[1]])
            else:
                expected.extend(values)

    scanlines = bytearray()
    for x0, y0, dx, dy in PASSES if interlaced else ((0, 0, 1, 1),):
        if x0 >= width or y0 >= height:
            continue
        previous = b""
        for y in range(y0, height, dy):
            row = packed([sample for x in range(x0, width, dx) for sample in samples[y][x]], depth)
            kind = y % 5 if filter_kind == 5 else filter_kind
            scanlines += filtered(row, previous, max(1, (channels * depth + 7) // 8), kind)
            previous = row
    compressor = zlib.compressobj(level=level, strategy=strategy)
    compressed = compressor.compress(scanlines) + compressor.flush()
    idat = b"".join(chunk(b"IDAT", compressed[offset:offset + split])
                    for offset in range(0, len(compressed), split)) if split else chunk(b"IDAT", compressed)
    header = struct.pack(">IIBBBBB", width, height, depth, color, 0, 0, int(interlaced))
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + metadata + idat + chunk(b"IEND", b""), bytes(expected)


HARNESS = r'''#![allow(dead_code)]
extern crate alloc;
mod wait {
    pub struct WaitQueue;
    impl WaitQueue {
        pub fn new() -> Self { Self }
        pub fn notify_all(&self) {}
        pub fn wait_for_event_blocking(&self, _: u64) { unreachable!() }
    }
}
mod workers { pub fn has_background_worker_slot() -> bool { false } }
mod graphics {
    #[path=PNG_PATH] pub mod png;
    #[path=POOL_PATH] mod png_decode_pool;
}
#[cfg(test)] mod tests {
    fn verify(group: &str) {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let cases = std::fs::read_to_string(directory.join("cases.txt")).unwrap();
        let mut count = 0;
        for line in cases.lines().filter(|line| line.starts_with(group)) {
            let fields: Vec<_> = line.split_whitespace().collect();
            let name = fields[0];
            let expected_width: u32 = fields[1].parse().unwrap();
            let expected_height: u32 = fields[2].parse().unwrap();
            let encoded = std::fs::read(directory.join(format!("{name}.png"))).unwrap();
            let expected = std::fs::read(directory.join(format!("{name}.rgba"))).unwrap();
            let image = crate::graphics::png::decode_png_rgba_bounded(&encoded,32*1024*1024,8192)
                .unwrap_or_else(|error| panic!("{name}: {error:?}"));
            assert_eq!((image.width,image.height),(expected_width,expected_height),"{name}");
            assert_eq!(image.rgba.len(),expected.len(),"{name}");
            if let Some(position) = image.rgba.iter().zip(&expected).position(|(actual,want)|actual!=want) {
                panic!("{name}: byte {position}: actual {}, expected {}",image.rgba[position],expected[position]);
            }
            count += 1;
        }
        assert!(count>0,"empty fixture group {group}");
        println!("{group}: {count} exact-pixel fixtures");
    }
    #[test] fn all_color_depths_filters_and_transparency() { verify("color-"); }
    #[test] fn adam7_odd_extents_and_subbyte_rows() { verify("adam7-"); }
    #[test] fn zlib_strategies_and_fragmented_idat() { verify("zlib-"); }
    #[test] fn wide_rows_and_decompression_window_boundaries() { verify("window-"); }
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-png-decoder-") as directory:
        project = Path(directory)
        source = project / "src"
        source.mkdir()
        fixtures = project / "fixtures"
        fixtures.mkdir()
        cases = []

        def add(name: str, width: int, height: int, color: int, depth: int, **options) -> None:
            encoded, expected = fixture(width, height, color, depth, **options)
            (fixtures / (name + ".png")).write_bytes(encoded)
            (fixtures / (name + ".rgba")).write_bytes(expected)
            cases.append(f"{name} {width} {height}\n")

        for color, depth in FORMATS:
            for filter_kind in range(6):
                for transparent in (False, True) if color in (0, 2, 3) else (False,):
                    add(f"color-{color}-{depth}-{filter_kind}-{int(transparent)}", 19, 11,
                        color, depth, filter_kind=filter_kind, transparent=transparent)
            for width, height in ((1, 1), (1, 17), (17, 1), (3, 5), (19, 17)):
                add(f"adam7-{color}-{depth}-{width}-{height}", width, height,
                    color, depth, interlaced=True, filter_kind=5, transparent=True)
        for strategy in (zlib.Z_DEFAULT_STRATEGY, zlib.Z_FILTERED, zlib.Z_HUFFMAN_ONLY,
                         zlib.Z_RLE, zlib.Z_FIXED):
            for level in (0, 1, 9):
                for split in (0, 1, 13):
                    add(f"zlib-{strategy}-{level}-{split}", 37, 29, 6, 8,
                        strategy=strategy, level=level, split=split, filter_kind=5)
        for width, height in ((8192, 3), (4097, 17), (257, 257), (256, 256)):
            for color, depth in ((2, 8), (6, 8), (6, 16), (3, 1)):
                add(f"window-{width}-{height}-{color}-{depth}", width, height,
                    color, depth, filter_kind=2, split=97, transparent=True)
        (fixtures / "cases.txt").write_text("".join(cases))

        pool = (DECODERS / "png_decode_pool.rs").read_text()
        pool = pool.replace(production_item(pool, "spawn_parallel"),
                            "fn spawn_parallel(_: &Arc<RowExpandJob>) { unreachable!() }\n")
        task = production_item(pool, "png_row_expand_task")
        pool = pool.replace("#[trueos_executor::task(pool_size = PNG_ROW_EXPAND_TASK_POOL_SIZE)]\n" + task, "")
        (source / "png_decode_pool.rs").write_text(pool)
        (source / "lib.rs").write_text(HARNESS.replace("PNG_PATH", json.dumps(str(DECODERS / "png.rs")))
                                       .replace("POOL_PATH", json.dumps(str(source / "png_decode_pool.rs"))))
        (project / "Cargo.toml").write_text(f'''[package]
name = "trueos-png-decoder-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
png = {{ path = {json.dumps(str(ROOT / "vendor/png-0.18.1"))}, default-features = false }}
spin = "0.10"
[patch.crates-io]
fdeflate = {{ path = {json.dumps(str(ROOT / "vendor/fdeflate-0.3.7"))} }}
simd-adler32 = {{ path = {json.dumps(str(ROOT / "vendor/simd-adler32-0.3.8"))} }}
crc32fast = {{ path = {json.dumps(str(ROOT / "vendor/crc32fast-1.5.0"))} }}
''')
        environment = dict(os.environ)
        environment["CARGO_TARGET_DIR"] = str(ROOT / "tgt/png-decoder-host-tests")
        environment["RUSTUP_TOOLCHAIN"] = "nightly-2026-07-10"
        print(f"Checking {len(cases)} exact-pixel PNG cases", flush=True)
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                        "--", "--nocapture"], cwd=project, env=environment, check=True)


if __name__ == "__main__":
    main()
