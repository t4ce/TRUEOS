#!/usr/bin/env python3
"""Check vmedia size boundaries, a full-size PNG atlas, and optional original JPEGs."""

from pathlib import Path
import argparse
import json
import os
import struct
import subprocess
import tempfile
import tomllib
import zlib

from test_clip_position3_uv_texture import ROOT, constant, item


def png_chunk(kind: bytes, data: bytes) -> bytes:
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def gallery_fixture() -> bytes:
    width, height = 6168, 4112
    compressor = zlib.compressobj(1)
    compressed = bytearray()
    row = b"\0" + bytes([23, 91, 157, 255]) * width
    for _ in range(height):
        compressed.extend(compressor.compress(row))
    compressed.extend(compressor.flush())
    header = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", header) + png_chunk(b"IDAT", compressed) + png_chunk(b"IEND", b"")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("atlases", nargs="*")
    parser.add_argument(
        "--jpeg", action="append", default=[],
        help="Decode an original JPEG through the production codec and admission checks",
    )
    args = parser.parse_args()
    service = "src/r/services/media_service.rs"
    declarations = [constant(service, name) for name in (
        "MAX_ENCODED_BYTES", "MAX_RGBA_BYTES", "MAX_DIMENSION",
        "ERR_INVALID", "ERR_TOO_LARGE", "FORMAT_PNG", "BACKEND_PNG", "PIXEL_FORMAT_RGBA8",
        "FORMAT_JPEG", "BACKEND_ZUNE_JPEG",
    )]
    declarations += [item(service, name) for name in (
        "ImageInfo", "DecodedImage", "validate_encoded_length", "validated_image",
        "rgba_byte_len_within_limit", "image_capacity_tests",
    )]
    with tempfile.TemporaryDirectory(prefix="trueos-vmedia-capacity-") as temporary:
        directory = Path(temporary)
        (directory / "src").mkdir()
        (directory / "atlas.png").write_bytes(gallery_fixture())
        (directory / "Cargo.toml").write_text(f'''[package]
name = "trueos-vmedia-capacity-tests"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
png = {{ path = "{ROOT / 'vendor/png-0.18.1'}", default-features = false }}
core3 = {{ version = "0.1.2", default-features = false, features = ["alloc"] }}
zune-core = {{ path = "{ROOT / 'vendor/zune-core-0.5.1'}", default-features = false }}
zune-jpeg = {{ path = "{ROOT / 'vendor/zune-jpeg-0.5.15'}", default-features = false, features = ["x86", "portable_simd"] }}
[patch.crates-io]
zune-core = {{ path = "{ROOT / 'vendor/zune-core-0.5.1'}" }}
fdeflate = {{ path = "{ROOT / 'vendor/fdeflate-0.3.7'}" }}
simd-adler32 = {{ path = "{ROOT / 'vendor/simd-adler32-0.3.8'}" }}
crc32fast = {{ path = "{ROOT / 'vendor/crc32fast-1.5.0'}" }}
''')
        source = "#![allow(dead_code)]\n" + "\n".join(declarations) + r'''
#[test]
fn vendored_png_decoder_accepts_full_size_rgba_atlas_with_default_limits() {
    let encoded = include_bytes!("../atlas.png");
    assert!(validate_encoded_length(encoded.len()).is_ok());
    let mut decoder = png::Decoder::new(core3::io::Cursor::new(encoded.as_slice()));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().expect("atlas PNG header");
    let mut rgba = vec![0; reader.output_buffer_size().expect("atlas output size")];
    let frame = reader.next_frame(&mut rgba).expect("full atlas PNG decode");
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert_eq!(frame.bit_depth, png::BitDepth::Eight);
    rgba.truncate(frame.buffer_size());
    let image = validated_image(FORMAT_PNG, BACKEND_PNG, frame.width, frame.height, rgba)
        .unwrap_or_else(|error| panic!("decoded atlas rejected: {error}"));
    assert_eq!((image.info.width, image.info.height), (6168, 4112));
    assert_eq!(image.info.stride_bytes, 24_672);
    assert_eq!(image.info.byte_len, 101_451_264);
    assert!(image.rgba.chunks_exact(4).all(|pixel| pixel == [23, 91, 157, 255]));
}
'''
        if args.atlases:
            atlas_paths = ", ".join(json.dumps(str(Path(path).resolve())) for path in args.atlases)
            source += '''
#[test]
fn prepared_atlas_files_pass_service_and_vendored_decoder() {
    for path in [''' + atlas_paths + '''] {
        let encoded = std::fs::read(path).expect("read production atlas");
        assert!(validate_encoded_length(encoded.len()).is_ok(), "encoded atlas rejected: {path}");
        let mut decoder = png::Decoder::new(core3::io::Cursor::new(encoded.as_slice()));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().expect("production atlas PNG header");
        let mut rgba = vec![0; reader.output_buffer_size().expect("atlas output size")];
        let frame = reader.next_frame(&mut rgba).expect("production atlas PNG decode");
        assert_eq!(frame.color_type, png::ColorType::Rgba);
        assert_eq!(frame.bit_depth, png::BitDepth::Eight);
        rgba.truncate(frame.buffer_size());
        let image = validated_image(FORMAT_PNG, BACKEND_PNG, frame.width, frame.height, rgba)
            .unwrap_or_else(|error| panic!("decoded atlas rejected: {path}: {error}"));
        assert_eq!((image.info.width, image.info.height), (6168, 4112));
        assert_eq!(image.info.byte_len, 101_451_264);
        println!("atlas admitted: {path} encoded={} rgba={}", encoded.len(), image.info.byte_len);
    }
}
'''
        if args.jpeg:
            jpeg_paths = ", ".join(json.dumps(str(Path(path).resolve())) for path in args.jpeg)
            source += f'''
extern crate alloc;
#[macro_export]
macro_rules! log {{ ($($arg:tt)*) => {{ println!($($arg)*); }} }}
#[path = "{ROOT / 'crates/trueos-graphics/decoder/jpeg_layout.rs'}"]
mod jpeg_layout;
#[path = "{ROOT / 'crates/trueos-graphics/decoder/jpeg.rs'}"]
mod jpeg;
#[test]
fn original_jpegs_pass_production_decoder_and_service() {{
    for path in [{jpeg_paths}] {{
        let encoded = std::fs::read(path).expect("read JPEG");
        assert!(validate_encoded_length(encoded.len()).is_ok());
        let image = jpeg::decode_jpeg_rgba(&encoded)
            .unwrap_or_else(|error| panic!("JPEG decode failed: {{}}", error.code()));
        let image = validated_image(FORMAT_JPEG, BACKEND_ZUNE_JPEG,
            image.width, image.height, image.rgba)
            .unwrap_or_else(|error| panic!("JPEG admission failed: {{error}}"));
        println!("JPEG admitted: {{path}} encoded={{}} dimensions={{}}x{{}} rgba={{}}",
            encoded.len(), image.info.width, image.info.height, image.info.byte_len);
    }}
}}
'''
        (directory / "src/lib.rs").write_text(source)
        env = os.environ.copy()
        env["RUSTUP_TOOLCHAIN"] = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
        env["CARGO_TARGET_DIR"] = str(ROOT / "bld/vmedia-capacity-host-target")
        subprocess.run([
            "cargo", "test", "--offline", "--quiet", "--target", "x86_64-unknown-linux-gnu",
            "--manifest-path", str(directory / "Cargo.toml"),
            "--", "--nocapture",
        ], cwd=directory, env=env, check=True)


if __name__ == "__main__":
    main()
