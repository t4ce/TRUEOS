#!/usr/bin/env python3
"""Exercise production async filesystem owner quiescence and release on the host."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    production = "\n".join(item("src/r/io/async_fs_cabi.rs", name) for name in (
        "pending_in", "pending_for_owner", "release_owner", "take_request",
        "finish_request", "discard",
    ))
    harness = r'''
#![allow(dead_code)]
use std::collections::{BTreeMap, VecDeque};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
struct Request { id: u32, owner: u32 }
enum OperationState { Pending, Upload(Vec<u8>), Unit, Read(Vec<u8>) }
struct Operation { owner: u32, state: OperationState }
const FS_ERR_NOT_FOUND: i32 = -1;
static ASYNC_FS_REQUESTS: Mutex<VecDeque<Request>> = Mutex::new(VecDeque::new());
static ASYNC_FS_ACTIVE_REQUEST: Mutex<Option<(u32, u32)>> = Mutex::new(None);
static ASYNC_FS_OPERATIONS: Mutex<BTreeMap<u32, Operation>> = Mutex::new(BTreeMap::new());
static TEST_SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn reset() -> std::sync::MutexGuard<'static, ()> {
    let guard = TEST_SERIAL.lock().unwrap();
    ASYNC_FS_REQUESTS.lock().clear();
    *ASYNC_FS_ACTIVE_REQUEST.lock() = None;
    ASYNC_FS_OPERATIONS.lock().clear();
    guard
}
fn insert(id: u32, owner: u32, state: OperationState) {
    ASYNC_FS_OPERATIONS.lock().insert(id, Operation { owner, state });
}
fn enqueue(id: u32, owner: u32) {
    insert(id, owner, OperationState::Pending);
    ASYNC_FS_REQUESTS.lock().push_back(Request { id, owner });
}
#[test]
fn discarded_running_write_blocks_release_until_filesystem_completion() {
    let _guard = reset();
    enqueue(1, 10);
    let write = take_request().unwrap();
    assert_eq!(pending_for_owner(10), 1);
    assert_eq!(discard(10, 1), 0);
    assert!(ASYNC_FS_OPERATIONS.lock().is_empty());
    assert_eq!(pending_for_owner(10), 1, "discard must not hide a live disk transaction");
    assert_eq!(release_owner(10), Err(1));
    finish_request(&write, Some(OperationState::Unit));
    assert_eq!(pending_for_owner(10), 0);
    assert_eq!(release_owner(10), Ok(0));
}
#[test]
fn discarded_queue_entry_stays_pending_until_service_skips_it() {
    let _guard = reset();
    enqueue(2, 10);
    assert_eq!(discard(10, 2), 0);
    assert_eq!(pending_for_owner(10), 1);
    assert_eq!(release_owner(10), Err(1));
    let cancelled = take_request().unwrap();
    assert_eq!(pending_for_owner(10), 1);
    finish_request(&cancelled, None);
    assert_eq!(release_owner(10), Ok(0));
}
#[test]
fn pause_retains_upload_and_result_and_stop_releases_only_its_owner() {
    let _guard = reset();
    insert(3, 10, OperationState::Upload(vec![1, 2, 3]));
    insert(4, 10, OperationState::Read(vec![4, 5]));
    insert(5, 20, OperationState::Upload(vec![6]));
    assert_eq!(pending_for_owner(10), 0);
    assert_eq!(ASYNC_FS_OPERATIONS.lock().len(), 3, "pause observation preserves handles");
    assert_eq!(release_owner(10), Ok(2));
    assert!(ASYNC_FS_OPERATIONS.lock().contains_key(&5));
}
#[test]
fn submitting_and_queued_work_block_release_without_removing_results() {
    let _guard = reset();
    insert(6, 10, OperationState::Pending);
    enqueue(7, 10);
    insert(8, 10, OperationState::Read(vec![8]));
    enqueue(9, 20);
    assert_eq!(pending_for_owner(10), 2, "queued requests must not count twice");
    assert_eq!(release_owner(10), Err(2));
    assert!(ASYNC_FS_OPERATIONS.lock().contains_key(&8));
    assert_eq!(pending_for_owner(20), 1);
    assert_eq!(pending_for_owner(30), 0);
}
#[test]
fn completion_publishes_result_before_owner_becomes_quiescent() {
    let _guard = reset();
    enqueue(10, 10);
    let request = take_request().unwrap();
    finish_request(&request, Some(OperationState::Read(vec![1, 2])));
    assert_eq!(pending_for_owner(10), 0);
    assert!(matches!(ASYNC_FS_OPERATIONS.lock().get(&10).unwrap().state,
                     OperationState::Read(_)));
    assert_eq!(release_owner(10), Ok(1));
}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-async-fs-lifecycle-") as directory:
        path = Path(directory)
        source = path / "lifecycle.rs"
        source.write_text(harness + production)
        binary = path / "lifecycle"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu",
             str(source), "-o", str(binary)], check=True, cwd=ROOT,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
