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
    direct = json.loads((native / "tier-width-wings-manifest.json").read_text())
    filled = json.loads((native / "inset-wings-manifest.json").read_text())
    breed = json.loads((native / "breed-wings-manifest.json").read_text())
    translucent = json.loads((native / "translucent-wings-manifest.json").read_text())
    inset_outline = json.loads((native / "inset-outline-manifest.json").read_text())
    package_path = native / "bfly-tier-width-wings.apshader"
    package = package_path.read_bytes()
    fill_package = (native / "bfly-inset-wings.apshader").read_bytes()
    breed_package = (native / "bfly-breed-wings.apshader").read_bytes()
    translucent_package = (native / "bfly-translucent-wings.apshader").read_bytes()
    inset_outline_package = (native / "bfly-inset-outline.apshader").read_bytes()
    mask_manifest = json.loads((native / "celestial-masks-manifest.json").read_text())
    mask_payload = bytearray(struct.pack("<I", len(mask_manifest["stages"])))
    for stage in mask_manifest["stages"]:
        asset = (native / stage["asset_file"]).read_bytes()
        mask_payload += struct.pack("<II", stage["kind"], len(asset)) + asset
    masks = b"APSHDR1\0" + struct.pack("<II", mask_manifest["id"], len(mask_payload)) + mask_payload
    texture_bundle = (native / "app-shaders.bundle").read_bytes()
    smoke = (native / "bfly-smoke-history.apshader").read_bytes()
    cloud = (native / "bfly-cloud-history.apshader").read_bytes()
    cloud_trail = (native / "bfly-cloud-trail.apshader").read_bytes()

    with tempfile.TemporaryDirectory(prefix="bfly-shader-intake-") as directory:
        path = Path(directory)
        (path / "src").mkdir()
        (path / "bfly.o").write_bytes(shader_elf(
            package + fill_package + breed_package + translucent_package + inset_outline_package + masks))
        (path / "history.o").write_bytes(shader_elf(smoke + cloud + cloud_trail))
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
    for (id, sizes) in @GRAPHICS_STAGES@ {
        assert!(app_shaders::present(id));
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
}

#[test]
fn admits_distinct_history_masks_and_a_shader_with_a_shared_mask() {
    let elf = include_bytes!("../history.o");
    app_shaders::register_blueprint_elf(elf).unwrap();
    let smoke = app_shaders::bytes(25, 1).unwrap();
    let cloud = app_shaders::bytes(27, 1).unwrap();
    assert_eq!(smoke, include_bytes!(@SMOKE_MASK@));
    assert_eq!(cloud, include_bytes!(@CLOUD_MASK@));
    assert!(app_shaders::bytes(28, 0).is_some());
    assert!(app_shaders::bytes(28, 1).is_none());
    app_shaders::register_blueprint_elf(elf).unwrap();
    assert!(core::ptr::eq(smoke, app_shaders::bytes(25, 1).unwrap()));
    assert!(core::ptr::eq(cloud, app_shaders::bytes(27, 1).unwrap()));
}

#[test]
fn admits_celestial_masks_in_sampling_order() {
    app_shaders::register_blueprint_elf(include_bytes!("../bfly.o")).unwrap();
    assert_eq!(app_shaders::bytes(35, 0).unwrap(), include_bytes!(@MOON_MASK@));
    assert_eq!(app_shaders::bytes(35, 1).unwrap(), include_bytes!(@EARTH_MASK@));
    assert!(app_shaders::words(35, 0).is_none());
    assert!(app_shaders::bytes(35, 2).is_none());
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
            "@SMOKE_MASK@", json.dumps(str((native / "bfly-smoke.r8").resolve()))).replace(
            "@CLOUD_MASK@", json.dumps(str((native / "bfly-cloud.r8").resolve()))).replace(
            "@MOON_MASK@", json.dumps(str((native / "bfly-moon.r8").resolve()))).replace(
            "@EARTH_MASK@", json.dumps(str((native / "bfly-earth.r8").resolve()))).replace(
            "@GRAPHICS_STAGES@", repr([(record["id"], [stage["bytes"] for stage in record["stages"]])
                                      for record in (direct, filled, breed, translucent, inset_outline)]))
        (path / "src/lib.rs").write_text(source)
        subprocess.run(["cargo", "test", "--lib", "--offline", "--quiet"], cwd=path, check=True)


if __name__ == "__main__":
    main()
