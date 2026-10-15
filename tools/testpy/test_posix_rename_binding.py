#!/usr/bin/env python3
"""Compile the production POSIX rename shim against a deterministic async CABI."""
from pathlib import Path
import subprocess
import tempfile

from test_indexed_fs_remove import extract

SOURCE = Path(__file__).resolve().parents[2] / "src/std_abi_shim.rs"

HARNESS = r'''
#![allow(non_camel_case_types, dead_code)]
use std::ffi::{c_char, c_int, CStr};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::Mutex;

static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
static EXISTS: AtomicI32 = AtomicI32::new(1);
static START: AtomicI32 = AtomicI32::new(7);
static POLLS: AtomicUsize = AtomicUsize::new(0);
static DISCARDS: AtomicUsize = AtomicUsize::new(0);
static STATUS: Mutex<Vec<i32>> = Mutex::new(Vec::new());
static PATHS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());
const TRUEOS_EINVAL: c_int = 22;
const TRUEOS_ENOENT: c_int = 2;

fn abi_cstr_to_string(ptr: *const c_char, _: usize) -> Option<String> {
    if ptr.is_null() { return None; }
    Some(unsafe { CStr::from_ptr(ptr) }.to_str().ok()?.to_owned())
}
fn fs_rc_to_errno(code: i32) -> c_int { if code == -5 { 2 } else { 5 } }

mod r {
    pub mod path {
        pub struct FsPath(String);
        impl FsPath {
            pub fn parse(path: &str, _: bool) -> Result<Self, ()> { Ok(Self(path.into())) }
            pub fn to_relative_string(self) -> String { self.0 }
        }
    }
    pub mod io {
        pub mod env {
            pub fn resolve_fs_path(path: &str, _: bool) -> Option<String> { Some(path.into()) }
        }
        pub mod cabi {
            pub unsafe fn trueos_cabi_fs_exists(_: *const u8, _: usize) -> i32 {
                crate::EXISTS.load(std::sync::atomic::Ordering::Relaxed)
            }
            pub fn trueos_cabi_poll_once() {
                crate::POLLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        pub mod async_fs_cabi {
            pub const FS_ERR_ALREADY_EXISTS: i32 = -9;
            pub unsafe fn trueos_cabi_async_fs_rename_start(
                old: *const u8, old_len: usize, new: *const u8, new_len: usize,
            ) -> i32 {
                let old = unsafe { std::slice::from_raw_parts(old, old_len) };
                let new = unsafe { std::slice::from_raw_parts(new, new_len) };
                crate::PATHS.lock().unwrap().push((String::from_utf8_lossy(old).into_owned(), String::from_utf8_lossy(new).into_owned()));
                crate::START.load(std::sync::atomic::Ordering::Relaxed)
            }
            pub fn trueos_cabi_async_fs_status(_: u32) -> i32 {
                crate::STATUS.lock().unwrap().remove(0)
            }
            pub fn trueos_cabi_async_fs_discard(_: u32) -> i32 {
                crate::DISCARDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                0
            }
        }
    }
}

fn reset(start: i32, statuses: &[i32], exists: i32) {
    START.store(start, Ordering::Relaxed);
    EXISTS.store(exists, Ordering::Relaxed);
    POLLS.store(0, Ordering::Relaxed);
    DISCARDS.store(0, Ordering::Relaxed);
    STATUS.lock().unwrap().clear();
    STATUS.lock().unwrap().extend_from_slice(statuses);
    PATHS.lock().unwrap().clear();
}

#[test]
fn rename_uses_one_async_namespace_operation_and_discards_it() {
    reset(7, &[0, 1], 1);
    assert_eq!(unsafe { rename(c"old".as_ptr(), c"new".as_ptr()) }, 0);
    assert_eq!(PATHS.lock().unwrap().as_slice(), &[("old".into(), "new".into())]);
    assert_eq!(POLLS.load(Ordering::Relaxed), 1);
    assert_eq!(DISCARDS.load(Ordering::Relaxed), 1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), 0);
}

#[test]
fn existing_destination_reports_eexist_without_success() {
    reset(7, &[-9], 1);
    assert_eq!(unsafe { rename(c"old".as_ptr(), c"new".as_ptr()) }, -1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), 17);
    assert_eq!(DISCARDS.load(Ordering::Relaxed), 1);
}

#[test]
fn same_missing_path_reports_enoent_without_starting_rename() {
    reset(7, &[], 0);
    assert_eq!(unsafe { rename(c"missing".as_ptr(), c"missing".as_ptr()) }, -1);
    assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), 2);
    assert!(PATHS.lock().unwrap().is_empty());
}
'''


def main():
    production = extract(SOURCE.read_text(), "rename")
    with tempfile.TemporaryDirectory(prefix="posix-rename-") as directory:
        rust = Path(directory) / "test.rs"
        binary = Path(directory) / "test"
        rust.write_text(HARNESS + production)
        subprocess.run(["rustc", "--edition=2024", "--test", "-Awarnings", str(rust), "-o", str(binary)], check=True)
        subprocess.run([str(binary), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
