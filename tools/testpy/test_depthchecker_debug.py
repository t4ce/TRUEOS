#!/usr/bin/env python3
"""Exercise the production diagnostic handoff against a mock VMCS on the host."""

from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
production = (ROOT / "src/hv/depthchecker_debug.rs").read_text()
vmx = (ROOT / "src/hv/vmx.rs").read_text()
registers = re.search(r"pub struct GuestRegisters \{.*?^}", vmx, re.M | re.S).group()
constants = "\n".join(
    re.search(r"^(?:pub|pub\(crate\)) const " + name + r":.*?;", vmx, re.M).group()
    for name in sorted(set(re.findall(r"vmx::(VMCS_[A-Z0-9_]+|VMX_EXTENDED_STATE_BYTES)", production)))
)
source = r'''#![allow(dead_code, non_upper_case_globals)]
mod percpu { pub fn current_slot() -> usize { 2 } }
mod limine { pub fn hhdm_offset() -> Option<u64> { Some(0xffff800000000000) } }
mod hv {
    pub static ARCHIVE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
    pub fn app_vm_archive(_:u8)->Option<String> { ARCHIVE.lock().unwrap().clone() }
    pub mod memory {
        pub const GUEST_STACK_VA_BASE:u64=0x400000;
        pub static STACK:[u8;4096]=[0;4096];
        pub fn guest_stack_slice_for_vm(_:u8)->Option<&'static [u8]> { Some(&STACK) }
    }
    pub mod vmx {
        use std::{collections::BTreeMap,sync::Mutex};
        @CONSTANTS@
        #[derive(Copy,Clone,Default)] #[repr(C)] @REGISTERS@
        pub struct LaunchResult { pub guest_cr2:u64 }
        pub static VMCS:Mutex<BTreeMap<u64,u64>>=Mutex::new(BTreeMap::new());
        pub static WRITES:Mutex<Vec<(u64,u64)>>=Mutex::new(Vec::new());
        pub fn vmread(field:u64)->Option<u64> { VMCS.lock().unwrap().get(&field).copied() }
        pub fn vmwrite(field:u64,value:u64)->Result<(),&'static str> {
            VMCS.lock().unwrap().insert(field,value); WRITES.lock().unwrap().push((field,value)); Ok(())
        }
        pub fn guest_registers()->GuestRegisters {
            GuestRegisters {rax:1,rbx:2,rcx:3,rdx:4,rsi:5,rdi:6,rbp:7,r8:8,r9:9,r10:10,r11:11,r12:12,r13:13,r14:14,r15:15}
        }
        pub fn guest_extended_state_snapshot(_:u8)->Result<(u64,[u8;VMX_EXTENDED_STATE_BYTES]),&'static str> { Ok((7,[0xa5;VMX_EXTENDED_STATE_BYTES])) }
    }
    pub mod depthchecker_debug {
        @PRODUCTION@
        #[cfg(test)] mod tests {
            use super::*;
            use crate::hv::{ARCHIVE,vmx};
            static TEST_LOCK:std::sync::Mutex<()>=std::sync::Mutex::new(());
            fn prepare() {
                DEPTHCHECKER_DEBUG_ENABLE.store(0,Ordering::SeqCst);
                DEPTHCHECKER_DEBUG_ADDRESS_COUNT.store(1,Ordering::SeqCst);
                DEPTHCHECKER_DEBUG_ADDRESSES[0].store(0x123400,Ordering::SeqCst);
                DEPTHCHECKER_DEBUG_HIT_COUNT.store(0,Ordering::SeqCst);
                *ARCHIVE.lock().unwrap()=Some("depthchecker_factchecker.bp".into());
                let mut state=vmx::VMCS.lock().unwrap(); state.clear();
                for (field,value) in [
                    (vmx::VMCS_VMEXIT_INTERRUPTION_INFO,0x80000603),
                    (vmx::VMCS_GUEST_RIP,0x123400),(vmx::VMCS_GUEST_RSP,0x400010),
                    (vmx::VMCS_GUEST_RFLAGS,0x202),(vmx::VMCS_GUEST_CR3,0x12000),
                    (vmx::VMCS_CTRL_EPT_POINTER,0x1301e),(vmx::VMCS_GUEST_FS_BASE,0x15000),
                    (vmx::VMCS_GUEST_GS_BASE,0x16000),(vmx::VMCS_GUEST_CR0,0x80010033),
                    (vmx::VMCS_GUEST_CR4,0x2620),(vmx::VMCS_GUEST_IA32_EFER,0xd00),
                    (vmx::VMCS_GUEST_CS_SELECTOR,8),(vmx::VMCS_GUEST_SS_SELECTOR,16),
                ] {state.insert(field,value);}
                vmx::WRITES.lock().unwrap().clear();
            }
            #[test] fn exact_gate_and_resume_preserve_the_intercepted_instruction() {
                let _guard=TEST_LOCK.lock().unwrap(); prepare();
                let exit=vmx::LaunchResult {guest_cr2:0xfeed};
                assert!(!try_handoff(7,&exit).unwrap());
                assert!(vmx::WRITES.lock().unwrap().is_empty());
                DEPTHCHECKER_DEBUG_ENABLE.store(1,Ordering::SeqCst);
                assert!(try_handoff(7,&exit).unwrap());
                assert_eq!(*vmx::WRITES.lock().unwrap(),vec![(vmx::VMCS_GUEST_RIP,0x123400)]);
                assert_eq!(vmx::vmread(vmx::VMCS_GUEST_RIP),Some(0x123400));
                assert_eq!(DEPTHCHECKER_DEBUG_HIT_COUNT.load(Ordering::SeqCst),1);
                for archive in ["codex.bp","depthchecker_factchecker","DEPTHCHECKER_FACTCHECKER.bp","other/depthchecker_factchecker.bp"] {
                    *ARCHIVE.lock().unwrap()=Some(archive.into());
                    assert!(!try_handoff(7,&exit).unwrap());
                }
                assert_eq!(vmx::WRITES.lock().unwrap().len(),1);
                assert_eq!(DEPTHCHECKER_DEBUG_HIT_COUNT.load(Ordering::SeqCst),1);
            }
            #[test] fn unexpected_exceptions_addresses_and_invalid_whitelists_are_not_consumed() {
                let _guard=TEST_LOCK.lock().unwrap(); prepare();
                for raw in [0x00000603,0x8000060e,0x80000003,0x80000403,0x80000303] {
                    assert_eq!(armed_resume_rip(1,"depthchecker_factchecker.bp",raw,0x123400),None);
                }
                assert_eq!(armed_resume_rip(1,"depthchecker_factchecker.bp",0x80000603,0x123401),None);
                for count in [0,65,u64::MAX] {
                    DEPTHCHECKER_DEBUG_ADDRESS_COUNT.store(count,Ordering::SeqCst);
                    assert_eq!(armed_resume_rip(1,"depthchecker_factchecker.bp",0x80000603,0x123400),None);
                }
                DEPTHCHECKER_DEBUG_ADDRESS_COUNT.store(64,Ordering::SeqCst);
                DEPTHCHECKER_DEBUG_ADDRESSES[63].store(0x987600,Ordering::SeqCst);
                assert_eq!(armed_resume_rip(1,"depthchecker_factchecker.bp",0x80000603,0x987600),Some(0x987600));
                assert_eq!(armed_resume_rip(2,"depthchecker_factchecker.bp",0x80000603,0x987600),None);
            }
            #[test] fn snapshot_is_actual_guest_state_with_stable_schema_and_stack_alias() {
                let _guard=TEST_LOCK.lock().unwrap(); prepare();
                let s=capture_snapshot(7,&vmx::LaunchResult {guest_cr2:0xfeed},0x123400).unwrap();
                assert_eq!(s.magic.to_le_bytes(),*b"DCFCSNAP");
                assert_eq!(s.schema_version,1); assert_eq!(s.snapshot_bytes,1152);
                assert_eq!((s.vm_id,s.cpu_slot,s.guest_rip,s.guest_rsp),(7,2,0x123400,0x400010));
                assert_eq!(s.guest_gpr,[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15]);
                assert_eq!((s.guest_xsave_mask,s.guest_xsave_size),(7,832)); assert_eq!(s.guest_xsave,[0xa5;832]);
                assert_eq!((s.guest_cr3,s.eptp,s.hhdm_offset),(0x12000,0x1301e,0xffff800000000000));
                assert_eq!((s.guest_cr2,s.guest_fs_base,s.guest_gs_base),(0xfeed,0x15000,0x16000));
                assert_eq!((s.guest_stack_base,s.stack_bytes),(0x400000,4096));
                assert_eq!(s.stack_host_base,crate::hv::memory::STACK.as_ptr() as u64);
            }
            #[test] fn raw_stack_alias_is_bounded_and_checks_address_overflow() {
                let host=0xffff800220000000;
                assert_eq!(bounded_stack_host_address(0x400000,host,0x2000000,0x400010,16),Some(host+16));
                for (address,bytes) in [(0x3fffff,1),(0x2400000,1),(0x400000,65537),(0x400000,0),(u64::MAX,8)] {
                    assert_eq!(bounded_stack_host_address(0x400000,host,0x2000000,address,bytes),None);
                }
                assert_eq!(bounded_stack_host_address(0x400000,u64::MAX-4,32,0x400008,8),None);
                assert_eq!(bounded_stack_host_address(0x400000,0,32,0x400000,8),None);
            }
        }
    }
}
'''
# Outer module text is loaded verbatim; only inner documentation comments need
# ordinary comment syntax after insertion into the generated host module.
source = source.replace("@CONSTANTS@", constants).replace("@REGISTERS@", registers)
source = source.replace("@PRODUCTION@", production.replace("//!", "//"))
with tempfile.TemporaryDirectory(prefix="depthchecker-debug-host-") as folder:
    folder = Path(folder)
    fixture = folder / "tests.rs"
    binary = folder / "tests"
    fixture.write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(fixture), "-o", str(binary)], check=True)
    subprocess.run([str(binary)], check=True)
