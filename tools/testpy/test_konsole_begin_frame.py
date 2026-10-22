#!/usr/bin/env python3
"""Keep the live Konsole sizing/handoff contract after retiring row packets."""
from pathlib import Path
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/audit'))
from removal_candidates import function_span


def main():
    source = (ROOT / 'src/r/io/fs_cabi.rs').read_text()
    production = '\n'.join(source[a:b] for a, b in
                           (function_span(source, name) for name in
                            ['konsole_begin_frame_size', 'trueos_cabi_konsole_begin_frame']))
    harness = r'''
#![allow(dead_code)]
use std::cell::RefCell;
#[derive(Default)]
struct Fixture { guest: bool, vm: Option<u8>, size: (u32,u32), claimed: bool,
                 calls: Vec<(u64,u64,bool)>, status: u32 }
thread_local! { static STATE: RefCell<Fixture> = RefCell::new(Fixture::default()); }
const KONSOLE_FRAME_FLAG_TERMINAL_HANDOFF: u32 = 1 << 31;
fn claim_attached_console_for_terminal_io() { STATE.with(|s| s.borrow_mut().claimed=true); }
mod hv {
    pub fn current_hull_guest_context_vm_id()->Option<u8> {
        super::STATE.with(|s| s.borrow().guest.then_some(1))
    }
    pub fn current_guest_execution_context_vm_id()->Option<u8> {
        super::STATE.with(|s| s.borrow().vm)
    }
    pub fn blueprint_console_konsole_begin_frame(_:u8,cols:usize,rows:usize,handoff:bool)->(u32,u32) {
        super::STATE.with(|s| { let mut s=s.borrow_mut();
            let claimed=s.claimed; assert!(!handoff || claimed);
            s.calls.push((cols as u64,rows as u64,claimed)); s.size })
    }
}
mod trueos_vm { pub mod vmcall {
    pub const OP_BP_SHELL_KONSOLE_BEGIN_FRAME:u32=0xA1;
    pub const STATUS_OK:u32=0;
    pub fn call(op:u32,cols:u64,rows:u64)->(u32,u64) {
        assert_eq!(op,OP_BP_SHELL_KONSOLE_BEGIN_FRAME);
        crate::STATE.with(|s| { let mut s=s.borrow_mut(); let claimed=s.claimed;
            s.calls.push((cols,rows,claimed)); (s.status,((s.size.0 as u64)<<32)|s.size.1 as u64) })
    }
}}
PRODUCTION
#[test]
fn invalid_dimensions_and_missing_or_unavailable_console_fail() {
    for (cols,rows) in [(0,25),(80,0),(0,0)] {
        assert_eq!(trueos_cabi_konsole_begin_frame(cols,rows,0),-1);
    }
    assert!(STATE.with(|s| s.borrow().calls.is_empty()));
    assert_eq!(trueos_cabi_konsole_begin_frame(80,25,0),-1);
    STATE.with(|s| s.borrow_mut().vm=Some(1));
    assert_eq!(trueos_cabi_konsole_begin_frame(80,25,0),-1);
}
#[test]
fn host_handoff_claims_before_resizing_and_plain_resize_does_not_claim() {
    STATE.with(|s| *s.borrow_mut()=Fixture {vm:Some(1),size:(80,25),..Default::default()});
    assert_eq!(trueos_cabi_konsole_begin_frame(80,25,3),0);
    assert_eq!(trueos_cabi_konsole_begin_frame(80,25,KONSOLE_FRAME_FLAG_TERMINAL_HANDOFF|3),0);
    STATE.with(|s| assert_eq!(s.borrow().calls,vec![(80,25,false),(80,25,true)]));
}
#[test]
fn guest_handoff_flag_dimensions_and_host_failure_are_preserved() {
    STATE.with(|s| *s.borrow_mut()=Fixture {guest:true,size:(100,40),..Default::default()});
    assert_eq!(trueos_cabi_konsole_begin_frame(100,40,KONSOLE_FRAME_FLAG_TERMINAL_HANDOFF),0);
    STATE.with(|s| { let mut s=s.borrow_mut(); assert_eq!(s.calls,vec![(100,40|(1<<63),true)]);s.status=1; });
    assert_eq!(trueos_cabi_konsole_begin_frame(100,40,0),-1);
}
'''.replace('PRODUCTION', production)
    with tempfile.TemporaryDirectory(prefix='trueos-konsole-begin-') as directory:
        source_path = Path(directory) / 'test.rs'
        binary = Path(directory) / 'test'
        source_path.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(source_path), '-o', str(binary)], check=True)
        subprocess.run([str(binary)], check=True)


if __name__ == '__main__':
    main()
