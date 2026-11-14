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
        #[derive(Debug, PartialEq)] enum OperationState { Read(Vec<u8>), Unit, Failed(i32) }
        fn selected_disk(path: &str) -> Result<(u8, &str), i32> { Ok((0, path)) }
        fn map_block_error(_: ()) -> i32 { FS_ERR_IO }
        @INSTALL@
        @APPDB_VIRTUAL_READ@
        pub fn read(path: &str) -> Option<Result<Vec<u8>, i32>> {
            appdb_virtual_read(path).map(|state| match state {
                OperationState::Read(bytes) => Ok(bytes),
                OperationState::Failed(code) => Err(code),
                _ => panic!("AppDB virtual files must be read-only"),
            })
        }
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
fn controller_roles_are_pinned_to_seeded_archive_and_bytes() {
    reset();
    let original = app_db::get("apps.bp").unwrap().unwrap();
    assert!(app_db::is_remote_input_controller("apps.bp", &original));
    // Copying an approved executable under another name cannot lend its role.
    assert!(!app_db::is_remote_input_controller("sqlite.bp", &original));
    app_db::insert_downloads(&[("sqlite.bp".into(), original.clone())]).unwrap();
    assert!(!app_db::is_remote_input_controller("sqlite.bp", &original));
    // An update retaining the capability bit cannot replace the approved code.
    let mut replacement = original.clone();
    replacement.push(1);
    assert!(!app_db::is_remote_input_controller("apps.bp", &replacement));
    app_db::insert_downloads(&[("apps.bp".into(), replacement.clone())]).unwrap();
    assert!(!app_db::is_remote_input_controller("apps.bp", &replacement));
    assert!(app_db::is_remote_input_controller("apps.bp", &original));
    // Reloading the database cannot approve a runtime replacement either.
    reset();
    assert!(app_db::is_remote_input_controller("apps.bp", &original));
    assert!(!app_db::is_remote_input_controller("apps.bp", &replacement));
}
#[test]
fn live_appdb_virtual_catalog_tracks_imports_without_changing_the_catalog() {
    reset();
    let before = app_db::list().unwrap();
    let read = |path: &str| io::install::read(path).unwrap().unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&read("vFile:appdb")).unwrap();
    assert_eq!(catalog, serde_json::json!({
        "version": 1, "apps": [{ "name": "apps", "archive": "apps.bp" }]
    }));
    assert_eq!(app_db::list().unwrap(), before);
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
    assert_eq!(read("vFile:appbundle.json"), include_bytes!(@JSON@));
    assert!(io::install::read("vFile:unknown").is_none());

    app_db::insert_downloads(&[download("sqlite.bp")]).unwrap();
    let catalog: serde_json::Value = serde_json::from_slice(&read("vFile:appdb")).unwrap();
    assert_eq!(catalog["apps"], serde_json::json!([
        { "name": "apps", "archive": "apps.bp" },
        { "name": "sqlite", "archive": "sqlite.bp" }
    ]));
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 1);
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
fn missing_xerj_rolls_back_updates_even_when_sqlite_is_installed() {
    reset();
    app_db::insert_downloads(&[download("sqlite.bp")]).unwrap();
    REFRESHES.store(0, Ordering::SeqCst);
    let before = app_db::list().unwrap();
    let old = app_db::get("apps.bp").unwrap();
    let error = app_db::insert_downloads(&[download("apps.bp"), download("codex.bp")]).unwrap_err();
    assert!(error.contains("required Blueprint xerj.bp is unavailable"));
    assert_eq!(app_db::list().unwrap(), before);
    assert_eq!(app_db::get("apps.bp").unwrap(), old);
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
}
#[test]
fn batch_publishes_all_and_refreshes_once_regardless_of_input_order() {
    reset();
    app_db::insert_downloads(&[download("codex.bp"), download("xerj-service.bp"), download("xerj.bp"), download("sqlite.bp"), download("lily2vec.bp")]).unwrap();
    assert_eq!(app_db::get("codex.bp").unwrap().unwrap(), download("codex.bp").1);
    assert_eq!(app_db::get("sqlite.bp").unwrap().unwrap(), download("sqlite.bp").1);
    assert_eq!(app_db::get("xerj.bp").unwrap().unwrap(), download("xerj.bp").1);
    assert_eq!(app_db::get("xerj-service.bp").unwrap().unwrap(), download("xerj-service.bp").1);
    assert_eq!(app_db::list().unwrap().len(), 6);
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 1);
}
#[test]
fn missing_lily_rolls_back_codex_when_other_requirements_are_installed() {
    reset();
    app_db::insert_downloads(&[download("sqlite.bp"), download("xerj.bp"), download("xerj-service.bp")]).unwrap();
    REFRESHES.store(0, Ordering::SeqCst);
    let before = app_db::list().unwrap();
    let error = app_db::insert_downloads(&[download("codex.bp")]).unwrap_err();
    assert!(error.contains("required Blueprint lily2vec.bp is unavailable"));
    assert_eq!(app_db::list().unwrap(), before);
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
    app_db::insert_downloads(&[download("codex.bp"), download("lily2vec.bp")]).unwrap();
    assert!(app_db::get("codex.bp").unwrap().is_some());
    assert!(app_db::get("lily2vec.bp").unwrap().is_some());
}
#[test]
fn lily_alone_succeeds_without_codex() {
    reset();
    app_db::insert_downloads(&[download("lily2vec.bp")]).unwrap();
    assert!(app_db::get("lily2vec.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_none());
}
#[test]
fn sqlite_alone_succeeds_without_codex() {
    reset();
    app_db::insert_downloads(&[download("sqlite.bp")]).unwrap();
    assert!(app_db::get("sqlite.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_none());
}
#[test]
fn xerj_alone_succeeds_without_codex_or_sqlite() {
    reset();
    app_db::insert_downloads(&[download("xerj.bp")]).unwrap();
    assert!(app_db::get("xerj.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert!(app_db::get("sqlite.bp").unwrap().is_none());
}
#[test]
fn missing_service_rolls_back_codex_even_when_its_engine_is_installed() {
    reset();
    app_db::insert_downloads(&[download("sqlite.bp"), download("xerj.bp")]).unwrap();
    REFRESHES.store(0, Ordering::SeqCst);
    let before = app_db::list().unwrap();
    let error = app_db::insert_downloads(&[download("codex.bp")]).unwrap_err();
    assert!(error.contains("required Blueprint xerj-service.bp is unavailable"));
    assert_eq!(app_db::list().unwrap(), before);
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
}
#[test]
fn service_requires_its_engine_without_requiring_codex() {
    reset();
    let error = app_db::insert_downloads(&[download("xerj-service.bp")]).unwrap_err();
    assert!(error.contains("required Blueprint xerj.bp is unavailable"));
    assert!(app_db::get("xerj-service.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
    app_db::insert_downloads(&[download("xerj-service.bp"), download("xerj.bp")]).unwrap();
    assert!(app_db::get("xerj-service.bp").unwrap().is_some());
    assert!(app_db::get("xerj.bp").unwrap().is_some());
    assert!(app_db::get("codex.bp").unwrap().is_none());
    assert_eq!(REFRESHES.load(Ordering::SeqCst), 1);
}
#[test]
fn missing_or_invalid_member_never_imports_any_of_the_batch() {
    for broken in ["sqlite.bp", "xerj.bp", "xerj-service.bp", "lily2vec.bp", "codex.bp"] {
        for invalid in [false, true] {
            reset(); stage("sqlite.bp"); stage("xerj.bp"); stage("xerj-service.bp"); stage("lily2vec.bp"); stage("codex.bp");
            let path = format!("common/dl/apps/{broken}");
            if invalid {
                FILES.lock().unwrap().insert(path, b"corrupt".to_vec());
            } else {
                FILES.lock().unwrap().remove(&path);
            }
            assert!(io::install::run(b"common/dl/apps/sqlite.bp\ncommon/dl/apps/xerj.bp\ncommon/dl/apps/xerj-service.bp\ncommon/dl/apps/lily2vec.bp\ncommon/dl/apps/codex.bp").is_err());
            for name in ["sqlite.bp", "xerj.bp", "xerj-service.bp", "lily2vec.bp", "codex.bp"] {
                assert!(app_db::get(name).unwrap().is_none());
            }
            assert_eq!(REFRESHES.load(Ordering::SeqCst), 0);
        }
    }
}
#[test]
fn batch_request_imports_all_and_keeps_single_path_compatibility() {
    reset(); stage("sqlite.bp"); stage("xerj.bp"); stage("xerj-service.bp"); stage("lily2vec.bp"); stage("codex.bp");
    io::install::run(b"common/dl/apps/sqlite.bp\ncommon/dl/apps/xerj.bp\ncommon/dl/apps/xerj-service.bp\ncommon/dl/apps/lily2vec.bp\ncommon/dl/apps/codex.bp").unwrap();
    assert!(app_db::get("sqlite.bp").unwrap().is_some());
    assert!(app_db::get("xerj.bp").unwrap().is_some());
    assert!(app_db::get("xerj-service.bp").unwrap().is_some());
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
#[cfg(not(feature = "buildin-apps"))]
#[test]
fn kernel_only_keeps_metadata_and_accepts_downloads_without_controller_roles() {
    assert_eq!(app_db::init_bsp().unwrap(), 0);
    assert!(app_db::list().unwrap().is_empty());
    assert_eq!(app_db::appbundle_json().unwrap(), include_bytes!(@JSON@));
    let controller = b"TRBP\x01\x00\x03\x10controller".to_vec();
    app_db::insert_downloads(&[("apps.bp".into(), controller.clone())]).unwrap();
    assert!(!app_db::is_remote_input_controller("apps.bp", &controller));
    assert!(app_db::get("apps.bp").unwrap().is_some());
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
        # A seeded approved controller in this fixture exercises the immutable
        # role independently of actual application names or mutable hash rows.
        (packages / 'apps.bp').write_bytes(b'TRBP' + bytes([1, 0, 3, 16]) + bytes(16))
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
        (packages / 'sqlite.bp').write_bytes(b'TRBP' + bytes(20))
        manifest.write_text('{"buildins":["apps","codex","sqlite"]}')
        result = subprocess.run([env['CARGO_TARGET_DIR'] + '/debug/trueos-appdb-seed', str(manifest), str(packages),
                                 str(seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, capture_output=True)
        assert result.returncode != 0 and b'xerj.bp is missing from the seed' in result.stderr
        assert seed.read_bytes() == original
        (packages / 'xerj.bp').write_bytes(b'TRBP' + bytes(20))
        manifest.write_text('{"buildins":["apps","codex","sqlite","xerj"]}')
        result = subprocess.run([env['CARGO_TARGET_DIR'] + '/debug/trueos-appdb-seed', str(manifest), str(packages),
                                 str(seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, capture_output=True)
        assert result.returncode != 0 and b'xerj-service.bp is missing from the seed' in result.stderr
        assert seed.read_bytes() == original
        (packages / 'xerj-service.bp').write_bytes(b'TRBP' + bytes(20))
        manifest.write_text('{"buildins":["apps","codex","sqlite","xerj","xerj-service"]}')
        result = subprocess.run([env['CARGO_TARGET_DIR'] + '/debug/trueos-appdb-seed', str(manifest), str(packages),
                                 str(seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, capture_output=True)
        assert result.returncode != 0 and b'lily2vec.bp is missing from the seed' in result.stderr
        assert seed.read_bytes() == original
        (packages / 'lily2vec.bp').write_bytes(b'TRBP' + bytes(20))
        manifest.write_text('{"buildins":["apps","codex","sqlite","xerj","xerj-service","lily2vec"]}')
        complete_seed = path / 'complete-seed.db'
        subprocess.run([env['CARGO_TARGET_DIR'] + '/debug/trueos-appdb-seed', str(manifest), str(packages),
                        str(complete_seed), '2026-11-08 00:00Z', str(BP / 'appbundle.json')], cwd=path, check=True)
        assert complete_seed.is_file()

        text = HARNESS.replace('@APPDB@', json.dumps(str(ROOT / 'src/app_db.rs')))
        text = text.replace('@JSON@', json.dumps(str(BP / 'appbundle.json')))
        text = text.replace('@INSTALL@', function(ROOT / 'src/r/io/async_fs_cabi.rs', 'async fn install_appdb('))
        text = text.replace('@APPDB_VIRTUAL_READ@', function(ROOT / 'src/r/io/async_fs_cabi.rs', 'fn appdb_virtual_read('))
        (path / 'src/lib.rs').write_text(text)
        (path / 'build.rs').write_text('''fn main() {
            let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
            let bytes = std::fs::read("seed.db").unwrap();
            std::fs::write(out.join("app.db.lz4"), &bytes).unwrap();
            let expected = if std::env::var_os("CARGO_FEATURE_BUILDIN_APPS").is_some() { 1 } else { 0 };
            std::fs::write(out.join("appdb-seed.rs"), format!("const APP_DB_IMAGE_LEN: usize = {};\\nconst APP_DB_EXPECTED_APPS: usize = {};", bytes.len(), expected)).unwrap();
        }''')
        (path / 'Cargo.toml').write_text('''[package]
name="trueos-appbundle-db-tests"
version="0.0.0"
edition="2024"
[workspace]
[features]
default=["buildin-apps"]
buildin-apps=[]
[dependencies]
redb={version="=4.4.0",default-features=false,features=["std","experimental-api-5"]}
sha2={version="=0.10.9",default-features=false}
serde_json={version="1.0",default-features=false,features=["alloc"]}
spin="0.10"
trueos-appbundle={path=''' + json.dumps(str(BP / 'crates/trueos-appbundle')) + '}\n')
        subprocess.run(['cargo', '+nightly-2026-07-10', 'test', '--offline', '--', '--test-threads=1'], cwd=path, env=env, check=True)
        subprocess.run(['cargo', '+nightly-2026-07-10', 'test', '--offline', '--no-default-features',
                        'kernel_only_keeps_metadata', '--', '--test-threads=1'], cwd=path, env=env, check=True)


if __name__ == '__main__':
    main()
