#!/usr/bin/env python3
"""Exercise Codex persistence routing from the production volatile-files module.

The host harness compiles unchanged production items, with a small fake disk
selector and async directory creator. It never opens app data or credentials.
Run with ``python3 tools/testpy/test_codex_persistence.py``; only rustc is needed.
"""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "src/r/fs/volatile_files.rs").read_text()


def item(name: str) -> str:
    match = re.search(rf"^(?:pub\([^)]*\)\s+)?(?:async\s+)?fn {name}\b", SOURCE, re.M)
    if not match:
        raise ValueError(f"missing production function {name}")
    start = match.start()
    brace = SOURCE.index("{", match.start())
    depth = 0
    for index in range(brace, len(SOURCE)):
        if SOURCE[index] == "{":
            depth += 1
        elif SOURCE[index] == "}":
            depth -= 1
            if depth == 0:
                return SOURCE[start : index + 1]
    raise ValueError(f"unclosed production function {name}")


def verify_prepare_contract() -> None:
    prepare = item("prepare")
    ensure = prepare.index("ensure_durable_codex_home(source, root).await?;")
    early_return = prepare.index("if let Some(disk) = disk_for_path(root)")
    skip = prepare.index("if persistent_path(&path)")
    file_open = prepare.index("fs::file_read_open_async(source, &path)")
    if not (ensure < early_return and skip < file_open):
        raise AssertionError("prepare must ensure durable home before early return and skip it before copying files")
    if prepare.count("persistent_path(&path)") != 1:
        raise AssertionError("prepare must retain one explicit persistent-subtree seed exclusion")


def run_harness() -> None:
    functions = "\n".join(
        item(name)
        for name in (
            "contains",
            "persistent_path",
            "disk_for_path",
            "selected_disk",
            "ensure_durable_codex_home",
        )
    )
    harness = r'''
#![allow(dead_code, unused_imports, private_interfaces)]
use std::sync::Mutex as StdMutex;

pub struct Lock<T>(StdMutex<T>);
impl<T> Lock<T> {
    fn new(value: T) -> Self { Self(StdMutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub struct DeviceHandle(u8);
#[derive(Clone, Copy)] struct Storage { disk: DeviceHandle, initialized: bool }
static ROOTS: std::sync::LazyLock<Lock<Vec<String>>> =
    std::sync::LazyLock::new(|| Lock::new(Vec::new()));
static STORAGE: std::sync::LazyLock<Lock<Option<Storage>>> =
    std::sync::LazyLock::new(|| Lock::new(None));

#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum FsError { NoSpace, Injected }
type Result<T> = std::result::Result<T, FsError>;
static CREATE_RESULT: std::sync::LazyLock<Lock<Result<bool>>> =
    std::sync::LazyLock::new(|| Lock::new(Ok(true)));
static CREATED_PATH: std::sync::LazyLock<Lock<Option<String>>> =
    std::sync::LazyLock::new(|| Lock::new(None));

pub mod fs {
    use super::{DeviceHandle, Result, CREATED_PATH, CREATE_RESULT};
    pub fn primary_root_handle() -> Option<DeviceHandle> { Some(DeviceHandle(1)) }
    pub async fn dir_create_all_async(_disk: DeviceHandle, path: &str) -> Result<bool> {
        *CREATED_PATH.lock() = Some(path.to_owned());
        *CREATE_RESULT.lock()
    }
}
mod block { pub use crate::DeviceHandle; }
'''
    harness += functions
    harness = harness.replace("alloc::format!", "format!")
    harness += r'''
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    unsafe fn clone(_: *const ()) -> RawWaker { raw() }
    unsafe fn no_op(_: *const ()) {}
    fn raw() -> RawWaker { RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone, no_op, no_op, no_op)) }
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("fake async filesystem unexpectedly yielded"),
    }
}

#[test]
fn exact_persistent_subtrees_fall_through_to_primary() {
    *ROOTS.lock() = vec!["apps/codex".into(), "apps/codex-check".into()];
    *STORAGE.lock() = Some(Storage { disk: DeviceHandle(2), initialized: true });
    for path in ["apps/codex/.codex", "apps/codex/.codex/auth.json", "apps/codex-check/.codex/sessions/x"] {
        assert_eq!(disk_for_path(path), None, "{path}");
        assert_eq!(selected_disk(path), Some(DeviceHandle(1)), "{path}");
    }
}

#[test]
fn supporting_paths_remain_ram_and_namespace_prefixes_do_not_match() {
    *ROOTS.lock() = vec!["apps/codex".into(), "apps/codex-check".into()];
    *STORAGE.lock() = Some(Storage { disk: DeviceHandle(2), initialized: true });
    for path in ["apps/codex/tmp/file", "apps/codex/config", "apps/codex-check/tmp/x", "apps/codex/.codex-backup/x"] {
        assert_eq!(disk_for_path(path), Some(DeviceHandle(2)), "{path}");
    }
    for path in ["apps/codex-other/.codex/auth.json", "apps/codex-checkpoint/.codex/x"] {
        assert!(!persistent_path(path), "{path}");
        assert_eq!(disk_for_path(path), None, "{path}");
    }
    for path in ["apps/codex/.codex", "apps/codex/.codex/a/b", "apps/codex-check/.codex", "apps/codex-check/.codex/a"] {
        assert!(persistent_path(path), "{path}");
    }
    for path in ["apps/codex/.codex-other", "apps/codex-check/.codexx", "apps/codex/.codex2"] {
        assert!(!persistent_path(path), "{path}");
    }
}

#[test]
fn durable_home_creation_uses_requested_primary_path_and_propagates_errors() {
    *CREATE_RESULT.lock() = Ok(true);
    *CREATED_PATH.lock() = None;
    assert_eq!(block_on(ensure_durable_codex_home(DeviceHandle(1), "apps/codex")), Ok(()));
    assert_eq!(CREATED_PATH.lock().as_deref(), Some("apps/codex/.codex"));

    *CREATE_RESULT.lock() = Ok(false);
    assert_eq!(block_on(ensure_durable_codex_home(DeviceHandle(1), "apps/codex-check")), Err(FsError::NoSpace));
    assert_eq!(CREATED_PATH.lock().as_deref(), Some("apps/codex-check/.codex"));

    *CREATE_RESULT.lock() = Err(FsError::Injected);
    assert_eq!(block_on(ensure_durable_codex_home(DeviceHandle(1), "apps/codex")), Err(FsError::Injected));
}
'''

    with tempfile.TemporaryDirectory(prefix="codex-persistence-") as temporary:
        directory = Path(temporary)
        source = directory / "harness.rs"
        executable = directory / "harness"
        source.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], check=True)
        subprocess.run([str(executable)], check=True)


def main() -> None:
    verify_prepare_contract()
    run_harness()


if __name__ == "__main__":
    main()
