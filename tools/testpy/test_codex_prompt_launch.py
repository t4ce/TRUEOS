#!/usr/bin/env python3
"""Exercise the production UI4 AppDB launch helper with host admission adapters."""
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]
source = (ROOT / 'src/ui4/codex_prompt.rs').read_text()
helper = source[source.index('fn launch_codex('):source.index('impl ColdLaunch')]
harness = r'''
use std::sync::Arc;
#[derive(Default)]
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
use std::string::String;
extern crate alloc;
#[derive(Default)]
struct LaunchState { run: Option<(u8, u64)> }
struct LaunchStatus(Arc<Mutex<LaunchState>>);
mod allcaps { pub mod hv { pub const VM_ID_LIMIT: usize = 2; } }
static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static INSTALL_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
mod app_db {
    pub fn get(name: &str) -> Result<Option<Vec<u8>>, String> {
        assert!(matches!(name, "codex.bp" | "apps.bp"));
        Ok((name == "apps.bp" || crate::INSTALLED.load(std::sync::atomic::Ordering::Relaxed))
            .then(|| vec![1]))
    }
}
mod hv {
    #[derive(Debug, PartialEq)]
    pub enum BlueprintConsoleSurface { Text, Terminal }
    pub struct State { pub running: bool, pub starting: bool }
    pub fn vm_state(_: u8) -> State { State { running: crate::RUNNING.load(std::sync::atomic::Ordering::Relaxed), starting: false } }
    pub fn vm_run_generation(_: u8) -> Option<u64> { Some(1) }
    pub fn blueprint_current_run_archive(_: u8, _: u64) -> Option<String> { Some("codex.bp".into()) }
    pub mod launcher {
        use super::BlueprintConsoleSurface;
        pub struct LaunchRequest {
            pub name: String,
            pub console_surface: BlueprintConsoleSurface,
            pub script: Option<String>,
            pub signal_codex_startup: bool,
        }
        impl LaunchRequest {
            pub fn new(name: String, bytes: Vec<u8>) -> Self {
                assert_eq!(bytes, vec![1]);
                Self { name, console_surface: BlueprintConsoleSurface::Text, script: None, signal_codex_startup: false }
            }
        }
        pub fn enqueue(request: LaunchRequest, observer: Option<Box<crate::LaunchStatus>>) -> Result<(), String> {
            use std::sync::atomic::Ordering::Relaxed;
            assert!(request.signal_codex_startup);
            assert!(observer.unwrap().0.lock().run.is_none());
            if request.name == "codex.bp" {
                assert_eq!(request.console_surface, BlueprintConsoleSurface::Terminal);
                assert!(request.script.is_none());
                crate::COUNT.fetch_add(1, Relaxed);
            } else {
                assert_eq!(request.name, "apps.bp");
                assert_eq!(request.script.as_deref(), Some("install codex\n"));
                crate::INSTALL_COUNT.fetch_add(1, Relaxed);
            }
            Ok(())
        }
    }
}
@HELPER@
fn main() {
    use std::sync::atomic::Ordering::Relaxed;
    let state = Arc::new(Mutex::default());
    assert!(!launch_codex(state.clone(), true).unwrap());
    assert_eq!(COUNT.load(Relaxed), 1);
    RUNNING.store(true, Relaxed);
    assert!(!launch_codex(state.clone(), true).unwrap());
    assert_eq!(state.lock().run, Some((0, 1)));
    assert_eq!(COUNT.load(Relaxed), 1, "existing VM must be reused");
    RUNNING.store(false, Relaxed);
    INSTALLED.store(false, Relaxed);
    let install = Arc::new(Mutex::default());
    assert!(launch_codex(install.clone(), true).unwrap());
    assert!(launch_codex(install, false).is_err());
    assert_eq!(INSTALL_COUNT.load(Relaxed), 1);
    assert_eq!(COUNT.load(Relaxed), 1);
    INSTALLED.store(true, Relaxed);
    assert!(!launch_codex(Arc::new(Mutex::default()), false).unwrap());
    assert_eq!(COUNT.load(Relaxed), 2);
}
'''.replace('@HELPER@', helper)
with tempfile.TemporaryDirectory(prefix='codex-prompt-launch-') as directory:
    path = pathlib.Path(directory)
    (path / 'main.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', str(path / 'main.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test')], check=True)
print('Codex prompt launch checks passed')
