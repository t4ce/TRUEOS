#!/usr/bin/env python3
"""Check that Microfont can start before any Lily frame is presented."""

from pathlib import Path
import subprocess
import tempfile

from test_mouse_visual_cursors import block


ROOT = Path(__file__).resolve().parents[2]
FONT = ROOT / "src/r/services/shell3_font_service.rs"


def harness() -> str:
    prepare = block(FONT.read_text(), "pub(crate) async fn prepare")
    return r'''
extern crate alloc;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::OnceLock;

static ELAPSED_MS: AtomicUsize = AtomicUsize::new(0);
static JOBS: AtomicUsize = AtomicUsize::new(0);
static RETRY_ONCE: AtomicBool = AtomicBool::new(false);

struct Duration(usize);

impl Duration {
    fn from_millis(ms: usize) -> Self {
        Self(ms)
    }
}

struct Timer;

impl Timer {
    async fn after(duration: Duration) {
        ELAPSED_MS.fetch_add(duration.0, Ordering::Relaxed);
    }
}

mod r {
    pub mod blocking {
        pub fn try_spawn_blocking_job_with_purpose<F: FnOnce()>(job: F, purpose: &str) -> Result<(), F> {
            assert_eq!(purpose, "shell3-font");
            crate::JOBS.fetch_add(1, crate::Ordering::Relaxed);
            job();
            Ok(())
        }
    }
}

static FONT: OnceLock<()> = OnceLock::new();
static PREPARING: AtomicBool = AtomicBool::new(false);

struct Reply(std::sync::Mutex<Option<Result<(), &'static str>>>);

impl Reply {
    fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    fn signal(&self, result: Result<(), &'static str>) {
        *self.0.lock().unwrap() = Some(result);
    }

    async fn wait(&self) -> Result<(), &'static str> {
        self.0.lock().unwrap().take().unwrap()
    }
}

fn prepare_once() -> Result<(), &'static str> {
    if RETRY_ONCE.swap(false, Ordering::Relaxed) {
        return Err("render-busy");
    }
    FONT.get_or_init(|| ());
    Ok(())
}
''' + prepare + r'''
fn prepare_font() {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let mut future = std::pin::pin!(prepare());
    assert_eq!(std::future::Future::poll(future.as_mut(), &mut cx), std::task::Poll::Ready(Ok(())));
}

#[test]
fn ready_font_work_starts_without_waiting_for_lily() {
    prepare_font();
    prepare_font();
    assert_eq!(ELAPSED_MS.load(Ordering::Relaxed), 0);
    assert_eq!(JOBS.load(Ordering::Relaxed), 1);
}

#[test]
fn render_busy_retries_on_the_existing_font_interval() {
    RETRY_ONCE.store(true, Ordering::Relaxed);
    prepare_font();
    assert_eq!(ELAPSED_MS.load(Ordering::Relaxed), 20);
    assert_eq!(JOBS.load(Ordering::Relaxed), 2);
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-font-bootstrap-") as directory:
        source, binary = Path(directory) / "tests.rs", Path(directory) / "tests"
        source.write_text(harness())
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)], check=True)
        names = [line.removesuffix(": test")
                 for line in subprocess.check_output([str(binary), "--list"], text=True).splitlines()
                 if line.endswith(": test")]
        for name in names:
            subprocess.run([str(binary), name, "--exact"], check=True)


if __name__ == "__main__":
    main()
