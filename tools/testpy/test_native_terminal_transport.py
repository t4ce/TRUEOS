#!/usr/bin/env python3
"""Compile the real terminal ABI adapters against instrumented native services."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(source, name):
    start = re.search(rf'^pub (?:unsafe )?extern "C" fn {name}\(', source, re.M).start()
    end = source.index('\n}', start) + 2
    return source[start:end]


def main():
    source = (ROOT / "src/r/io/fs_cabi.rs").read_text()
    functions = "\n".join(item(source, name) for name in (
        "trueos_cabi_env_current_dir", "trueos_cabi_shell_attached_read_v1",
        "trueos_cabi_shell_attached_observe_v1", "trueos_cabi_shell_attached_wait_after_v1",
        "trueos_cabi_shell_attached_wake_v1"))
    copy_start = source.index("unsafe fn copy_text(")
    functions += "\n" + source[copy_start:source.index('\n}', copy_start) + 2]
    harness = r'''#![allow(dead_code)]
extern crate alloc;
use std::sync::atomic::{AtomicUsize, Ordering::SeqCst};
static CONTEXT: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicUsize = AtomicUsize::new(7);
static TRANSPORT_ERROR: AtomicUsize = AtomicUsize::new(0);
mod hv {
    pub fn current_hull_guest_context_vm_id() -> Option<u8> {
        (super::CONTEXT.load(super::SeqCst) == 2).then_some(3)
    }
    pub fn current_guest_execution_context_vm_id() -> Option<u8> {
        (super::CONTEXT.load(super::SeqCst) != 0).then_some(3)
    }
    pub fn blueprint_console_read(vm: u8, out: &mut [u8]) -> usize {
        assert_eq!(vm,3); super::CALLS.fetch_add(1,super::SeqCst);
        out.fill(b'n'); out.len()
    }
}
mod trueos_vm { pub mod vmcall {
    pub const PAYLOAD_CAP: usize = 16;
    pub const OP_BP_SHELL_ATTACHED_READ: u32 = 203;
    pub const STATUS_OK: u32 = 0;
    pub fn call_with_payload(op:u32,cap:u64,arg:u64,input:&[u8],out:&mut [u8])->(u32,u64) {
        assert_eq!(op,OP_BP_SHELL_ATTACHED_READ); assert_eq!(cap as usize,out.len());
        assert_eq!(arg,0); assert!(input.is_empty());
        crate::CALLS.fetch_add(1,crate::SeqCst); out.fill(b'g');
        (crate::TRANSPORT_ERROR.load(crate::SeqCst) as u32,out.len() as u64)
    }
}}
mod wait { pub const BLUEPRINT_IO_WAIT_KEY:u64=123; }
mod r {
 pub mod platform {
    pub fn trueos_tokio_platform_wait_observe(key:u64)->u32 {
        assert_eq!(key,crate::wait::BLUEPRINT_IO_WAIT_KEY);
        crate::GENERATION.load(crate::SeqCst) as u32
    }
    pub fn trueos_tokio_platform_wait_after(key:u64,observed:u32,timeout:u64)->bool {
        assert_eq!(key,crate::wait::BLUEPRINT_IO_WAIT_KEY); assert_eq!(timeout,42);
        crate::GENERATION.load(crate::SeqCst) as u32 != observed
    }
    pub fn trueos_tokio_platform_wake_all(key:u64)->usize {
        assert_eq!(key,crate::wait::BLUEPRINT_IO_WAIT_KEY);
        crate::GENERATION.fetch_add(1,crate::SeqCst); 0
    }
 }
 pub mod io {
  pub mod env { pub fn current_app_fs_root()->Option<String> { Some("/disk/apps/".into()) } }
  pub mod fs_cabi {
   use crate::trueos_vm;
   const FS_ERR_NO_SPACE:i32=-9;
   unsafe fn trueos_cabi_env_var(key:*const u8,len:usize,out:*mut u8,cap:usize)->isize {
    assert_eq!(unsafe { std::slice::from_raw_parts(key,len) },b"TRUEOS_APP_FS_ROOT");
    assert!(cap>=7); unsafe {std::ptr::copy_nonoverlapping(b"/guest/".as_ptr(),out,7)}; 7
   }
   // @FUNCTIONS@
  }
 }
}
use r::io::fs_cabi::*;
#[test] fn batch_reads_preserve_caps_and_errors() {
 let mut out=[0;64]; CALLS.store(0,SeqCst); CONTEXT.store(1,SeqCst);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(out.as_mut_ptr(),out.len())},64);
 assert_eq!(out,[b'n';64]); assert_eq!(CALLS.load(SeqCst),1);
 CONTEXT.store(2,SeqCst);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(out.as_mut_ptr(),out.len())},16);
 assert_eq!(&out[..16],&[b'g';16]); assert_eq!(CALLS.load(SeqCst),2);
 TRANSPORT_ERROR.store(1,SeqCst);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(out.as_mut_ptr(),out.len())},-3);
 TRANSPORT_ERROR.store(0,SeqCst);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(std::ptr::null_mut(),0)},0);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(std::ptr::null_mut(),1)},-1);
 CONTEXT.store(0,SeqCst);
 assert_eq!(unsafe {trueos_cabi_shell_attached_read_v1(out.as_mut_ptr(),out.len())},-2);
}
#[test] fn observe_and_wait_forward_the_same_native_generation() {
 CONTEXT.store(1,SeqCst); GENERATION.store(7,SeqCst);
 let observed=trueos_cabi_shell_attached_observe_v1();
 assert_eq!(trueos_cabi_shell_attached_wait_after_v1(observed,42),0);
 assert_eq!(trueos_cabi_shell_attached_wake_v1(),1);
 assert_eq!(trueos_cabi_shell_attached_wait_after_v1(observed,42),1);
 CONTEXT.store(0,SeqCst);
 assert_eq!(trueos_cabi_shell_attached_wait_after_v1(observed,42),-2);
 assert_eq!(trueos_cabi_shell_attached_wake_v1(),-2);
}
#[test] fn current_directory_queries_native_scope_without_c_terminator() {
 CONTEXT.store(1,SeqCst);
 assert_eq!(unsafe {trueos_cabi_env_current_dir(std::ptr::null_mut(),0)},10);
 let mut out=[0x55;12];
 assert_eq!(unsafe {trueos_cabi_env_current_dir(out.as_mut_ptr(),9)},-9);
 assert_eq!(out,[0x55;12]);
 assert_eq!(unsafe {trueos_cabi_env_current_dir(out.as_mut_ptr(),12)},10);
 assert_eq!(&out[..10],b"/disk/apps"); assert_eq!(&out[10..],&[0x55;2]);
 CONTEXT.store(2,SeqCst);
 assert_eq!(unsafe {trueos_cabi_env_current_dir(out.as_mut_ptr(),12)},6);
 assert_eq!(&out[..6],b"/guest");
}
'''.replace("// @FUNCTIONS@", functions)
    with tempfile.TemporaryDirectory(prefix="trueos-native-terminal-abi-") as directory:
        directory = Path(directory)
        source = directory / "test.rs"
        binary = directory / "test"
        source.write_text(harness)
        subprocess.run(["rustc", "--edition=2024", "--test", source, "-o", binary], check=True)
        subprocess.run([binary, "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
