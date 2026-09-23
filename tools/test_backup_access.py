#!/usr/bin/env python3
"""Compile and test the actual no_std admission gate against host block handles."""
import json
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="trueos-backup-access-") as directory:
    project = Path(directory)
    (project / "Cargo.toml").write_text('''[package]
name = "trueos-backup-access-tests"
version = "0.0.0"
edition = "2024"
[dependencies]
spin = "0.10"
[workspace]
''')
    (project / "src").mkdir()
    source = r'''
extern crate alloc;
mod disc {
    pub mod block {
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub struct DiscId(pub u32);
        #[derive(Clone, Copy)]
        pub struct DeviceHandle(pub u32, pub Option<u32>);
        #[derive(Debug, PartialEq, Eq)]
        pub enum Error { NotReady, InvalidParam }
        pub type Result<T> = core::result::Result<T, Error>;
        impl DeviceHandle {
            pub fn id(self) -> DiscId { DiscId(self.0) }
            pub fn parent(self) -> Option<DiscId> { self.1.map(DiscId) }
        }
        pub fn device_handle(id: DiscId) -> Option<DeviceHandle> { Some(DeviceHandle(id.0, None)) }
    }
    #[path = SOURCE]
    pub mod access;
    #[cfg(test)]
    mod tests {
        use super::{access::{Activity, Exclusive}, block::*};
        #[test]
        fn complete_transaction_and_stream_lifetime_prevent_backup() {
            let disk = DeviceHandle(100, None);
            let transaction = Activity::begin(disk).unwrap();
            let io = Activity::begin(disk).unwrap();
            drop(io);
            assert_eq!(Exclusive::acquire(disk).err(), Some(Error::NotReady));
            drop(transaction);
            let backup = Exclusive::acquire(disk).unwrap();
            assert_eq!(Activity::begin(disk).err(), Some(Error::NotReady));
            assert_eq!(Exclusive::acquire(disk).err(), Some(Error::NotReady));
            drop(backup);
            assert!(Activity::begin(disk).is_ok());
        }
        #[test]
        fn partitions_share_whole_disk_ownership() {
            let disk = DeviceHandle(200, None);
            let partition = DeviceHandle(201, Some(200));
            let io = Activity::begin(partition).unwrap();
            assert_eq!(Exclusive::acquire(disk).err(), Some(Error::NotReady));
            drop(io);
            let backup = Exclusive::acquire(disk).unwrap();
            assert_eq!(Activity::begin(partition).err(), Some(Error::NotReady));
            assert_eq!(Exclusive::acquire(partition).err(), Some(Error::InvalidParam));
            assert!(Activity::begin(DeviceHandle(202, None)).is_ok());
            drop(backup);
            assert!(Activity::begin(partition).is_ok());
        }
        #[test]
        fn concurrent_admission_never_overlaps_backup() {
            use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
            let active = Arc::new(AtomicUsize::new(0));
            let threads: Vec<_> = (0..4).map(|_| {
                let active = active.clone();
                std::thread::spawn(move || {
                    for _ in 0..2000 {
                        if let Ok(io) = Activity::begin(DeviceHandle(300, None)) {
                            active.fetch_add(1, Ordering::SeqCst);
                            std::thread::yield_now();
                            active.fetch_sub(1, Ordering::SeqCst);
                            drop(io);
                        }
                    }
                })
            }).collect();
            for _ in 0..2000 {
                if let Ok(backup) = Exclusive::acquire(DeviceHandle(300, None)) {
                    assert_eq!(active.load(Ordering::SeqCst), 0);
                    std::thread::yield_now();
                    assert_eq!(active.load(Ordering::SeqCst), 0);
                    drop(backup);
                }
            }
            for thread in threads { thread.join().unwrap(); }
        }
        #[test]
        fn_error_scope_restores_admission() {
            fn fail(disk: DeviceHandle) -> Result<()> {
                let _lease = Exclusive::acquire(disk)?;
                Err(Error::InvalidParam)
            }
            let disk = DeviceHandle(400, None);
            assert!(fail(disk).is_err());
            assert!(Activity::begin(disk).is_ok());
        }
    }
}
'''.replace("SOURCE", json.dumps(str(root / "src/disc/access.rs"))).replace("fn_error_scope", "fn error_scope")
    (project / "src/lib.rs").write_text(source)
    subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                    "--target", "x86_64-unknown-linux-gnu"], cwd=project, check=True)
