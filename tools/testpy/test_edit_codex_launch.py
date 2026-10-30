#!/usr/bin/env python3
"""Exercise Edit -> launch ABI -> kernel admission using production functions.

Only the VM scheduler, AppDB catalog and downloader child are substituted.
No node reboot or network access is needed for these lifecycle/failure cases.
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
EDIT = ROOT.parent / 'TRUEOS-Buildins/edit/crates/edit/src/bin/edit/ide.rs'


def function(path, name):
    text = path.read_text()
    start = text.index(name)
    body = text.index('{', start)
    depth = 1
    end = body + 1
    while depth:
        depth += (text[end] == '{') - (text[end] == '}')
        end += 1
    return text[start:end]


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::{cell::RefCell, io, rc::Rc, time::Instant};
use trueos_ide::protocol;
#[derive(Default)]
struct World {
    installed: bool, catalog_failed: bool, helper_done: bool,
    helper_starts: usize, helper_drops: usize,
    launches: Vec<(String, Option<String>, String)>,
}
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
mod app_db {
    pub struct AppEntry { pub archive: String }
    pub fn list() -> Result<Vec<AppEntry>, ()> {
        crate::WORLD.with(|w| {
            let w = w.borrow();
            if w.catalog_failed { Err(()) }
            else if w.installed { Ok(vec![AppEntry { archive: "codex.bp".into() }]) }
            else { Ok(vec![]) }
        })
    }
}
mod hv { pub fn blueprint_console_target(_: u8) -> Option<()> { Some(()) } }
mod shell3 { pub mod tui {
    pub type Frontend = ();
    pub fn frontend_for_target(_: &()) -> Option<Frontend> { Some(()) }
} }
mod workers {
    pub struct Worker;
    pub fn pick_background_spawner() -> Option<Worker> { Some(Worker) }
    impl Worker { pub fn spawn(&self, _: ()) {} }
}
mod adapter {
    use alloc::string::String;
    #[path = @PARSER@] mod launch_request;
    mod env { pub fn resolve_blueprint_archive_path(p: &str) -> Result<String, i32> { Ok(p.into()) } }
    mod request {
        use super::*;
        @KERNEL@
        fn launch_archive_task(_: Option<()>, archive: String, _: Option<String>, script: Option<String>, destination: launch_request::Destination) -> Result<(), ()> {
            crate::WORLD.with(|w| w.borrow_mut().launches.push((archive, script, format!("{destination:?}"))));
            Ok(())
        }
    }
    pub fn launch(payload: &[u8]) -> i32 { request::launch_script_request(0, payload) }
}
mod trueos { pub mod vshell {
    #[derive(Clone, Copy)]
    pub enum LaunchDestination { CurrentShell, NewShell3, Headless }
    mod vcabi {
        pub unsafe fn trueos_cabi_blueprint_launch_script_v1(ptr: *const u8, len: usize) -> i32 {
            crate::adapter::launch(unsafe { std::slice::from_raw_parts(ptr, len) })
        }
    }
    @SDK@
} }
struct Registration { endpoint: u64 }
struct Project { _registration: Registration }
struct IdeSession {
    project: Rc<RefCell<Project>>,
    codex_download: Option<(OwnedChild, String)>,
}
struct DiagnosticTail { bytes: Vec<u8> }
struct OwnedChild { started: Instant, diagnostics: Option<DiagnosticTail> }
impl OwnedChild {
    fn spawn_script(program: &str, script: &str) -> io::Result<Self> {
        assert_eq!((program, script), ("apps.bp", "install codex\n"));
        WORLD.with(|w| w.borrow_mut().helper_starts += 1);
        Ok(Self { started: Instant::now(), diagnostics: Some(DiagnosticTail { bytes: b"download diagnostic".to_vec() }) })
    }
    fn poll(&mut self) -> io::Result<bool> { WORLD.with(|w| Ok(w.borrow().helper_done)) }
}
impl Drop for OwnedChild { fn drop(&mut self) { WORLD.with(|w| w.borrow_mut().helper_drops += 1); } }
impl IdeSession {
    @STARTING@
    @START@
    @POLL@
}
@LAUNCH@
fn session() -> IdeSession {
    WORLD.with(|w| *w.borrow_mut() = World::default());
    IdeSession { project: Rc::new(RefCell::new(Project { _registration: Registration { endpoint: 42 } })), codex_download: None }
}
#[test]
fn installed_codex_launches_with_exact_endpoint_in_its_own_shell() {
    let mut s = session();
    WORLD.with(|w| w.borrow_mut().installed = true);
    s.start_codex().unwrap();
    assert!(!s.is_starting_codex());
    WORLD.with(|w| {
        let w=w.borrow();
        assert_eq!(w.helper_starts, 0);
        assert_eq!(w.launches, [("codex.bp".into(), Some("ide-endpoint 42\n".into()), "NewShell3".into())]);
    });
}
#[test]
fn missing_codex_downloads_once_then_launches_original_attachment() {
    let mut s = session();
    s.start_codex().unwrap(); s.start_codex().unwrap();
    assert!(s.is_starting_codex());
    assert!(!s.poll_codex_download().unwrap());
    WORLD.with(|w| { let mut w=w.borrow_mut(); assert!(w.launches.is_empty()); assert_eq!(w.helper_starts,1); w.installed=true; w.helper_done=true; });
    assert!(s.poll_codex_download().unwrap());
    assert!(!s.is_starting_codex());
    WORLD.with(|w| { let w=w.borrow(); assert_eq!(w.helper_drops,1); assert_eq!(w.launches[0].1.as_deref(),Some("ide-endpoint 42\n")); });
}
#[test]
fn failed_download_is_visible_and_releases_retry_state() {
    let mut s = session(); s.start_codex().unwrap();
    WORLD.with(|w| w.borrow_mut().helper_done=true);
    let error=s.poll_codex_download().unwrap_err().to_string();
    assert!(error.contains("(-2)") && error.contains("download diagnostic"));
    assert!(!s.is_starting_codex());
    WORLD.with(|w| assert!(w.borrow().launches.is_empty()));
    s.start_codex().unwrap();
    WORLD.with(|w| assert_eq!(w.borrow().helper_starts,2));
}
#[test]
fn unavailable_catalog_does_not_trigger_a_download_or_admit_a_launch() {
    let mut s=session(); WORLD.with(|w| w.borrow_mut().catalog_failed=true);
    assert!(s.start_codex().unwrap_err().to_string().contains("(-5)"));
    WORLD.with(|w| { let w=w.borrow(); assert!(w.launches.is_empty()); assert_eq!(w.helper_starts,0); });
}
#[test]
fn closing_during_download_cleans_only_the_helper() {
    let mut s=session(); s.start_codex().unwrap(); drop(s);
    WORLD.with(|w| { let w=w.borrow(); assert_eq!(w.helper_drops,1); assert!(w.launches.is_empty()); });
}
#[test]
fn explicit_archive_paths_keep_the_existing_launch_path() {
    let _s=session();
    assert_eq!(adapter::launch(b"/apps/common/dl/apps/codex.bp\0ide-endpoint 42\n\0sh3"),0);
    WORLD.with(|w| assert_eq!(w.borrow().launches.len(),1));
}
mod startup {
    use std::sync::{Mutex, OnceLock};
    use trueos_ide::protocol;
    static STARTUP_ENDPOINT: OnceLock<u64> = OnceLock::new();
    static READ: Mutex<Result<Vec<u8>, i32>> = Mutex::new(Err(-8));
    mod trueos_v { pub mod vfs_async {
        pub const ERR_NOT_FOUND: i32 = -8;
        pub async fn read_file(_: &[u8]) -> Result<Vec<u8>, i32> { super::super::READ.lock().unwrap().clone() }
    } }
    @INITIALIZE@
    fn initialize() -> Result<(), String> {
        let mut future = Box::pin(initialize_launch_script());
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        match std::future::Future::poll(future.as_mut(), &mut cx) {
            std::task::Poll::Ready(result) => result,
            _ => panic!("fixture read is immediately ready"),
        }
    }
    #[test]
    fn ordinary_launch_and_failed_streams_do_not_bind_an_ide() {
        initialize().unwrap();
        assert!(STARTUP_ENDPOINT.get().is_none());
        *READ.lock().unwrap() = Err(-2);
        assert!(initialize().unwrap_err().contains("(-2)"));
        *READ.lock().unwrap() = Ok(b"ide-endpoint not-an-id\n".to_vec());
        assert!(initialize().is_err());
        assert!(STARTUP_ENDPOINT.get().is_none());
        *READ.lock().unwrap() = Ok(b"ide-endpoint 42\n".to_vec());
        initialize().unwrap();
        assert_eq!(STARTUP_ENDPOINT.get(), Some(&42));
    }
}
'''


def main():
    text = HARNESS.replace('@PARSER@', json.dumps(str(ROOT / 'src/r/io/launch_request.rs')))
    kernel = function(ROOT / 'src/r/io/fs_cabi.rs', 'fn launch_script_request(')
    text = text.replace('@KERNEL@', kernel.replace('fn launch_script_request', 'pub(super) fn launch_script_request', 1))
    text = text.replace('@SDK@', function(ROOT.parent / 'TRUEOS-Buildins/vendor/trueos-v/src/vshell.rs', 'pub fn launch_with_destination('))
    text = text.replace('@INITIALIZE@', function(ROOT.parent / 'codex/codex-rs/core/src/trueos_ide.rs', 'pub async fn initialize_launch_script('))
    for marker, name in [('@START@', 'pub fn start_codex('), ('@STARTING@', 'pub fn is_starting_codex('), ('@POLL@', 'fn poll_codex_download('), ('@LAUNCH@', 'fn launch_codex(')]:
        source = function(EDIT, name)
        source = source.replace('        #[cfg(not(target_os = "trueos"))]\n        return Err(io::Error::new(io::ErrorKind::Unsupported, "Codex VMs require TRUEOS"));\n', '')
        source = source.replace('        #[cfg(not(target_os = "trueos"))]\n        return false;\n', '')
        source = source.replace('        #[cfg(target_os = "trueos")]\n', '')
        text = text.replace(marker, source)
    with tempfile.TemporaryDirectory(prefix='trueos-edit-codex-launch-') as t:
        t = Path(t)
        (t / 'src').mkdir()
        (t / 'src/lib.rs').write_text(text)
        (t / 'Cargo.toml').write_text('[package]\nname="edit-codex-launch-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\ntrueos-ide={path=' + json.dumps(str(ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-ide')) + ',default-features=false}\n')
        env = dict(os.environ)
        env.setdefault('CARGO_TARGET_DIR', '/tmp/trueos-edit-codex-launch-target')
        subprocess.run(['cargo', '+nightly-2026-07-10', 'test', '--offline'], cwd=t, env=env, check=True)


if __name__ == '__main__':
    main()
