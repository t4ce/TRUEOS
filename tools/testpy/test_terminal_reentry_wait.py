#!/usr/bin/env python3
"""Check the production native terminal wait against typed reentry races."""
from pathlib import Path
import subprocess
import tempfile

from test_posix_renameat import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    production = function(ROOT / 'src/r/io/fs_cabi.rs', 'wait_attached_console_readable')
    harness = r'''
#![allow(dead_code)]
use std::cell::{Cell,RefCell};
std::thread_local! {
    static NATIVE:Cell<Option<u8>>=const {Cell::new(Some(7))};
    static HULL:Cell<bool>=const {Cell::new(false)};
    static BYTES:Cell<usize>=const {Cell::new(0)};
    static PENDING:Cell<bool>=const {Cell::new(false)};
    static RACE:Cell<bool>=const {Cell::new(false)};
    static GENERATION:Cell<u64>=const {Cell::new(10)};
    static WAITS:RefCell<Vec<(u64,u64)>>=const {RefCell::new(Vec::new())};
    static OBSERVATIONS:Cell<usize>=const {Cell::new(0)};
}
fn trueos_cabi_shell_attached_readable_len()->usize {BYTES.with(Cell::get)}
mod hv {
    pub fn current_hull_guest_context_vm_id()->Option<u8> {crate::HULL.with(|v|v.get().then_some(7))}
    pub fn current_guest_execution_context_vm_id()->Option<u8> {crate::NATIVE.with(std::cell::Cell::get)}
    pub fn blueprint_terminal_reentry_requested(vm:u8)->bool {
        assert_eq!(vm,7);
        if crate::RACE.with(std::cell::Cell::get) {crate::GENERATION.with(|v|v.set(v.get()+1));}
        crate::PENDING.with(std::cell::Cell::get)
    }
}
mod r {pub mod platform {
    pub fn trueos_tokio_platform_wait_observe(_:u64)->u64 {
        crate::OBSERVATIONS.with(|v|v.set(v.get()+1));crate::GENERATION.with(std::cell::Cell::get)
    }
    pub fn trueos_tokio_platform_wait_after(_:u64,observed:u64,timeout:u64)->bool {
        crate::WAITS.with(|v|v.borrow_mut().push((observed,timeout)));
        crate::GENERATION.with(std::cell::Cell::get)!=observed
    }
}}
mod wait {
    pub const BLUEPRINT_IO_WAIT_KEY:u64=42;
    pub fn spin_until_timeout(_:u64,condition:impl Fn()->bool)->bool {condition()}
}
mod trueos_vm {pub mod vmcall {
    pub const STATUS_OK:u32=0;pub const OP_BP_SHELL_ATTACHED_WAIT_READABLE:u32=9;
    pub fn call(op:u32,timeout:u64,_:u64)->(u32,u64) {assert_eq!(op,9);assert_eq!(timeout,50);(0,1)}
}}
fn setup() {
    NATIVE.with(|v|v.set(Some(7)));HULL.with(|v|v.set(false));BYTES.with(|v|v.set(0));
    PENDING.with(|v|v.set(false));RACE.with(|v|v.set(false));GENERATION.with(|v|v.set(10));
    WAITS.with(|v|v.borrow_mut().clear());OBSERVATIONS.with(|v|v.set(0));
}
#[test] fn request_before_wait_is_ready_without_a_fake_stdin_byte() {
    setup();PENDING.with(|v|v.set(true));assert!(wait_attached_console_readable(50));
    assert_eq!(BYTES.with(Cell::get),0);WAITS.with(|v|assert!(v.borrow().is_empty()));
}
#[test] fn request_racing_the_probe_is_not_lost() {
    setup();RACE.with(|v|v.set(true));assert!(wait_attached_console_readable(50));
    WAITS.with(|v|assert_eq!(&*v.borrow(),&[(10,50)]));assert_eq!(BYTES.with(Cell::get),0);
}
#[test] fn byte_readiness_and_timeout_keep_their_own_results() {
    setup();BYTES.with(|v|v.set(1));assert!(wait_attached_console_readable(50));
    WAITS.with(|v|assert!(v.borrow().is_empty()));
    BYTES.with(|v|v.set(0));assert!(!wait_attached_console_readable(50));
    WAITS.with(|v|assert_eq!(&*v.borrow(),&[(10,50)]));
}
#[test] fn hull_carrier_still_uses_vmcall_and_kernel_fallback_is_bounded() {
    setup();HULL.with(|v|v.set(true));assert!(wait_attached_console_readable(50));
    assert_eq!(OBSERVATIONS.with(Cell::get),0);
    HULL.with(|v|v.set(false));NATIVE.with(|v|v.set(None));
    assert!(!wait_attached_console_readable(50));assert_eq!(OBSERVATIONS.with(Cell::get),0);
}
'''
    with tempfile.TemporaryDirectory(prefix='terminal-reentry-wait-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(harness + production)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'),
                        '-o', str(path / 'test')], check=True)
        subprocess.run([str(path / 'test'), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
