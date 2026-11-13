#!/usr/bin/env python3
"""Exercise the production UI4 AppDB launch helper with host admission adapters."""
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
source = (ROOT / 'src/ui4/codex_prompt.rs').read_text()
helper = source[source.index('fn launch_codex('):source.index('/// Queue only copied references')]
harness = r'''
use std::sync::{Arc, Mutex};
use std::string::String;
extern crate alloc;
struct LaunchStatus(Arc<Mutex<Option<String>>>);
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 2; } }
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
mod app_db {
    pub fn get(name: &str) -> Result<Option<Vec<u8>>, String> {
        assert_eq!(name, "codex.bp");
        Ok(crate::INSTALLED.load(std::sync::atomic::Ordering::Relaxed).then(|| vec![1]))
    }
}
mod hv {
    pub struct State { pub running: bool, pub starting: bool }
    pub fn vm_state(_: u8) -> State { State { running: crate::RUNNING.load(std::sync::atomic::Ordering::Relaxed), starting: false } }
    pub fn vm_run_generation(_: u8) -> Option<u64> { Some(1) }
    pub fn blueprint_current_run_archive(_: u8, _: u64) -> Option<String> { Some("codex.bp".into()) }
    pub mod launcher {
        pub struct LaunchRequest;
        impl LaunchRequest { pub fn new(name: String, bytes: Vec<u8>) -> Self { assert_eq!(name,"codex.bp"); assert_eq!(bytes,vec![1]); Self } }
        pub fn enqueue(_: LaunchRequest, observer: Option<Box<crate::LaunchStatus>>) -> Result<(), String> {
            assert!(observer.is_some());
            crate::COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed); Ok(())
        }
    }
}
@HELPER@
fn main() {
    use std::sync::atomic::Ordering::Relaxed;
    let status = Arc::new(Mutex::new(None));
    launch_codex(status.clone()).unwrap();
    assert_eq!(COUNT.load(Relaxed), 1);
    RUNNING.store(true, Relaxed);
    launch_codex(status.clone()).unwrap();
    assert_eq!(COUNT.load(Relaxed), 1, "existing VM must be reused");
    RUNNING.store(false, Relaxed);
    INSTALLED.store(false, Relaxed);
    assert!(launch_codex(status).unwrap_err().contains("not installed"));
    assert_eq!(COUNT.load(Relaxed), 1);
}
'''.replace('@HELPER@', helper)
with tempfile.TemporaryDirectory(prefix='codex-prompt-launch-') as directory:
    path = pathlib.Path(directory)
    (path / 'main.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', str(path / 'main.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test')], check=True)
print('Codex prompt launch checks passed')
