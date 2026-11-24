#!/usr/bin/env python3
"""Exercise the production Blueprint shader intake with bfly's native bundle."""

import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
APP = ROOT.parent / "TRUEOS-Blueprints/apps/bfly"


def shader_elf(bundle):
    names = b"\x00.trueos.shaders\x00.shstrtab\x00"
    data = 64 + len(names)
    table = (data + len(bundle) + 7) & ~7
    elf = bytearray(table + 3 * 64)
    ident = b"\x7fELF\x02\x01\x01".ljust(16, b"\x00")
    struct.pack_into("<16sHHIQQQIHHHHHH", elf, 0,
                     ident, 1, 62, 1, 0, 0, table, 0, 64, 0, 0, 64, 3, 2)
    elf[64:data] = names
    elf[data:data + len(bundle)] = bundle
    struct.pack_into("<IIQQQQIIQQ", elf, table + 64,
                     1, 1, 0, 0, data, len(bundle), 0, 0, 1, 0)
    struct.pack_into("<IIQQQQIIQQ", elf, table + 128,
                     17, 3, 0, 0, 64, len(names), 0, 0, 1, 0)
    return elf


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, default=APP)
    args = parser.parse_args()
    native = args.app / "shaders/trueos"
    direct = json.loads((native / "direct-wings-manifest.json").read_text())
    package_path = native / "bfly-direct-wings.apshader"
    package = package_path.read_bytes()
    texture_bundle = (native / "app-shaders.bundle").read_bytes()

    with tempfile.TemporaryDirectory(prefix="bfly-shader-intake-") as directory:
        path = Path(directory)
        (path / "src").mkdir()
        (path / "bfly.o").write_bytes(shader_elf(package))
        (path / "textures.o").write_bytes(shader_elf(texture_bundle))
        corrupt = bytearray(package)
        corrupt[-1] ^= 1
        (path / "corrupt.o").write_bytes(shader_elf(corrupt))
        (path / "Cargo.toml").write_text('''[package]
name = "bfly-shader-intake-test"
version = "0.0.0"
edition = "2024"
[dependencies]
object = { version = "0.39.1", default-features = false, features = ["read_core", "elf"] }
sha2 = { version = "=0.10.9", default-features = false }
spin = { version = "0.10", default-features = false, features = ["once"] }
''')
        source = r'''
#![allow(dead_code)]
extern crate alloc;
#[path = @LOADER@]
mod app_shaders;

#[test]
fn admits_archived_texture_packages_as_aligned_native_stages() {
    let elf = include_bytes!("../textures.o");
    app_shaders::register_blueprint_elf(elf).unwrap();
    for (id, sizes) in [(11, &[272, 2224, 656, 48, 48][..]), (12, &[5752, 1888, 2048, 3776])] {
        assert!(app_shaders::present(id));
        for (stage, size) in sizes.iter().enumerate() {
            let words = app_shaders::words(id, stage).unwrap();
            assert_eq!(words.len() * 4, *size);
            assert_eq!(words.as_ptr().align_offset(4), 0);
            assert!(app_shaders::bytes(id, stage).is_none());
        }
        assert!(app_shaders::words(id, sizes.len()).is_none());
    }
}

#[test]
fn admits_direct_tessellation_stages_and_reuses_storage() {
    let elf = include_bytes!("../bfly.o");
    app_shaders::register_blueprint_elf(elf).unwrap();
    let id = @DIRECT_ID@;
    assert!(app_shaders::present(id));
    let sizes = @DIRECT_STAGE_SIZES@;
    let mut stage_pointers = [core::ptr::null(); 4];
    for (stage, (pointer, size)) in stage_pointers.iter_mut().zip(sizes).enumerate() {
        let words = app_shaders::words(id, stage).unwrap();
        assert_eq!(words.len() * 4, size);
        *pointer = words.as_ptr();
        assert_eq!(words.as_ptr().align_offset(4), 0);
        assert!(app_shaders::bytes(id, stage).is_none());
    }
    assert_eq!(sizes.len(), stage_pointers.len());
    assert!(app_shaders::words(id, sizes.len()).is_none());
    app_shaders::register_blueprint_elf(elf).unwrap();
    for (stage, pointer) in stage_pointers.iter().enumerate() {
        assert_eq!(app_shaders::words(id, stage).unwrap().as_ptr(), *pointer);
    }
}

#[test]
fn rejects_corrupt_unknown_and_truncated_packages() {
    let corrupt = include_bytes!("../corrupt.o");
    assert_eq!(app_shaders::register_blueprint_elf(corrupt), Err("app-shader-sha256-mismatch"));
    let mut package = include_bytes!(@PACKAGE@).to_vec();
    let truncated = &package[..package.len() - 1];
    assert_eq!(app_shaders::register_section(truncated), Err("app-shader-truncated"));
    package[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(app_shaders::register_section(&package), Err("app-shader-untrusted-id"));
}
'''.replace("@LOADER@", json.dumps(str(ROOT / "src/gpu/app_shaders.rs"))).replace(
            "@PACKAGE@", json.dumps(str(package_path.resolve()))).replace(
            "@DIRECT_ID@", str(direct["id"])).replace(
            "@DIRECT_STAGE_SIZES@", json.dumps([stage["bytes"] for stage in direct["stages"]]))
        (path / "src/lib.rs").write_text(source)
        subprocess.run(["cargo", "test", "--lib", "--offline", "--quiet"], cwd=path, check=True)


if __name__ == "__main__":
    main()
