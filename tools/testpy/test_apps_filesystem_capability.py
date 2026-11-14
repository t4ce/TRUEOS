#!/usr/bin/env python3
"""Exercise RAM AppDB import and per-app readiness without disks or a VM.

Production import, wire decoder and readiness policy run unchanged. Blueprint
ELF validation and redb commit are fixture boundaries, observed for atomicity.
"""
from pathlib import Path
import json
import os
import subprocess
import tempfile
import tomllib

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]
BUNDLE = ROOT.parent / "TRUEOS-Blueprints/crates/trueos-appbundle"


def main():
    metadata = tomllib.loads((ROOT.parent / "TRUEOS-Buildins/apps/Cargo.toml").read_text())
    assert metadata["package"]["metadata"]["trueos-blueprint"]["filesystem-independent"] is True
    source = r'''
extern crate alloc;
use std::cell::RefCell;
#[macro_export] macro_rules! log_error { ($($t:tt)*) => {{}} }
mod r { pub mod readiness {
    pub const BACKGROUND_AP_WORKER_READY:u32=1;
    pub const TRUEOSFS_ROOT_MOUNTED:u32=2;
    pub const RAYON_READY:u32=4;
    pub const NET_ANY_CONFIGURED:u32=8;
    pub const NET_SOCKET_READY:u32=16;
    pub const TLS_SOCKET_SERVICE_READY:u32=32;
    pub const INTEL_HDA_READY:u32=64;
}}
mod hv { pub mod blueprint {
    pub fn prebind_required_readiness(bytes: &[u8]) -> Result<(), String> {
        crate::VALIDATED.with_borrow_mut(|seen| seen.push(bytes.to_vec()));
        if bytes == b"bad" { Err("invalid ELF".into()) } else { Ok(()) }
    }
}}
thread_local! {
    static VALIDATED: RefCell<Vec<Vec<u8>>> = const { RefCell::new(Vec::new()) };
    static COMMITS: RefCell<usize> = const { RefCell::new(0) };
}
mod app_db {
    pub fn insert_downloads(downloads: &[(String, Vec<u8>)]) -> Result<(), String> {
        assert_eq!(crate::VALIDATED.with_borrow(|seen| seen.len()), downloads.len());
        crate::COMMITS.with_borrow_mut(|count| *count += 1);
        Ok(())
    }
}
struct Request { owner: u32 }
#[derive(Debug, PartialEq)] enum OperationState { Unit, Failed(i32) }
const FS_ERR_BAD_PATH:i32=-1; const FS_ERR_BAD_PARAM:i32=-2; const FS_ERR_IO:i32=-3;
@OWNER@
@IMPORT@
@BASE@
@POLICY@
@RAYON@
#[test] fn validates_entire_batch_before_one_commit() {
    let mut bytes = Vec::new();
    trueos_appbundle::append_ram_install(&mut bytes, "helper.bp", b"helper").unwrap();
    trueos_appbundle::append_ram_install(&mut bytes, "app.bp", b"app").unwrap();
    assert_eq!(install_appdb_ram(&Request { owner: 0x80000001 }, &bytes), OperationState::Unit);
    assert_eq!(COMMITS.with_borrow(|count| *count), 1);
}
#[test] fn invalid_member_and_non_guest_never_commit() {
    let mut bytes = Vec::new();
    trueos_appbundle::append_ram_install(&mut bytes, "helper.bp", b"helper").unwrap();
    trueos_appbundle::append_ram_install(&mut bytes, "app.bp", b"bad").unwrap();
    assert_eq!(install_appdb_ram(&Request { owner: 1 }, &bytes), OperationState::Failed(FS_ERR_BAD_PATH));
    assert!(VALIDATED.with_borrow(|seen| seen.is_empty()));
    assert_eq!(install_appdb_ram(&Request { owner: 0x80000001 }, &bytes), OperationState::Failed(FS_ERR_BAD_PARAM));
    assert_eq!(COMMITS.with_borrow(|count| *count), 0);
}
#[test] fn apps_contract_keeps_network_and_child_contract_keeps_filesystem() {
    use r::readiness::*;
    assert_eq!(prebind_import_readiness("trueos_cabi_async_fs_read_start", true), 0);
    assert_eq!(prebind_import_readiness("trueos_cabi_async_fs_read_start", false), TRUEOSFS_ROOT_MOUNTED);
    assert_eq!(prebind_import_readiness("trueos_cabi_net_fetch_start", true),
        NET_ANY_CONFIGURED | NET_SOCKET_READY | TLS_SOCKET_SERVICE_READY);
    assert_ne!(prebind_import_readiness("trueos_cabi_archive_open", true) & TRUEOSFS_ROOT_MOUNTED, 0);
}
'''
    fs = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
    prebind = (ROOT / "src/hv/blueprint/prebind.rs").read_text()
    for placeholder, text, name in [
        ("OWNER", fs, "vm_for_owner"), ("IMPORT", fs, "install_appdb_ram"),
        ("BASE", prebind, "prebind_base_readiness"),
        ("POLICY", prebind, "prebind_import_readiness"),
        ("RAYON", prebind, "is_rayon_import"),
    ]:
        source = source.replace(f"@{placeholder}@", item(text, "fn " + name))
    with tempfile.TemporaryDirectory(prefix="trueos-apps-no-fs-") as directory:
        project = Path(directory)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(source)
        (project / "Cargo.toml").write_text(
            '[package]\nname="apps-no-fs-policy-tests"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\ntrueos-appbundle={path=' + json.dumps(str(BUNDLE)) + '}\n')
        env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT / "tgt/apps-boundary-tests-host"))
        subprocess.run(["cargo", "+nightly-2026-07-10", "test", "--offline", "--target", "x86_64-unknown-linux-gnu"],
                       cwd=project, env=env, check=True)


if __name__ == "__main__":
    main()
