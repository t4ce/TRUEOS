#!/usr/bin/env python3
"""Exercise production file-world opening at controlled filesystem wait points.

The filesystem stub runs a competing open before returning, on the same host
thread. This deliberately interleaves the two operations without preemption.
A held registry lock fails immediately rather than hanging the test process.
The production registry, opening, materialization and cleanup code are extracted;
Mutexes, filesystem storage and descriptor fixtures are supplied by the host.
"""
from pathlib import Path
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    shim = ROOT / "src/std_abi_shim.rs"
    source = shim.read_text()
    definitions = source[source.index("pub(crate) struct RegularFileWorld {"):
                         source.index("enum SocketFd {")]
    definitions += "\n".join(function(shim, name) for name in (
        "open_regular_file_world", "materialize_regular_file_world",
        "cleanup_regular_file_world", "writeback_regular_file_world", "fsync",
    ))
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{string::String, vec::Vec, sync::Arc};
use core::ffi::c_int;
use core::sync::atomic::{AtomicI32, Ordering};
use std::{cell::RefCell, collections::BTreeMap, sync::Mutex as StdMutex};
#[path="@ROOT@/src/r/static_map.rs"] mod static_map;
use static_map::FixedKeyMap;
#[macro_export] macro_rules! log { ($($args:tt)*) => {{ let _ = format_args!($($args)*); }}; }
const OPEN_FILE_CAPACITY: usize = 4;
const TRUEOS_ENOENT: i32 = 2;
const TRUEOS_EIO: i32 = 5;
const TRUEOS_EAGAIN: i32 = 11;
const TRUEOS_EBADF: i32 = 9;
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
struct Mutex<T>(StdMutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self { Self(StdMutex::new(value)) }
    fn lock(&self)->std::sync::MutexGuard<'_,T> {
        self.0.try_lock().expect("cooperative operation spun on a held mutex")
    }
}
static REGULAR_FILE_WORLDS: Mutex<RegularFileWorldRegistry> =
    Mutex::new(RegularFileWorldRegistry::new());
#[derive(Clone)] enum OpenFile {
    Regular { path:Option<String>, world:Arc<Mutex<RegularFileWorld>>, lock:Arc<()> },
    Other,
}
static OPEN_FILES: Mutex<FixedKeyMap<c_int,OpenFile,4>> = Mutex::new(FixedKeyMap::new());
fn trace_posix_file_world(_:Option<&str>)->bool { false }
#[derive(Clone,Copy,PartialEq,Eq)] enum Stage { Read, Metadata, Write }
struct Disk {
    bytes: BTreeMap<String,Vec<u8>>,
    failure: Option<(Stage,i32)>,
    reads: usize,
    metadata: usize,
}
std::thread_local! {
    static DISK: RefCell<Disk> = RefCell::new(Disk {
        bytes:BTreeMap::new(), failure:None, reads:0, metadata:0,
    });
    static HANDOFF: RefCell<Option<(Stage,Box<dyn FnOnce()>)>> = RefCell::new(None);
}
mod r { pub mod io { pub mod kfs {
    #[derive(Clone,Copy)] pub struct FsStat {
        pub revision:u64, pub file_id:u64, pub root_id:u32,
    }
} } }
fn filesystem_boundary(stage:Stage) {
    // Assert the contract even when no competing operation is installed.
    drop(REGULAR_FILE_WORLDS.0.try_lock().expect("registry held across filesystem wait"));
    drop(OPEN_FILES.0.try_lock().expect("descriptor table held across filesystem wait"));
    let handoff = HANDOFF.with(|h| {
        let mut h=h.borrow_mut();
        if h.as_ref().is_some_and(|(at,_)| *at==stage) { h.take() } else { None }
    });
    if let Some((_,run))=handoff { run(); }
}
fn read_file_from_cabi(path:&str)->Result<Vec<u8>,c_int> {
    filesystem_boundary(Stage::Read);
    DISK.with(|disk| {
        let mut disk=disk.borrow_mut();disk.reads+=1;
        if let Some((Stage::Read,errno))=disk.failure { return Err(errno); }
        disk.bytes.get(path).cloned().ok_or(TRUEOS_ENOENT)
    })
}
fn regular_file_metadata(path:&str)->Result<r::io::kfs::FsStat,c_int> {
    filesystem_boundary(Stage::Metadata);
    DISK.with(|disk| {
        let mut disk=disk.borrow_mut();disk.metadata+=1;
        if let Some((Stage::Metadata,errno))=disk.failure { return Err(errno); }
        if !disk.bytes.contains_key(path) { return Err(TRUEOS_ENOENT); }
        Ok(r::io::kfs::FsStat { revision:9, file_id:17, root_id:2 })
    })
}
fn write_file_to_cabi(path:&str,bytes:&[u8])->Result<(),c_int> {
    filesystem_boundary(Stage::Write);
    DISK.with(|disk| {
        let mut disk=disk.borrow_mut();
        if let Some((Stage::Write,errno))=disk.failure { return Err(errno); }
        disk.bytes.insert(path.into(),bytes.into());Ok(())
    })
}
@DEFINITIONS@
#[cfg(test)] mod tests {
    use super::*;
    fn reset() {
        *REGULAR_FILE_WORLDS.lock()=RegularFileWorldRegistry::new();
        *OPEN_FILES.lock()=FixedKeyMap::new();TRUEOS_ERRNO.store(0,Ordering::Relaxed);
        DISK.with(|d| { let mut d=d.borrow_mut();d.bytes.clear();d.failure=None;d.reads=0;d.metadata=0; });
        HANDOFF.with(|h| *h.borrow_mut()=None);
    }
    fn disk(path:&str,bytes:&[u8]) { DISK.with(|d| { d.borrow_mut().bytes.insert(path.into(),bytes.into()); }); }
    fn handoff(stage:Stage,run:impl FnOnce()+'static) { HANDOFF.with(|h| *h.borrow_mut()=Some((stage,Box::new(run)))); }
    fn open(path:&str)->Arc<Mutex<RegularFileWorld>> { open_regular_file_world(path,false,false,false).unwrap().0 }
    fn descriptor(path:&str)->(Arc<Mutex<RegularFileWorld>>,std::sync::Weak<()>) {
        let world=open(path);world.lock().dirty=true;
        let lock=Arc::new(());let weak=Arc::downgrade(&lock);
        assert!(OPEN_FILES.lock().insert(7,OpenFile::Regular { path:Some(path.into()),world:world.clone(),lock }).is_ok());
        (world,weak)
    }
    #[test] fn fsync_allows_peer_open_during_writeback_and_retains_a_closed_description() {
        for stage in [Stage::Write,Stage::Metadata] {
            reset();disk("a",b"A");disk("b",b"B");let (world,weak)=descriptor("a");
            let observed=weak.clone();
            handoff(stage,move || {
                assert_eq!(open("b").lock().bytes,b"B");
                OPEN_FILES.lock().remove(7);assert_eq!(observed.strong_count(),1);
            });
            assert_eq!(unsafe { fsync(7) },0);assert!(!world.lock().dirty);
            assert_eq!(weak.strong_count(),0);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),0);
        }
    }
    #[test] fn fsync_preserves_a_peer_write_that_occurs_while_the_snapshot_is_stored() {
        reset();disk("a",b"A");let (world,_)=descriptor("a");let peer=world.clone();
        handoff(Stage::Write,move || {
            let mut w=peer.lock();w.bytes=b"later write".to_vec();w.revision+=1;
        });
        assert_eq!(unsafe { fsync(7) },0);
        let w=world.lock();assert_eq!(w.bytes,b"later write");assert!(w.dirty);
        DISK.with(|d| assert_eq!(d.borrow().bytes.get("a").unwrap(),b"A"));
    }
    #[test] fn fsync_errors_preserve_dirty_data_and_release_the_description() {
        reset();disk("a",b"A");let (world,weak)=descriptor("a");
        DISK.with(|d| d.borrow_mut().failure=Some((Stage::Write,TRUEOS_EIO)));
        assert_eq!(unsafe { fsync(7) },-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EIO);
        assert!(world.lock().dirty);assert_eq!(weak.strong_count(),1);
        for fd in [-1,9] { assert_eq!(unsafe { fsync(fd) },-1);assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed),TRUEOS_EBADF); }
    }
    #[test] fn different_paths_can_open_during_both_filesystem_waits() {
        for stage in [Stage::Read,Stage::Metadata] {
            reset();disk("a",b"A");disk("b",b"B");
            handoff(stage,|| { assert_eq!(open("b").lock().bytes,b"B"); });
            assert_eq!(open("a").lock().bytes,b"A");
            assert!(REGULAR_FILE_WORLDS.lock().get("b").is_some());
        }
    }
    #[test] fn missing_file_does_not_block_a_peer_or_leave_a_registry_entry() {
        reset();disk("b",b"B");handoff(Stage::Read,|| { open("b"); });
        assert!(matches!(open_regular_file_world("absent",false,false,false),Err(TRUEOS_ENOENT)));
        assert!(REGULAR_FILE_WORLDS.lock().get("absent").is_none());
        assert!(REGULAR_FILE_WORLDS.lock().get("b").is_some());
    }
    #[test] fn same_path_race_reuses_the_published_world_and_preserves_peer_writes() {
        for stage in [Stage::Read,Stage::Metadata] {
            reset();disk("a",b"disk");
            let winner=Arc::new(StdMutex::new(None));let captured=winner.clone();
            handoff(stage,move || {
                let world=open("a");
                { let mut w=world.lock();w.bytes=b"peer write".to_vec();w.dirty=true;w.revision=23; }
                *captured.lock().unwrap()=Some(world);
            });
            let (world,reused)=open_regular_file_world("a",false,false,true).unwrap();
            assert!(reused);assert!(Arc::ptr_eq(&world,winner.lock().unwrap().as_ref().unwrap()));
            let w=world.lock();assert_eq!(w.bytes,b"peer write");assert_eq!(w.revision,23);assert!(w.dirty);
        }
    }
    #[test] fn truncating_racer_applies_truncation_to_the_winner_once() {
        reset();disk("a",b"disk");
        handoff(Stage::Metadata,|| { let w=open("a");w.lock().revision=41; });
        let (world,reused)=open_regular_file_world("a",true,true,true).unwrap();
        assert!(reused);let w=world.lock();assert!(w.bytes.is_empty());assert!(w.dirty);assert_eq!(w.revision,42);
    }
    #[test] fn cached_truncation_needs_no_filesystem_call_and_readonly_truncation_is_ignored() {
        reset();disk("a",b"disk");let original=open("a");
        let (same,reused)=open_regular_file_world("a",false,true,false).unwrap();
        assert!(reused);assert!(Arc::ptr_eq(&same,&original));assert_eq!(same.lock().bytes,b"disk");
        let (same,reused)=open_regular_file_world("a",false,true,true).unwrap();
        assert!(reused);let w=same.lock();assert!(w.bytes.is_empty());assert!(w.dirty);assert_eq!(w.revision,1);
        DISK.with(|d| { let d=d.borrow();assert_eq!((d.reads,d.metadata),(1,1)); });
    }
    #[test] fn missing_created_world_retains_dirty_and_metadata_semantics() {
        for writable in [false,true] {
            reset();let (world,reused)=open_regular_file_world("new",true,false,writable).unwrap();
            assert!(!reused);let w=world.lock();assert!(w.bytes.is_empty());assert_eq!(w.dirty,writable);
            assert_eq!(w.revision,u64::from(writable));assert_eq!((w.file_id,w.root_id,w.disk_revision),(0,0,0));
        }
    }
    #[test] fn failed_materialization_can_reuse_a_concurrent_creator() {
        for stage in [Stage::Read,Stage::Metadata] {
            reset();
            if stage==Stage::Metadata {
                disk("new",b"disk");
                DISK.with(|d| d.borrow_mut().failure=Some((stage,TRUEOS_ENOENT)));
            }
            handoff(stage,|| { open_regular_file_world("new",true,true,true).unwrap(); });
            let (world,reused)=open_regular_file_world("new",false,false,false).unwrap();
            assert!(reused);assert!(world.lock().dirty);
        }
    }
    #[test] fn filesystem_errors_leave_the_registry_unlocked_and_unpublished() {
        for stage in [Stage::Read,Stage::Metadata] {
            reset();disk("a",b"A");DISK.with(|d| d.borrow_mut().failure=Some((stage,TRUEOS_EIO)));
            assert!(matches!(open_regular_file_world("a",false,false,false),Err(TRUEOS_EIO)));
            assert!(REGULAR_FILE_WORLDS.lock().get("a").is_none());
        }
    }
    #[test] fn capacity_and_cleanup_preserve_shared_world_identity() {
        reset();
        for path in ["a","b","c","d"] { disk(path,b"data");open(path); }
        disk("e",b"extra");
        assert!(matches!(open_regular_file_world("e",false,false,false),Err(TRUEOS_EAGAIN)));
        let held=open("a");let peer=open("a");cleanup_regular_file_world(Some("a"),&held);
        assert!(REGULAR_FILE_WORLDS.lock().get("a").is_some());drop(peer);
        cleanup_regular_file_world(Some("a"),&held);assert!(REGULAR_FILE_WORLDS.lock().get("a").is_none());
        assert_eq!(open("e").lock().bytes,b"extra");
    }
}
'''.replace("@DEFINITIONS@", definitions).replace("@ROOT@", str(ROOT))
    with tempfile.TemporaryDirectory(prefix="trueos-file-world-") as temp:
        path = Path(temp) / "tests.rs"
        path.write_text(harness)
        binary = Path(temp) / "tests"
        subprocess.run(["rustc", "--edition=2024", "--test", str(path), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True, timeout=30)


if __name__ == "__main__":
    main()
