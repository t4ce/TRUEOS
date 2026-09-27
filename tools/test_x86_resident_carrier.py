#!/usr/bin/env python3
"""Execute production resident-entry/retirement control flow with VMX stubs."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

ROOT = Path(__file__).resolve().parents[1]
path = 'src/hv/mod.rs'
entry = item(path, 'run_transient_protected32')
constants = sorted(set(re.findall(r'\b(?:VMCS_[A-Z0-9_]+|PIN_BASED_EXTERNAL_INTERRUPT_EXITING|EXIT_CTL_ACKNOWLEDGE_INTERRUPT_ON_EXIT|RFLAGS_[A-Z0-9_]+)\b', entry)))
code = r'''
#![allow(dead_code, unused_unsafe)]
use std::sync::{Mutex, atomic::{AtomicBool, AtomicU16, Ordering}};
use std::collections::BTreeMap;
const TRUEOS_VM_CPU_SLOT_LIMIT: usize = 4;
static CALLS: Mutex<Vec<&str>> = Mutex::new(Vec::new());
static FIELDS: Mutex<BTreeMap<u64,u64>> = Mutex::new(BTreeMap::new());
static FAIL_CLEAR: AtomicBool = AtomicBool::new(false);
static FAIL_ENTRY: AtomicBool = AtomicBool::new(false);
static PDPT: [u64;4] = [11,22,33,44];
static mut PAGE: [u64;512] = [0;512];
const VMX_PAGE_SIZE: usize = 4096;
static DEDICATED_X86_OWNER: [AtomicU16;4] = [const { AtomicU16::new(0) };4];
static RESIDENT_X86_VMCS_READY: [AtomicBool;4] = [const { AtomicBool::new(false) };4];
static RESIDENT_X86_VMCS_LOADED: [AtomicBool;4] = [const { AtomicBool::new(false) };4];
fn record(s: &'static str) { CALLS.lock().unwrap().push(s); }
mod percpu { pub fn current_slot() -> usize {3} }
mod workers { pub fn last_ap_execution_slot() -> Option<u32> {Some(3)} }
mod phys { pub fn phys_to_virt(_: usize) -> usize {super::PDPT.as_ptr() as usize} }
mod allcaps { pub mod hv { pub const VMX_WC3_TRANSIENT_PREEMPTION_QUANTUM_MS: u64 = 1; } }
mod v { pub mod bp_abi {
    #[derive(Default,Copy,Clone)] pub struct TrueosX86DebugRegistersV1 {pub dr7:u32}
} }
#[derive(Default,Copy,Clone)] struct LaunchResult { entered:u32, guest_rip:u64 }
mod hv {
    pub mod lane { pub fn quarantine_x86_lane(_:usize) {crate::record("quarantine");} }
    pub mod vmx {
        pub const IA32_VMX_BASIC:u32 = 0;
        pub struct VmxExtendedState;
        #[derive(Default,Copy,Clone)] pub struct GuestRegisters;
        pub fn vmclear(_:u64)->bool {crate::record("clear");!crate::FAIL_CLEAR.load(crate::Ordering::Relaxed)}
        pub fn vmptrld(_:u64)->bool {crate::record("load");true}
        pub fn set_guest_registers(_:GuestRegisters) {}
        pub fn guest_registers()->GuestRegisters {GuestRegisters}
        pub fn vmlaunch_once_wrapper_with_extended_state(out:&mut crate::LaunchResult,_:&mut VmxExtendedState) {
            crate::record("launch");out.entered=(!crate::FAIL_ENTRY.load(crate::Ordering::Relaxed)) as u32;
        }
        pub fn vmresume_once_wrapper_with_extended_state(out:&mut crate::LaunchResult,_:&mut VmxExtendedState) {
            crate::record("resume");out.entered=(!crate::FAIL_ENTRY.load(crate::Ordering::Relaxed)) as u32;
        }
    }
}
struct Msr;
impl Msr {fn new(_:u32)->Self {Self} fn read(&self)->u64 {1}}
struct LineageRecord;
impl LineageRecord {fn new()->Self {Self}}
enum VmBootMode {Wc3Probe}
struct Protected32GuestInput {cr3:u64,eip:u32,esp:u32,eflags:u32,fs_base:u32,debug_registers:v::bp_abi::TrueosX86DebugRegistersV1}
fn current_vmx_root_active()->Result<bool,&'static str>{Ok(true)}
fn active_eptp_for_vm(_:u8)->Result<u64,&'static str>{Ok(0x2000)}
fn current_vmx_slot()->Result<usize,&'static str>{Ok(3)}
fn current_vmcs_page()->Result<*mut u8,&'static str>{Ok(std::ptr::addr_of_mut!(PAGE).cast())}
fn kernel_va_to_pa(_:u64)->Option<u64>{Some(0x9000)}
fn vmwrite(k:u64,v:u64)->Result<(),&'static str>{FIELDS.lock().unwrap().insert(k,v);Ok(())}
fn vmread(k:u64)->Option<u64>{Some(*FIELDS.lock().unwrap().get(&k).unwrap_or(&0))}
fn setup_vmcs_host_and_controls(_:u8,_:Option<u16>,_:u64,_:LineageRecord,_:VmBootMode,_:Option<Protected32GuestInput>)->Result<bool,&'static str>{record("setup");Ok(true)}
fn vmx_preemption_timer_ticks(_:u64)->(u32,u32){(10,0)}
type CarrierDebugRegisters = v::bp_abi::TrueosX86DebugRegistersV1;
impl CarrierDebugRegisters {fn capture()->Self{Self::default()} fn install(_:Self){}}
fn transient_exception_capture(_:&LaunchResult)->(u64,u64,u64){(0,0,0)}
fn merge_transient_debug_exception(_:&mut CarrierDebugRegisters,_:&LaunchResult){}
'''
code += '\n'.join(f'const {name}:u64={i+1};' for i, name in enumerate(constants))
code += '\n' + item(path, 'TransientProtected32Exit')
code += '\n' + item(path, 'DedicatedX86Scope')
source = (ROOT / path).read_text()
for marker in ['impl DedicatedX86Scope {', 'impl Drop for DedicatedX86Scope {']:
    start = source.index(marker)
    end = source.index('\n}\n', start) + 2
    code += '\n' + source[start:end]
code += '\n' + entry
code += r'''
fn reset() {
    CALLS.lock().unwrap().clear(); FIELDS.lock().unwrap().clear();
    FAIL_CLEAR.store(false,Ordering::Relaxed); FAIL_ENTRY.store(false,Ordering::Relaxed);
}
fn enter(cr3:u64,eip:u32) {
    run_transient_protected32(2,cr3,eip,0x4000,0x202,0x5000,
        CarrierDebugRegisters::default(),hv::vmx::GuestRegisters,&mut hv::vmx::VmxExtendedState).unwrap();
}
#[test] fn dedicated_scope_launches_once_resumes_and_clears_before_release() {
    reset();
    let scope=DedicatedX86Scope::enter(2);
    enter(0x1000,0x1234); enter(0x3000,0x5678);
    assert_eq!(*CALLS.lock().unwrap(),["clear","load","setup","launch","resume"]);
    assert_eq!(vmread(VMCS_GUEST_CR3),Some(0x3000));
    assert_eq!(vmread(VMCS_GUEST_RIP),Some(0x5678));
    assert_eq!(vmread(VMCS_GUEST_PDPTE3),Some(44));
    drop(scope);
    assert_eq!(CALLS.lock().unwrap().last(),Some(&"clear"));
    assert_eq!(DEDICATED_X86_OWNER[3].load(Ordering::Relaxed),0);
    assert!(!RESIDENT_X86_VMCS_READY[3].load(Ordering::Relaxed));
}
#[test] fn ordinary_lane_still_uses_fresh_vmcs() {
    reset(); enter(0x1000,1); enter(0x1000,2);
    assert_eq!(*CALLS.lock().unwrap(),["clear","load","setup","launch","clear","clear","load","setup","launch","clear"]);
}
#[test] fn failed_entry_cannot_be_resumed_as_launched() {
    reset(); let scope=DedicatedX86Scope::enter(2);
    FAIL_ENTRY.store(true,Ordering::Relaxed);enter(0x1000,1);
    assert!(!RESIDENT_X86_VMCS_READY[3].load(Ordering::Relaxed));
    FAIL_ENTRY.store(false,Ordering::Relaxed);enter(0x1000,1);
    assert!(!CALLS.lock().unwrap().contains(&"resume"));drop(scope);
}
#[test] fn failed_retirement_quarantines_lane() {
    reset();let scope=DedicatedX86Scope::enter(2);enter(0x1000,1);
    FAIL_CLEAR.store(true,Ordering::Relaxed);drop(scope);
    assert_eq!(CALLS.lock().unwrap().last(),Some(&"quarantine"));
}
#[test] fn foreign_owner_cannot_overwrite_resident_vmcs() {
    reset();let scope=DedicatedX86Scope::enter(2);enter(0x1000,1);
    let calls=CALLS.lock().unwrap().len();
    assert!(run_transient_protected32(1,0x1000,1,0x4000,0x202,0x5000,
        CarrierDebugRegisters::default(),hv::vmx::GuestRegisters,&mut hv::vmx::VmxExtendedState).is_err());
    assert_eq!(CALLS.lock().unwrap().len(),calls);drop(scope);
}
'''
with tempfile.TemporaryDirectory(prefix='x86-resident-') as tmp:
    src = Path(tmp) / 'tests.rs'; binary = Path(tmp) / 'tests'
    src.write_text(code)
    subprocess.run(['rustc', '--edition=2024', '--cfg', 'feature="wc3"', '--test', str(src), '-o', str(binary)], check=True)
    subprocess.run([str(binary), '--test-threads=1'], check=True)
