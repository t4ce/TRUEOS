#!/usr/bin/env python3
"""Exercise the host seed, production AppDB and atomic install request on host.

Only archive readiness, filesystem authorization/reads and the LZ4 boundary are
substituted. AppDB uses the real redb seed image and production transactions.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
BP = ROOT.parent / 'TRUEOS-Blueprints'


def function(path, name):
    text = path.read_text()
    start = text.index(name)
    end = text.index('{', start) + 1
    depth = 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[start:end]


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::{collections::BTreeMap, sync::{Mutex, atomic::{AtomicUsize, Ordering}}};
#[macro_export] macro_rules! log_error { ($($tt:tt)*) => {} }
static REFRESHES: AtomicUsize = AtomicUsize::new(0);
static FILES: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
#[path = @APPDB@] mod app_db;
mod shell3 { pub mod service {
    pub fn refresh_appdb_names() { crate::REFRESHES.fetch_add(1, crate::Ordering::SeqCst); }
} }
mod hv {
    pub fn with_guest_broker_context<T>(_: u8, f: impl FnOnce() -> T) -> T { f() }
    pub mod blueprint {
        pub fn prebind_required_readiness(bytes: &[u8]) -> Result<(), ()> {
            if bytes.starts_with(b"TRBP") { Ok(()) } else { Err(()) }
        }
    }
}
mod access_key_db { pub async fn audit_path(_: u8, _: u8, _: &str, _: &str, _: bool) {} }
mod r {
    pub mod lz4 {
        pub fn decompress_frame_cpu(bytes: &[u8], _: usize) -> Result<Vec<u8>, ()> { Ok(bytes.to_vec()) }
    }
    pub mod fs { pub mod trueosfs { pub mod authorized {
        pub struct Info { pub data_len: u64 }
        pub async fn node_info(_: u8, _: u8, path: &str) -> Result<Option<Info>, ()> {
            Ok(crate::FILES.lock().unwrap().get(path).map(|b| Info { data_len: b.len() as u64 }))
        }
        pub async fn read(_: u8, _: u8, path: &str, _: u64) -> Result<Option<Vec<u8>>, ()> {
            Ok(crate::FILES.lock().unwrap().get(path).cloned())
        }
    } } }
}
mod io {
    pub mod env {
        pub fn resolve_blueprint_archive_path(path: &str) -> Result<String, ()> {
            if path.contains("..") { Err(()) } else { Ok(path.into()) }
        }
    }
    pub mod install {
        use alloc::{string::String, vec::Vec};
        const FS_ERR_BAD_PATH: i32 = -1;
        const FS_ERR_NOT_FOUND: i32 = -8;
        const FS_ERR_TOO_LARGE: i32 = -7;
        const FS_ERR_BAD_PARAM: i32 = -9;
        const FS_ERR_IO: i32 = -2;
        struct Request { owner: u32, actor: u8 }
        #[derive(Debug, PartialEq)] enum OperationState { Unit, Failed(i32) }
        fn selected_disk(path: &str) -> Result<(u8, &str), i32> { Ok((0, path)) }
        fn map_block_error(_: ()) -> i32 { FS_ERR_IO }
        @INSTALL@
        pub fn run(paths: &[u8]) -> Result<(), i32> {
            let request = Request { owner: 0x8000_0001, actor: 1 };
            let mut future = std::pin::pin!(install_appdb(&request, paths));
            use std::{future::Future, task::{Context, Poll, Waker}};
            match future.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(OperationState::Unit) => Ok(()),
                Poll::Ready(OperationState::Failed(code)) => Err(code),
                _ => panic!("fixture unexpectedly parked"),
            }
        }
    }
}
fn reset() {
    assert_eq!(app_db::init_bsp().unwrap(), 1);
    REFRESHES.store(0, Ordering::SeqCst);
    FILES.lock().unwrap().clear();
}
fn download(name: &str) -> (String, Vec<u8>) { (name.into(), format!("TRBP-{name}").into_bytes()) }
fn stage(name: &str) {
    let (name, bytes) = download(name);
    FILES.lock().unwrap().insert(format!("common/dl/apps/{name}"), bytes);
}
#[test]
fn seeded_metadata_matches_source_and_is_not_a_launchable_app() {
    reset();
    assert_eq!(app_db::appbundle_json().unwrap(), include_bytes!(@JSON@));
    assert_eq!(app_db::list().unwrap().len(), 1);
    assert!(app_db::get("appbundle.json").unwrap().is_none());
}
#[test]
fn incomplete_bundle_rolls_back_all_bytes_hashes_and_dates() {
    reset();
    let before = app_db::list().unwrap();
    let old = app_db::get("apps.bp").unwrap();
    let error = app_db::insert_downloads(&[download("apps.bp"), download("codex.bp")]).unwrap_err();
    assert!(error.contains("required Blueprint sqlite.bp is unavailable"));
    assert_eq!(app_db::list().unwrap(), before);
    assert_eq!(app_db::get("apps.bp").unwrap(), old);
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
}
#[test]
fn batch_publishes_both_and_refreshes_once_regardless_of_input_order() {
    reset();
    app_db::insert_downloads(&[download("codex.bp"), download("sqlite.bp")]).unwrap();
    assert_eq!(app_db::get("codex.bp").unwrap().unwrap(), download("codex.bp").1);
    assert_eq!(app_db::get("sqlite.bp").unwrap().unwrap(), download("sqlite.bp").1);
    assert_eq!(app_db::list().unwrap().len(), 3);
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 1);
}
#[test]
fn sqlite_alone_succeeds_without_codex() {
    reset();
    app_db::insert_downloads(&[download("sqlite.bp")]).unwrap();
    assert!(app_db::get("sqlite.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_none());
}
#[test]
fn missing_or_invalid_second_file_never_imports_the_first() {
    for invalid in [false, true] {
        reset(); stage("sqlite.bp");
        if invalid { FILES.lock().unwrap().insert("common/dl/apps/codex.bp".into(), b"corrupt".to_vec()); }
        assert!(io::install::run(b"common/dl/apps/sqlite.bp\ncommon/dl/apps/codex.bp").is_err());
        assert!(app_db::get("sqlite.bp").unwrap().is_none());
        assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
    }
}
#[test]
fn batch_request_imports_both_and_keeps_single_path_compatibility() {
    reset(); stage("sqlite.bp"); stage("codex.bp");
    io::install::run(b"common/dl/apps/sqlite.bp\ncommon/dl/apps/codex.bp").unwrap();
    assert!(app_db::get("sqlite.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_some());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 1);
    reset(); stage("sqlite.bp");
    io::install::run(b"common/dl/apps/sqlite.bp").unwrap();
}
#[test]
fn empty_duplicate_and_escaping_batches_are_rejected() {
    reset(); stage("sqlite.bp");
    for paths in ["", "common/../sqlite.bp", "common/dl/apps/sqlite.bp\n\ncommon/dl/apps/sqlite.bp", "common/dl/apps/sqlite.bp\ncommon/dl/apps/sqlite.bp"] {
        assert!(io::install::run(paths.as_bytes()).is_err());
        assert!(app_db::get("sqlite.bp").unwrap().is_none());
    }
}
'''


def main():
    env = dict(os.environ)
    env.setdefault('CARGO_TARGET_DIR', str(ROOT / 'tgt/appbundle-db-tests'))
    with tempfile.TemporaryDirectory(prefix='trueos-appbundle-') as directory:
        path = Path(directory)
        (path / 'src').mkdir()
        packages = path / 'packages'
        packages.mkdir()
        (packages / 'apps.bp').write_bytes(b'TRBP' + bytes(20))
        manifest = path / 'buildins.json'
        manifest.write_text('{"buildins":["apps"]}')
        seed = path / 'seed.db'
        subprocess.run(['cargo', '+nightly-2026-07-10', 'run', '--offline', '--quiet', '--manifest-path',
                        str(ROOT / 'tools/appdb-seed/Cargo.toml'), '--', str(manifest), str(packages),
                        str(seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, env=env, check=True)
        # A malformed or partial seed must not replace the last complete image.
        original = seed.read_bytes()
        (packages / 'codex.bp').write_bytes(b'TRBP' + bytes(20))
        manifest.write_text('{"buildins":["apps","codex"]}')
        result = subprocess.run([env['CARGO_TARGET_DIR'] + '/debug/trueos-appdb-seed', str(manifest), str(packages),
                                 str(seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, capture_output=True)
        assert result.returncode != 0 and b'sqlite.bp is missing from the seed' in result.stderr
        assert seed.read_bytes() == original

        text = HARNESS.replace('@APPDB@', json.dumps(str(ROOT / 'src/app_db.rs')))
        text = text.replace('@JSON@', json.dumps(str(BP / 'appbundle.json')))
        text = text.replace('@INSTALL@', function(ROOT / 'src/r/io/async_fs_cabi.rs', 'async fn install_appdb('))
        (path / 'src/lib.rs').write_text(text)
        (path / 'build.rs').write_text('''fn main() {
            let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
            let bytes = std::fs::read("seed.db").unwrap();
            std::fs::write(out.join("app.db.lz4"), &bytes).unwrap();
            std::fs::write(out.join("appdb-seed.rs"), format!("const APP_DB_IMAGE_LEN: usize = {};\\nconst APP_DB_EXPECTED_APPS: usize = 1;", bytes.len())).unwrap();
        }''')
        (path / 'Cargo.toml').write_text('''[package]
name="trueos-appbundle-db-tests"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
redb={version="=4.4.0",default-features=false,features=["std","experimental-api-5"]}
sha2={version="=0.10.9",default-features=false}
spin="0.10"
trueos-appbundle={path=''' + json.dumps(str(BP / 'crates/trueos-appbundle')) + '}\n')
        subprocess.run(['cargo', '+nightly-2026-07-10', 'test', '--offline', '--', '--test-threads=1'], cwd=path, env=env, check=True)


if __name__ == '__main__':
    main()
