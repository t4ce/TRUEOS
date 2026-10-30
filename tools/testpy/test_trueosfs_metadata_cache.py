#!/usr/bin/env python3
"""Test production indexed metadata reads with a counting block-header backend."""

from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
SOURCE = (ROOT / "src/r/fs/trueosfs.rs").read_text()


def item(signature):
    start = SOURCE.index(signature)
    opening = SOURCE.index("{", start)
    depth, end = 1, opening + 1
    while depth:
        depth += (SOURCE[end] == "{") - (SOURCE[end] == "}")
        end += 1
    return SOURCE[start:end] + "\n"


HARNESS = r'''
#![allow(dead_code)]
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod block {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    pub struct DiscId(pub u32);
    #[derive(Clone, Copy)] pub struct DeviceHandle(pub DiscId);
    impl DeviceHandle { pub fn id(self) -> DiscId { self.0 } }
    #[derive(Debug, PartialEq)] pub enum Error { NotReady, Io }
}
mod disc { pub mod access {
    pub struct Activity;
    impl Activity { pub fn begin(_: crate::block::DeviceHandle) -> Result<Self, crate::block::Error> { Ok(Self) } }
} }
mod trueos_fs {
    #[derive(Clone, Copy)] pub enum LogKind { Put, Directory, Delete }
    #[derive(Clone, Copy, Debug, PartialEq)] pub struct NodeRecordRef { pub entry_lba: u64, pub data_len: u64 }
    pub struct FsParams { pub super_lba: u64, pub data_lba: u64, pub data_end_lba_exclusive: Option<u64> }
    pub async fn get_node_record_by_lba(io: &crate::KernelBlockIo, _: &FsParams, entry_lba: u64)
        -> Result<Option<NodeRecordRef>, crate::block::Error> {
        crate::READS.fetch_add(1, crate::Ordering::Relaxed);
        Ok(crate::HEADERS.lock().get(&(io.0.id(), entry_lba)).copied().flatten())
    }
}
struct KernelBlockIo(block::DeviceHandle);
impl KernelBlockIo { fn new(disk: block::DeviceHandle) -> Self { Self(disk) } }
struct TrueosFsPlacement { super_lba: u64, data_lba: u64, data_end_lba_exclusive: Option<u64> }
struct IndexRef { kind: trueos_fs::LogKind, entry_lba: u64 }
struct RootMount { disk_id: block::DiscId, seq: u32, index: Option<BTreeMap<Vec<u8>, IndexRef>> }
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());
static READS: AtomicUsize = AtomicUsize::new(0);
static HEADERS: Mutex<BTreeMap<(block::DiscId, u64), Option<trueos_fs::NodeRecordRef>>> = Mutex::new(BTreeMap::new());
const NODE_RECORD_CACHE_CAP: usize = 128;
static NODE_RECORD_CACHE_SEQ: AtomicU64 = AtomicU64::new(1);
static NODE_RECORD_CACHE: Mutex<Vec<NodeRecordCacheEntry>> = Mutex::new(Vec::new());
async fn ensure_index_async(_: block::DeviceHandle, _: &TrueosFsPlacement) -> Result<(), block::Error> { Ok(()) }
fn map_engine_err(error: block::Error) -> block::Error { error }
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct Wake;
    impl std::task::Wake for Wake { fn wake(self: std::sync::Arc<Self>) {} }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake));
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    match future.as_mut().poll(&mut context) {
        std::task::Poll::Ready(result) => result,
        std::task::Poll::Pending => panic!("fake header backend should finish immediately"),
    }
}
fn reset() {
    ROOTS.lock().clear(); HEADERS.lock().clear(); NODE_RECORD_CACHE.lock().clear();
    READS.store(0, Ordering::Relaxed);
}
fn install(disk: u32, seq: u32, lba: u64, len: u64) {
    ROOTS.lock().retain(|root| root.disk_id != block::DiscId(disk));
    let index = BTreeMap::from([(b"apps/config".to_vec(), IndexRef { kind: trueos_fs::LogKind::Put, entry_lba: lba })]);
    ROOTS.lock().push(RootMount { disk_id: block::DiscId(disk), seq, index: Some(index) });
    HEADERS.lock().insert((block::DiscId(disk), lba), Some(trueos_fs::NodeRecordRef { entry_lba: lba, data_len: len }));
}
fn lookup(disk: u32) -> Option<trueos_fs::NodeRecordRef> {
    block_on(lookup_node_via_index_async(block::DeviceHandle(block::DiscId(disk)),
        &TrueosFsPlacement { super_lba: 0, data_lba: 1, data_end_lba_exclusive: None }, "apps/config")).unwrap()
}
#[test] fn repeated_stats_and_unrelated_writes_do_not_reread_headers() {
    reset(); install(2, 7, 10, 5);
    assert_eq!(lookup(2).unwrap().data_len, 5);
    for lba in 100u64..612 {
        ROOTS.lock()[0].index.as_mut().unwrap().insert(lba.to_le_bytes().to_vec(),
            IndexRef { kind: trueos_fs::LogKind::Put, entry_lba: lba });
        assert_eq!(lookup(2).unwrap().data_len, 5);
    }
    assert_eq!(READS.load(Ordering::Relaxed), 1);
}
#[test] fn replacement_deletion_and_remount_follow_current_namespace() {
    reset(); install(2, 7, 10, 5); lookup(2);
    install(2, 7, 11, 9);
    assert_eq!(lookup(2).unwrap().data_len, 9);
    ROOTS.lock()[0].index.as_mut().unwrap().clear();
    assert_eq!(lookup(2), None);
    install(2, 8, 10, 20);
    assert_eq!(lookup(2).unwrap().data_len, 20);
    // A read admitted before remount may finish afterward; its old mount key
    // cannot poison the header now stored at the same physical address.
    node_record_cache_insert(block::DiscId(2), 7, trueos_fs::NodeRecordRef { entry_lba: 10, data_len: 5 });
    assert_eq!(lookup(2).unwrap().data_len, 20);
    assert_eq!(READS.load(Ordering::Relaxed), 3);
}
#[test] fn devices_and_missing_headers_do_not_share_cache_entries() {
    reset(); install(2, 7, 10, 5); install(3, 7, 10, 17);
    assert_eq!(lookup(2).unwrap().data_len, 5);
    assert_eq!(lookup(3).unwrap().data_len, 17);
    install(4, 7, 10, 25); HEADERS.lock().insert((block::DiscId(4), 10), None);
    assert_eq!(lookup(4), None);
    HEADERS.lock().insert((block::DiscId(4), 10), Some(trueos_fs::NodeRecordRef { entry_lba: 10, data_len: 25 }));
    assert_eq!(lookup(4).unwrap().data_len, 25);
    assert_eq!(READS.load(Ordering::Relaxed), 4);
}
#[test] fn cache_is_bounded_and_retains_recent_headers() {
    reset(); install(2, 7, 10, 5); lookup(2);
    for lba in 100..100 + NODE_RECORD_CACHE_CAP as u64 * 2 {
        assert!(lookup(2).is_some());
        node_record_cache_insert(block::DiscId(2), 7, trueos_fs::NodeRecordRef { entry_lba: lba, data_len: 0 });
        assert!(NODE_RECORD_CACHE.lock().len() <= NODE_RECORD_CACHE_CAP);
    }
    assert_eq!(READS.load(Ordering::Relaxed), 1);
}
'''


def main():
    source = HARNESS + "\n".join(item(signature) for signature in (
        "struct NodeRecordCacheEntry", "fn node_record_cache_lookup(",
        "fn node_record_cache_insert(", "async fn lookup_node_via_index_async(",
    ))
    with tempfile.TemporaryDirectory(prefix="trueos-metadata-cache-") as temporary:
        directory = Path(temporary)
        harness = directory / "tests.rs"
        harness.write_text(source)
        executable = directory / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(harness), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
