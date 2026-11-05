#!/usr/bin/env python3
"""Run the real SVG redb cache over a temporary TRUEOSFS-shaped host disk."""
from pathlib import Path
import json
import os
import struct
import subprocess
import tempfile
import zlib

ROOT = Path(__file__).resolve().parents[2]


def png(padding: int = 0) -> bytes:
    def chunk(kind: bytes, data: bytes) -> bytes:
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 8, 6, 0, 0, 0))
            + (chunk(b"tEXt", b"comment\0" + b"x" * padding) if padding else b"")
            + chunk(b"IDAT", zlib.compress(b"\0\x10\x20\x30\xff"))
            + chunk(b"IEND", b""))


HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::{AtomicBool,AtomicUsize,Ordering};
static FAIL_WRITE:AtomicBool=AtomicBool::new(false);
static REPLACE_AFTER_OPEN:AtomicBool=AtomicBool::new(false);
static FILE_READS:AtomicUsize=AtomicUsize::new(0);
static FILE_WRITES:AtomicUsize=AtomicUsize::new(0);
mod allocators {pub fn with_host_alloc_domain_strong<T>(f:impl FnOnce()->T)->T{f()}}
mod wait {
    pub struct EmbassySpinRawMutex(std::sync::Mutex<()>);
    unsafe impl embassy_sync::blocking_mutex::raw::RawMutex for EmbassySpinRawMutex {
        const INIT:Self=Self(std::sync::Mutex::new(()));
        fn lock<R>(&self,f:impl FnOnce()->R)->R{let _guard=self.0.lock().unwrap();f()}
    }
}
mod disc {pub mod block {#[derive(Clone,Copy)]pub struct DeviceHandle(pub u32);}}
fn file(disk:disc::block::DeviceHandle,path:&str)->std::path::PathBuf {
    std::path::Path::new(TEST_DISK).join(format!("disk-{}",disk.0)).join(path)
}
mod graphics {
    pub struct Error;impl Error{pub fn code(&self)->i32{-8}}
    pub mod webp_codec {pub fn inspect_webp(_: &[u8])->Result<(u32,u32),super::Error>{Err(super::Error)}}
    pub mod ico_codec {
        pub struct Frame{pub width:u32,pub height:u32}
        pub fn inspect_ico(_: &[u8])->Result<Frame,super::Error>{Err(super::Error)}
    }
}
mod r {pub mod fs {pub mod trueosfs {
    use crate::{disc::block::DeviceHandle,file,FILE_READS,FILE_WRITES,FAIL_WRITE,REPLACE_AFTER_OPEN,Ordering};
    use std::io::{Read,Seek};
    thread_local! {static OPEN:std::cell::RefCell<Vec<std::fs::File>>=const{std::cell::RefCell::new(Vec::new())};}
    #[derive(Clone,Copy)]pub struct FileReadHandle{index:usize,len:u64}
    impl FileReadHandle{pub const fn data_len(&self)->u64{self.len}}
    pub async fn file_read_open_async(disk:DeviceHandle,path:&str)->Result<Option<FileReadHandle>,()> {
        let target=file(disk,path);
        let opened=match std::fs::File::open(&target) {
            Ok(opened)=>opened,
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>return Ok(None),Err(_)=>return Err(())
        };
        let len=opened.metadata().map_err(|_|())?.len();
        let handle=OPEN.with(|files|{let mut files=files.borrow_mut();let index=files.len();files.push(opened);FileReadHandle{index,len}});
        if REPLACE_AFTER_OPEN.swap(false,Ordering::SeqCst) {
            let replacement=target.with_extension("external");
            std::fs::File::create(&replacement).unwrap().set_len(64*1024*1024+1).unwrap();
            std::fs::rename(replacement,&target).unwrap();
        }
        Ok(Some(handle))
    }
    pub async fn file_read_handle_range_async(handle:FileReadHandle,offset:u64,out:&mut[u8])->Result<Option<usize>,()> {
        FILE_READS.fetch_add(1,Ordering::SeqCst);
        assert!(out.len()<=64*1024,"production cache read exceeded bounded chunk");
        OPEN.with(|files|{
            let mut files=files.borrow_mut();let opened=&mut files[handle.index];
            opened.seek(std::io::SeekFrom::Start(offset)).map_err(|_|())?;
            opened.read(out).map(Some).map_err(|_|())
        })
    }
    pub async fn dir_create_all_async(disk:DeviceHandle,path:&str)->Result<bool,()> {
        std::fs::create_dir_all(file(disk,path)).map(|_|true).map_err(|_|())
    }
    pub async fn file_in_async(disk:DeviceHandle,path:&str,bytes:&[u8])->Result<bool,()> {
        FILE_WRITES.fetch_add(1,Ordering::SeqCst);
        if FAIL_WRITE.load(Ordering::SeqCst){return Err(())}
        let target=file(disk,path);let pending=target.with_extension("pending");
        std::fs::write(&pending,bytes).map_err(|_|())?;
        std::fs::rename(pending,target).map(|_|true).map_err(|_|())
    }
}}}
fn run<T>(future:impl core::future::Future<Output=T>)->T {
    let mut future=std::pin::pin!(future);
    let mut context=core::task::Context::from_waker(core::task::Waker::noop());
    match future.as_mut().poll(&mut context) {
        core::task::Poll::Ready(value)=>value,
        core::task::Poll::Pending=>panic!("mock filesystem operation unexpectedly blocked")
    }
}
#[cfg(test)]mod tests {
    use super::*;
    use disc::block::DeviceHandle;
    use trueos_redb::redb::ReadableDatabase;
    const PNG:&[u8]=include_bytes!("pixel.png");
    const DISK:DeviceHandle=DeviceHandle(1);
    fn path(name:&str)->String {format!("apps/hull/{name}/svg-assets.redb")}
    #[test]fn committed_png_survives_reopen_and_roots_disks_and_keys_are_independent() {
        let a=path("roundtrip");let b=path("other-root");
        assert!(run(cache::read(DISK,&a,"https://source.test/icon.svg?w=1")).is_none());
        run(cache::write(DISK,&a,"https://source.test/icon.svg?w=1",PNG)).unwrap();
        assert_eq!(run(cache::read(DISK,&a,"https://source.test/icon.svg?w=1")).unwrap(),PNG);
        assert!(run(cache::read(DISK,&a,"https://source.test/icon.svg?w=2")).is_none());
        assert!(run(cache::read(DISK,&b,"https://source.test/icon.svg?w=1")).is_none());
        assert!(run(cache::read(DeviceHandle(2),&a,"https://source.test/icon.svg?w=1")).is_none());
        let bytes=std::fs::read(file(DISK,&a)).unwrap();
        let reopened=trueos_redb::ImageDatabase::open(&bytes).unwrap();
        assert!(reopened.database().begin_read().is_ok());
    }
    #[test]fn corrupt_cache_is_a_miss_and_next_valid_conversion_replaces_it() {
        let p=path("corrupt");let target=file(DISK,&p);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();std::fs::write(&target,b"not a redb file").unwrap();
        assert!(run(cache::read(DISK,&p,"source")).is_none());
        run(cache::write(DISK,&p,"source",PNG)).unwrap();
        assert_eq!(run(cache::read(DISK,&p,"source")).unwrap(),PNG);
    }
    #[test]fn failed_commit_is_reported_without_losing_the_previous_durable_image() {
        let p=path("commit-failure");run(cache::write(DISK,&p,"old",PNG)).unwrap();
        let before=std::fs::read(file(DISK,&p)).unwrap();
        FAIL_WRITE.store(true,Ordering::SeqCst);
        assert!(run(cache::write(DISK,&p,"new",PNG)).is_err());
        FAIL_WRITE.store(false,Ordering::SeqCst);
        assert_eq!(std::fs::read(file(DISK,&p)).unwrap(),before);
        assert_eq!(run(cache::read(DISK,&p,"old")).unwrap(),PNG);
        assert!(run(cache::read(DISK,&p,"new")).is_none());
    }
    #[test]fn invalid_png_and_unbounded_keys_are_rejected_before_filesystem_work() {
        let p=path("invalid");FILE_WRITES.store(0,Ordering::SeqCst);
        for bytes in [b"<svg/>".as_slice(),b"not png",&PNG[..32]] {
            assert!(run(cache::write(DISK,&p,"source",bytes)).is_err());
        }
        assert!(run(cache::write(DISK,&p,&"x".repeat(8193),PNG)).is_err());
        assert_eq!(FILE_WRITES.load(Ordering::SeqCst),0);
        assert!(!file(DISK,&p).exists());
    }
    #[test]fn oversized_image_is_not_read_and_can_be_replaced_by_a_bounded_cache() {
        let p=path("oversize");let target=file(DISK,&p);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::File::create(&target).unwrap().set_len(64*1024*1024+1).unwrap();
        FILE_READS.store(0,Ordering::SeqCst);
        assert!(run(cache::read(DISK,&p,"source")).is_none());
        assert_eq!(FILE_READS.load(Ordering::SeqCst),0);
        run(cache::write(DISK,&p,"source",PNG)).unwrap();
        assert_eq!(run(cache::read(DISK,&p,"source")).unwrap(),PNG);
    }
    #[test]fn replacement_after_open_keeps_bounded_reads_on_the_original_record() {
        let p=path("replace-after-open");run(cache::write(DISK,&p,"source",PNG)).unwrap();
        REPLACE_AFTER_OPEN.store(true,Ordering::SeqCst);
        assert_eq!(run(cache::read(DISK,&p,"source")).unwrap(),PNG);
        assert!(std::fs::metadata(file(DISK,&p)).unwrap().len()>64*1024*1024);
        assert!(run(cache::read(DISK,&p,"source")).is_none());
    }
    #[test]fn entry_limit_resets_the_disposable_cache_and_keeps_the_new_conversion() {
        let p=path("entry-limit");
        for i in 0..256 {run(cache::write(DISK,&p,&format!("source-{i}"),PNG)).unwrap()}
        run(cache::write(DISK,&p,"source-0",PNG)).unwrap();
        assert!(run(cache::read(DISK,&p,"source-255")).is_some());
        run(cache::write(DISK,&p,"source-256",PNG)).unwrap();
        assert!(run(cache::read(DISK,&p,"source-0")).is_none());
        assert!(run(cache::read(DISK,&p,"source-255")).is_none());
        assert_eq!(run(cache::read(DISK,&p,"source-256")).unwrap(),PNG);
    }
    #[test]fn payload_limit_resets_before_retaining_more_than_32_mib() {
        let p=path("payload-limit");let large=include_bytes!("large.png").as_slice();
        for i in 0..4 {run(cache::write(DISK,&p,&format!("source-{i}"),large)).unwrap()}
        assert!(run(cache::read(DISK,&p,"source-0")).is_some());
        run(cache::write(DISK,&p,"source-4",large)).unwrap();
        assert!(run(cache::read(DISK,&p,"source-0")).is_none());
        assert!(run(cache::read(DISK,&p,"source-3")).is_none());
        assert_eq!(run(cache::read(DISK,&p,"source-4")).unwrap(),large);
    }
}
'''


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-hull-svg-cache-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/pixel.png").write_bytes(png())
        (project / "src/large.png").write_bytes(png(7 * 1024 * 1024))
        modules = (f"const TEST_DISK:&str={json.dumps(str(project / 'disk'))};\n"
                   f"#[path={json.dumps(str(ROOT / 'src/r/services/hull_image_policy.rs'))}] mod hull_image_policy;\n"
                   f"#[path={json.dumps(str(ROOT / 'src/r/services/hull_svg_cache.rs'))}] mod cache;\n")
        (project / "src/lib.rs").write_text(HARNESS + modules)
        redb = ROOT.parent / "TRUEOS-Blueprints/crates/trueos-redb"
        (project / "Cargo.toml").write_text(f'''[package]
name = "hull-svg-cache-host-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
trueos-redb = {{ path = {json.dumps(str(redb))}, default-features = false, features = ["std"] }}
redb = {{ version = "=4.4.0", default-features = false, features = ["experimental-api-5"] }}
embassy-sync = {{ version = "=0.8.0", default-features = false }}
sha2 = {{ version = "=0.10.9", default-features = false }}
''')
        environment = dict(os.environ)
        environment["CARGO_TARGET_DIR"] = str(ROOT / "tgt/hull-svg-cache-host-tests")
        environment["RUSTUP_TOOLCHAIN"] = "nightly-2026-07-10"
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                        "--", "--test-threads=1"], cwd=project, env=environment, check=True)


if __name__ == "__main__":
    main()
