#!/usr/bin/env python3
"""Exercise the production Blueprint current_exe/readlink shim with host buffers."""
from pathlib import Path
import subprocess
import tempfile
from test_posix_renameat import function
ROOT = Path(__file__).resolve().parents[2]

def main():
    harness = r'''
#![allow(dead_code, unsafe_op_in_unsafe_fn)]
extern crate alloc;
use alloc::string::String;
use core::{ffi::c_char, sync::atomic::{AtomicI32, Ordering}};
static TRUEOS_ERRNO: AtomicI32 = AtomicI32::new(0);
const TRUEOS_EINVAL: i32 = 22;
const TRUEOS_ENOENT: i32 = 2;
const TRUEOS_ENOSYS: i32 = 38;
std::thread_local! {static ROOT: std::cell::RefCell<Option<String>> = Default::default();}
mod r {pub mod io {pub mod env {
    pub fn current_app_fs_root() -> Option<String> {crate::ROOT.with(|r|r.borrow().clone())}
    pub fn var(_: &str) -> Option<String> {Some("remote/voxy.bp".into())}
}}}
unsafe fn abi_cstr_to_string(p: *const c_char, _: usize) -> Option<String> {
    std::ffi::CStr::from_ptr(p).to_str().ok().map(String::from)
}
unsafe fn abi_write_bytes(p: *mut u8, n: usize) -> Option<&'static mut [u8]> {
    Some(core::slice::from_raw_parts_mut(p,n))
}
#[test]
fn executable_identity_uses_instance_root() {
    assert_eq!(app_executable_path("/apps/voxy/child--uuid/", Some("remote/voxy.bp")), "/apps/voxy/child--uuid/voxy.bp");
    assert_eq!(app_executable_path("apps/voxy", None), "/apps/voxy/blueprint.bp");
    assert_eq!(app_executable_path("apps/voxy", Some("..")), "/apps/voxy/blueprint.bp");
}
#[test]
fn readlink_obeys_posix_buffer_semantics_and_errors() {
    ROOT.with(|r| *r.borrow_mut() = Some("apps/voxy".into()));
    let mut output = [0x55u8; 64];
    let path = c"/proc/self/exe";
    unsafe {
        let n = readlink(path.as_ptr(), output.as_mut_ptr().cast(), output.len());
        assert_eq!(n, 18);
        assert_eq!(&output[..n as usize], b"/apps/voxy/voxy.bp");
        assert_eq!(output[n as usize], 0x55);
        assert_eq!(readlink(path.as_ptr(), output.as_mut_ptr().cast(), 5), 5);
        assert_eq!(&output[..5], b"/apps");
        assert_eq!(readlink(path.as_ptr(), output.as_mut_ptr().cast(), 0), -1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), TRUEOS_EINVAL);
        assert_eq!(readlink(c"/some/link".as_ptr(), output.as_mut_ptr().cast(), 64), -1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), TRUEOS_ENOSYS);
        ROOT.with(|r| *r.borrow_mut() = None);
        assert_eq!(readlink(path.as_ptr(), output.as_mut_ptr().cast(), 64), -1);
        assert_eq!(TRUEOS_ERRNO.load(Ordering::Relaxed), TRUEOS_ENOENT);
    }
}
'''
    shim = ROOT / 'src/std_abi_shim.rs'
    harness += '\n'.join(function(shim, n) for n in ['app_executable_path', 'readlink'])
    with tempfile.TemporaryDirectory(prefix='trueos-current-exe-') as d:
        src, binary = Path(d) / 'test.rs', Path(d) / 'test'
        src.write_text(harness)
        subprocess.run(['rustc','--edition=2024','--test','--target','x86_64-unknown-linux-gnu',str(src),'-o',str(binary)],check=True,cwd=ROOT)
        subprocess.run([str(binary)],check=True)
if __name__ == '__main__': main()
