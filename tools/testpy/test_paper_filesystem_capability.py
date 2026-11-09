#!/usr/bin/env python3
"""Execute Paper's filesystem capability through production launch policy.

The real header parser, capability methods, prebind module, guest filesystem
bootstrap branch and Paper startup reads run unchanged. ELF import discovery,
display geometry and filesystem execution are fixture boundaries; this test
does not require prepared packages, a disk, a GPU or a guest VM.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib

from test_clip_position3_uv_texture import constant, item
from test_sdk_metadata_v2 import item as source_item


ROOT = Path(__file__).resolve().parents[2]
PAPER = ROOT.parent / "TRUEOS-Buildins/paper"
BLUEPRINT = "src/hv/blueprint/blueprint.rs"


def harness(package_dir: Path | None = None) -> str:
    metadata = tomllib.loads((PAPER / "Cargo.toml").read_text())
    independent = metadata["package"].get("metadata", {}).get("trueos-blueprint", {}).get(
        "filesystem-independent", False
    )
    assert isinstance(independent, bool)
    blueprint = (ROOT / BLUEPRINT).read_text()
    declarations = "\n".join(constant(BLUEPRINT, name) for name in (
        "BLUEPRINT_HEADER_LEN", "BLUEPRINT_PAYLOAD_MASK", "BLUEPRINT_PAYLOAD_RAW",
        "BLUEPRINT_PAYLOAD_TAR_LZ4", "BLUEPRINT_CAP_REPLICATABLE",
        "BLUEPRINT_CAP_ARGV_ENTRY_V1", "BLUEPRINT_CAP_FILESYSTEM_INDEPENDENT",
        "BLUEPRINT_CAP_TRUEOSFS_SCOPE",
    ))
    declarations += "\n" + item(BLUEPRINT, "BlueprintModule")
    declarations += "\n" + source_item(blueprint, "impl BlueprintModule<'_>")
    declarations += "\n" + "\n".join(item(BLUEPRINT, name) for name in (
        "le_u16", "le_u32", "le_u64", "parse_blueprint", "unpack_blueprint",
    ))
    readiness = "\n".join(constant("src/r/readiness.rs", name) for name in (
        "BACKGROUND_AP_WORKER_READY", "TRUEOSFS_ROOT_MOUNTED", "RAYON_READY",
        "NET_ANY_CONFIGURED", "NET_SOCKET_READY", "TLS_SOCKET_SERVICE_READY", "INTEL_HDA_READY",
    ))
    guest = (ROOT / "src/hv/guest_run.rs").read_text()
    bootstrap = guest[guest.index("    if module.is_filesystem_independent()"):
                      guest.index("    crate::blueprint_net_broker::set_vmx_guest_net_backend(true);")]
    startup = source_item((PAPER / "src/main.rs").read_text(), "fn run()")
    startup = startup[startup.index("    let settings ="):startup.index("    let output =")]
    glyph = source_item((PAPER / "src/main.rs").read_text(), "fn glyph_pixels(")
    settings = (PAPER / "src/lib.rs").read_text().split("/// Match Gridpaper's", 1)[0]
    settings = settings.removeprefix("#![no_std]\n")
    packaged_test = ""
    if package_dir is not None:
        package = package_dir / "dist/paper.bp"
        module = package_dir / "objects/paper/module.stripped.o"
        symbols = subprocess.check_output(["readelf", "-Ws", str(module)], text=True)
        imports = sorted(set(re.findall(r"\bUND\s+(\S+)", symbols)))
        assert imports, f"no undefined imports found in {module}"
        packaged_test = r'''
#[test]
fn prepared_paper_header_and_actual_imports_use_only_background_readiness() {
    let bytes = include_bytes!(@PACKAGE@);
    let module = blueprint::parse_blueprint(bytes).unwrap();
    assert!(module.is_filesystem_independent(), "prepared Paper lacks its capability");
    let imports: &[&str] = &@IMPORTS@;
    for import in PAPER_IMPORTS { assert!(imports.contains(import), "original import removed: {import}"); }
    // The matching stripped ELF supplies the actual import names. Raw wrapping
    // skips compression only; production prebind receives the packaged flags.
    let mut raw = blueprint::package(false, imports);
    let flags = (module.flags & !0xff) | 1;
    raw[6..8].copy_from_slice(&flags.to_le_bytes());
    assert_eq!(blueprint::prebind::prebind_required_readiness(&raw).unwrap().0,
               r::readiness::BACKGROUND_AP_WORKER_READY);
}
'''.replace("@PACKAGE@", json.dumps(str(package))).replace("@IMPORTS@", json.dumps(imports))
    return r'''
#![allow(dead_code, unused_variables, unused_imports)]
extern crate alloc;
use std::{cell::RefCell, string::String, vec::Vec};
const PAPER_INDEPENDENT: bool = @INDEPENDENT@;
mod paper { @PAPER_SETTINGS@ }
mod r {
    pub mod readiness { @READINESS@ }
    pub mod tar {
        pub fn decode_single_lz4(_: &[u8], _: usize) -> Result<Vec<u8>, ()> {
            panic!("raw fixture must not invoke compressed decoding")
        }
    }
}
mod allcaps { pub mod blueprint { pub const ASSET_DECODE_CAP_BYTES: usize = 1024 * 1024; } }
mod unix_compat {
    pub fn unsupported_unix_import_reason(name: &str) -> Option<&'static str> {
        ["opendir", "fdopendir", "readdir", "readdir_r", "closedir", "dirfd"]
            .contains(&name).then_some("unsupported Unix/POSIX import")
    }
}
mod blueprint {
    use super::*;
    @DECLARATIONS@
    pub struct ElfImport<'a> { pub name: &'a str }
    // Import discovery is deliberately a fixture boundary. Production prebind
    // consumes these names, including the original Paper async-FS ABI imports.
    pub fn elf_type_name(_: &[u8]) -> Option<&'static str> { Some("REL") }
    pub fn elf_imports(bytes: &[u8]) -> Result<Vec<ElfImport<'_>>, &'static str> {
        std::str::from_utf8(&bytes[4..]).map_err(|_| "fixture import encoding")
            .map(|names| names.split('\0').filter(|name| !name.is_empty())
                 .map(|name| ElfImport { name }).collect())
    }
    #[path = @PREBIND@] pub mod prebind;
    pub fn package(independent: bool, imports: &[&str]) -> Vec<u8> {
        let payload = [b"\x7fELF".as_slice(), imports.join("\0").as_bytes()].concat();
        let flags = BLUEPRINT_PAYLOAD_RAW
            | if independent { BLUEPRINT_CAP_FILESYSTEM_INDEPENDENT } else { 0 };
        let mut bytes = b"TRBP".to_vec();
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&flags.to_le_bytes());
        bytes.extend_from_slice(&0u64.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        bytes
    }
}
thread_local! {
    static DISK_CALLS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static LOGS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static READS: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static SCRIPT: RefCell<Option<Vec<u8>>> = const { RefCell::new(None) };
    static EXTENTS: RefCell<Vec<(u32,u32)>> = const { RefCell::new(Vec::new()) };
}
fn log(message: &str) { LOGS.with(|logs| logs.borrow_mut().push(message.into())); }
mod hv {
    pub fn hvlogf(_: std::fmt::Arguments<'_>) {}
    pub fn current_hull_guest_context_vm_id() -> Option<u8> { Some(1) }
    pub mod blueprint { pub fn app_fs_common_root() -> String { "apps/common".into() } }
}
mod allocators {
    pub fn with_hv_guest_alloc_domain<T>(_: u8, f: impl FnOnce() -> T) -> Option<T> { Some(f()) }
}
struct Timing;
impl Timing {
    fn measure<T, E>(&self, _: &str, f: impl FnOnce() -> Result<T,E>) -> Result<T,E> { f() }
}
fn create_blueprint_dir_all_async(path: &str) -> Result<(), String> {
    DISK_CALLS.with(|calls| calls.borrow_mut().push(path.into()));
    Ok(())
}
fn bootstrap(module: &blueprint::BlueprintModule<'_>, virtual_image_launch: bool) -> bool {
    let vm_id = 1;
    let app_fs_root = String::from("apps/paper");
    let timing = Timing;
    @BOOTSTRAP@
    true
}
mod trueos {
    pub mod async_fs {
        pub const ERR_NOT_FOUND: i32 = -2;
        pub fn block_on<T>(value: T) -> T { value }
        pub fn read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
            crate::READS.with(|reads| reads.borrow_mut().push(path.to_vec()));
            if path == b"vFile:launch" {
                return crate::SCRIPT.with(|script| script.borrow_mut().take()).ok_or(ERR_NOT_FOUND);
            }
            if path.starts_with(b"vFile:microfont-grid/") { return Ok(Vec::new()); }
            panic!("unexpected Paper read: {:?}", path);
        }
    }
    pub mod logl { pub mod level { pub const ERROR: u32 = 1; } pub fn log(_: u32, _: &str) {} }
}
mod ui4_scene {
    pub fn physical_extent(width: u32, height: u32) -> Result<(u32,u32), i32> {
        crate::EXTENTS.with(|extents| extents.borrow_mut().push((width,height)));
        Ok((width,height))
    }
}
fn ui(error: i32) -> i32 { error }
@GLYPH@
fn paper_startup() -> Result<(), i32> {
    @STARTUP@
    Ok(())
}
const PAPER_IMPORTS: &[&str] = &[
    "trueos_cabi_async_fs_read_start", "trueos_cabi_async_fs_status",
    "trueos_cabi_async_fs_result_len", "trueos_cabi_async_fs_result_read",
    "trueos_cabi_async_fs_discard",
];

#[test]
fn paper_capability_removes_only_its_async_fs_readiness_dependency() {
    use r::readiness::*;
    let bytes = blueprint::package(PAPER_INDEPENDENT, PAPER_IMPORTS);
    let module = blueprint::parse_blueprint(&bytes).unwrap();
    assert!(module.is_filesystem_independent(), "Paper metadata must set the existing capability");
    let (mask,payload) = blueprint::prebind::prebind_required_readiness(&bytes).unwrap();
    assert_eq!(mask, BACKGROUND_AP_WORKER_READY);
    assert_eq!(payload, module.payload);
    // Negative control: the identical original imports without the capability
    // reproduce the disk gate which prevented the RAM-only boot exercise.
    let ordinary = blueprint::package(false, PAPER_IMPORTS);
    assert_eq!(blueprint::prebind::prebind_required_readiness(&ordinary).unwrap().0,
               BACKGROUND_AP_WORKER_READY | TRUEOSFS_ROOT_MOUNTED);
}

#[test]
fn independent_apps_keep_disk_archive_network_and_unsupported_import_checks() {
    use r::readiness::*;
    for independent in [false,true] {
        let archive = blueprint::package(independent, &["trueos_cabi_archive_extract"]);
        assert_eq!(blueprint::prebind::prebind_required_readiness(&archive).unwrap().0,
                   BACKGROUND_AP_WORKER_READY | TRUEOSFS_ROOT_MOUNTED);
        let network = blueprint::package(independent, &["trueos_cabi_net_fetch_start"]);
        assert_eq!(blueprint::prebind::prebind_required_readiness(&network).unwrap().0,
                   BACKGROUND_AP_WORKER_READY | NET_ANY_CONFIGURED
                   | NET_SOCKET_READY | TLS_SOCKET_SERVICE_READY);
        let retired = blueprint::package(independent, &["trueos_cabi_fs_read"]);
        assert!(blueprint::prebind::prebind_required_readiness(&retired).unwrap_err()
            .contains("synchronous Blueprint filesystem ABI removed"));
    }
}

#[test]
fn real_guest_bootstrap_skips_disk_for_paper_but_preserves_ordinary_roots() {
    let bytes = blueprint::package(PAPER_INDEPENDENT, PAPER_IMPORTS);
    assert!(bootstrap(&blueprint::parse_blueprint(&bytes).unwrap(),false));
    assert!(DISK_CALLS.with(|calls| calls.borrow().is_empty()));
    assert!(LOGS.with(|logs| logs.borrow().iter().any(|line|
        line == "run: guest app fs bootstrap skipped contract=filesystem-independent")));
    let ordinary = blueprint::package(false,PAPER_IMPORTS);
    assert!(bootstrap(&blueprint::parse_blueprint(&ordinary).unwrap(),false));
    assert_eq!(DISK_CALLS.with(|calls| calls.borrow().clone()),["apps/paper","apps/common"]);
}

#[test]
fn paper_startup_is_font_free_and_keeps_demand_glyph_vfiles() {
    SCRIPT.with(|script| *script.borrow_mut() = Some(
        b"mode gridpaper\nsize 190x90\nstep 2\nwrap on\n".to_vec()));
    assert_eq!(paper_startup(),Ok(()));
    assert_eq!(READS.with(|reads| reads.borrow().clone()),
               [b"vFile:launch".to_vec()]);
    assert_eq!(EXTENTS.with(|extents| extents.borrow().clone()),[(190,90)]);
    assert!(SCRIPT.with(|script| script.borrow().is_none()));
    glyph_pixels(2,'A').unwrap();
    assert_eq!(READS.with(|reads| reads.borrow().clone()),
               [b"vFile:launch".to_vec(),b"vFile:microfont-grid/2/41".to_vec()]);
    READS.with(|reads| reads.borrow_mut().clear());
    EXTENTS.with(|extents| extents.borrow_mut().clear());
    assert_eq!(paper_startup(),Ok(()));
    assert_eq!(READS.with(|reads| reads.borrow().clone()),
               [b"vFile:launch".to_vec()]);
    assert_eq!(EXTENTS.with(|extents| extents.borrow().clone()),[(210,297)]);
    glyph_pixels(1,'§').unwrap();
    assert_eq!(READS.with(|reads| reads.borrow().clone()),
               [b"vFile:launch".to_vec(),b"vFile:microfont-grid/1/a7".to_vec()]);
}
'''.replace("@INDEPENDENT@", str(independent).lower()).replace(
        "@PAPER_SETTINGS@", settings
    ).replace("@PREBIND@", json.dumps(str(ROOT / "src/hv/blueprint/prebind.rs"))).replace(
        "@DECLARATIONS@", declarations
    ).replace("@READINESS@", readiness).replace("@BOOTSTRAP@", bootstrap).replace(
        "@GLYPH@", glyph
    ).replace("@STARTUP@", startup) + packaged_test


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package-dir", type=Path,
                        help="also check a prepared Paper package and its matching stripped ELF imports")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="trueos-paper-filesystem-capability-") as directory:
        source = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        source.write_text(harness(args.package_dir.resolve() if args.package_dir else None))
        subprocess.run([
            "rustc", "--edition=2024", "--target=x86_64-unknown-linux-gnu", "--test",
            "-C", "linker=" + str(ROOT / "tools/rust-host-ld"), str(source), "-o", str(binary),
        ], cwd=ROOT, check=True)
        subprocess.run([str(binary)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
