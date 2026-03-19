pub mod store;
pub use trueos_vm::guest;
pub use trueos_vm::stream;

use alloc::vec::Vec;
use core::arch::x86_64::__cpuid;
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_executor::{Spawner, task};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{Deque, String};
use spin::Mutex;
use x86_64::instructions::tables::{sgdt, sidt};
use x86_64::registers::control::{Cr0, Cr0Flags, Cr3, Cr4, Cr4Flags};
use x86_64::registers::model_specific::Msr;
use x86_64::registers::rflags;
use x86_64::registers::segmentation::{CS, DS, ES, FS, GS, SS, Segment};

use crate::shell2::{ShellBackend2, ShellIo2};

const IA32_FEATURE_CONTROL: u32 = 0x3A;
const IA32_VMX_BASIC: u32 = 0x480;
const IA32_VMX_TRUE_PINBASED_CTLS: u32 = 0x48D;
const IA32_VMX_TRUE_PROCBASED_CTLS: u32 = 0x48E;
const IA32_VMX_TRUE_EXIT_CTLS: u32 = 0x48F;
const IA32_VMX_TRUE_ENTRY_CTLS: u32 = 0x490;
const IA32_VMX_PROCBASED_CTLS2: u32 = 0x48B;
const IA32_VMX_CR0_FIXED0: u32 = 0x486;
const IA32_VMX_CR0_FIXED1: u32 = 0x487;
const IA32_VMX_CR4_FIXED0: u32 = 0x488;
const IA32_VMX_CR4_FIXED1: u32 = 0x489;
const IA32_SYSENTER_CS: u32 = 0x174;
const IA32_SYSENTER_ESP: u32 = 0x175;
const IA32_SYSENTER_EIP: u32 = 0x176;
const IA32_PAT: u32 = 0x277;
const IA32_PERF_GLOBAL_CTRL: u32 = 0x38F;
const IA32_FS_BASE: u32 = 0xC000_0100;
const IA32_GS_BASE: u32 = 0xC000_0101;
const IA32_EFER: u32 = 0xC000_0080;
const IA32_FEATURE_CONTROL_LOCK: u64 = 1 << 0;
const IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX: u64 = 1 << 2;
const MAIN_LOOP_MARKER: &[u8] = b"main: entering executor loop";
const VMX_PAGE_SIZE: usize = 4096;
const PAGE_SIZE_4K: usize = 4096;
const EPT_PDPT_ENTRIES: usize = 4;
const EPT_PD_ENTRIES: usize = 512;
const GUEST_STACK_VA_BASE: u64 = 0x0000_0000_0040_0000;
const GUEST_CODE_WINDOW_BYTES: usize = 64 * 1024;
const PT_ENTRY_PRESENT: u64 = 1 << 0;
const PT_ENTRY_WRITABLE: u64 = 1 << 1;

const VMCS_CTRL_PIN_BASED: u64 = 0x4000;
const VMCS_CTRL_CPU_BASED: u64 = 0x4002;
const VMCS_CTRL_EXCEPTION_BITMAP: u64 = 0x4004;
const VMCS_CTRL_EXIT: u64 = 0x400C;
const VMCS_CTRL_ENTRY: u64 = 0x4012;
const VMCS_CTRL_SECONDARY: u64 = 0x401E;

const VMCS_CTRL_EPT_POINTER: u64 = 0x201A;
const VMCS_CTRL_VMCS_LINK_POINTER: u64 = 0x2800;

const VMCS_HOST_CR0: u64 = 0x6C00;
const VMCS_HOST_CR3: u64 = 0x6C02;
const VMCS_HOST_CR4: u64 = 0x6C04;
const VMCS_HOST_FS_BASE: u64 = 0x6C06;
const VMCS_HOST_GS_BASE: u64 = 0x6C08;
const VMCS_HOST_TR_BASE: u64 = 0x6C0A;
const VMCS_HOST_GDTR_BASE: u64 = 0x6C0C;
const VMCS_HOST_IDTR_BASE: u64 = 0x6C0E;
const VMCS_HOST_SYSENTER_ESP: u64 = 0x6C10;
const VMCS_HOST_SYSENTER_EIP: u64 = 0x6C12;
const VMCS_HOST_RSP: u64 = 0x6C14;
const VMCS_HOST_RIP: u64 = 0x6C16;
const VMCS_HOST_SYSENTER_CS: u64 = 0x4C00;
const VMCS_HOST_CS_SELECTOR: u64 = 0x0C02;
const VMCS_HOST_SS_SELECTOR: u64 = 0x0C04;
const VMCS_HOST_DS_SELECTOR: u64 = 0x0C06;
const VMCS_HOST_ES_SELECTOR: u64 = 0x0C00;
const VMCS_HOST_FS_SELECTOR: u64 = 0x0C08;
const VMCS_HOST_GS_SELECTOR: u64 = 0x0C0A;
const VMCS_HOST_TR_SELECTOR: u64 = 0x0C0C;

const VMCS_GUEST_CR0: u64 = 0x6800;
const VMCS_GUEST_CR3: u64 = 0x6802;
const VMCS_GUEST_CR4: u64 = 0x6804;
const VMCS_GUEST_ES_SELECTOR: u64 = 0x0800;
const VMCS_GUEST_CS_SELECTOR: u64 = 0x0802;
const VMCS_GUEST_SS_SELECTOR: u64 = 0x0804;
const VMCS_GUEST_DS_SELECTOR: u64 = 0x0806;
const VMCS_GUEST_FS_SELECTOR: u64 = 0x0808;
const VMCS_GUEST_GS_SELECTOR: u64 = 0x080A;
const VMCS_GUEST_LDTR_SELECTOR: u64 = 0x080C;
const VMCS_GUEST_TR_SELECTOR: u64 = 0x080E;
const VMCS_GUEST_ES_LIMIT: u64 = 0x4800;
const VMCS_GUEST_CS_LIMIT: u64 = 0x4802;
const VMCS_GUEST_SS_LIMIT: u64 = 0x4804;
const VMCS_GUEST_DS_LIMIT: u64 = 0x4806;
const VMCS_GUEST_FS_LIMIT: u64 = 0x4808;
const VMCS_GUEST_GS_LIMIT: u64 = 0x480A;
const VMCS_GUEST_LDTR_LIMIT: u64 = 0x480C;
const VMCS_GUEST_TR_LIMIT: u64 = 0x480E;
const VMCS_GUEST_GDTR_LIMIT: u64 = 0x4810;
const VMCS_GUEST_IDTR_LIMIT: u64 = 0x4812;
const VMCS_GUEST_ES_AR: u64 = 0x4814;
const VMCS_GUEST_CS_AR: u64 = 0x4816;
const VMCS_GUEST_SS_AR: u64 = 0x4818;
const VMCS_GUEST_DS_AR: u64 = 0x481A;
const VMCS_GUEST_FS_AR: u64 = 0x481C;
const VMCS_GUEST_GS_AR: u64 = 0x481E;
const VMCS_GUEST_LDTR_AR: u64 = 0x4820;
const VMCS_GUEST_TR_AR: u64 = 0x4822;
const VMCS_GUEST_INTERRUPTIBILITY: u64 = 0x4824;
const VMCS_GUEST_ACTIVITY_STATE: u64 = 0x4826;
const VMCS_GUEST_SYSENTER_CS: u64 = 0x482A;
const VMCS_GUEST_VMCS_PREEMPT_TIMER: u64 = 0x482E;
const VMCS_GUEST_ES_BASE: u64 = 0x6806;
const VMCS_GUEST_CS_BASE: u64 = 0x6808;
const VMCS_GUEST_SS_BASE: u64 = 0x680A;
const VMCS_GUEST_DS_BASE: u64 = 0x680C;
const VMCS_GUEST_FS_BASE: u64 = 0x680E;
const VMCS_GUEST_GS_BASE: u64 = 0x6810;
const VMCS_GUEST_LDTR_BASE: u64 = 0x6812;
const VMCS_GUEST_TR_BASE: u64 = 0x6814;
const VMCS_GUEST_GDTR_BASE: u64 = 0x6816;
const VMCS_GUEST_IDTR_BASE: u64 = 0x6818;
const VMCS_GUEST_DR7: u64 = 0x681A;
const VMCS_GUEST_RSP: u64 = 0x681C;
const VMCS_GUEST_RIP: u64 = 0x681E;
const VMCS_GUEST_RFLAGS: u64 = 0x6820;
const VMCS_GUEST_PENDING_DBG: u64 = 0x6822;
const VMCS_GUEST_SYSENTER_ESP: u64 = 0x6824;
const VMCS_GUEST_SYSENTER_EIP: u64 = 0x6826;
const VMCS_GUEST_IA32_EFER: u64 = 0x2806;
const VMCS_GUEST_IA32_DEBUGCTL: u64 = 0x2802;
const VMCS_GUEST_IA32_PAT: u64 = 0x2804;
const VMCS_GUEST_IA32_PERF_GLOBAL_CTRL: u64 = 0x2808;

const VMCS_HOST_IA32_PAT: u64 = 0x2C00;
const VMCS_HOST_IA32_EFER: u64 = 0x2C02;
const VMCS_HOST_IA32_PERF_GLOBAL_CTRL: u64 = 0x2C04;

const VMCS_EXIT_REASON: u64 = 0x4402;
const VMCS_VMEXIT_INSTRUCTION_LEN: u64 = 0x440C;
const VMCS_EXIT_QUALIFICATION: u64 = 0x6400;
const VMCS_VM_INSTRUCTION_ERROR: u64 = 0x4400;
const VMCS_VMEXIT_GUEST_RIP: u64 = 0x681E;

const PROC_BASED_HLT_EXITING: u64 = 1 << 7;
const PROC_BASED_ACTIVATE_SECONDARY: u64 = 1 << 31;
const PROC2_BASED_ENABLE_EPT: u64 = 1 << 1;
const EXIT_CTL_HOST_ADDR_SPACE_SIZE: u64 = 1 << 9;
const ENTRY_CTL_IA32E_MODE_GUEST: u64 = 1 << 9;
const RFLAGS_RESERVED_BIT1: u64 = 1 << 1;
const RFLAGS_IF: u64 = 1 << 9;
const EXCEPTION_BITMAP_ALL: u64 = 0xFFFF_FFFF;
const VMEXIT_REASON_VMCALL: u64 = 0x12;
const HV_GDT_SEL_CODE: u16 = 0x08;
const HV_GDT_SEL_DATA: u16 = 0x10;
const HV_GDT_SEL_TSS: u16 = 0x18;
const HV_GDT_DESC_CODE64: u64 = 0x00AF_9B00_0000_FFFF;
const HV_GDT_DESC_DATA64: u64 = 0x00AF_9300_0000_FFFF;
const ELF64_HEADER_LEN: usize = 64;
const GUEST_STACK_BYTES: usize = 64 * 1024;

static VM1_RUNNING: AtomicBool = AtomicBool::new(false);
static VM1_STARTING: AtomicBool = AtomicBool::new(false);
static VM1_STOP_REQ: AtomicBool = AtomicBool::new(false);
static VM1_MARKER_SEEN: AtomicBool = AtomicBool::new(false);
static VM1_GUEST_BOOT_ARMED: AtomicBool = AtomicBool::new(false);
static HV_LOG_SEQ: AtomicU64 = AtomicU64::new(0);

const HV_LOG_CAP: usize = 64;
const HV_LOG_LINE: usize = 200;

#[derive(Clone)]
struct HvLogEntry {
    seq: u64,
    msg: String<HV_LOG_LINE>,
}

static HV_LOG_RING: Mutex<Deque<HvLogEntry, HV_LOG_CAP>> = Mutex::new(Deque::new());

#[repr(C, align(4096))]
struct VmxPage([u8; VMX_PAGE_SIZE]);

static mut VMXON_REGION: VmxPage = VmxPage([0u8; VMX_PAGE_SIZE]);
static mut VMCS_REGION: VmxPage = VmxPage([0u8; VMX_PAGE_SIZE]);

#[repr(align(16))]
struct GuestStack([u8; GUEST_STACK_BYTES]);

#[repr(C, align(4096))]
#[derive(Copy, Clone)]
struct EptPage([u64; 512]);

#[repr(C, align(4096))]
#[derive(Copy, Clone)]
struct GuestPage([u64; 512]);

static mut EPT_PML4: EptPage = EptPage([0u64; 512]);
static mut EPT_PDPT: EptPage = EptPage([0u64; 512]);
static mut EPT_PD: [EptPage; EPT_PDPT_ENTRIES] = [EptPage([0u64; 512]); EPT_PDPT_ENTRIES];
static mut GUEST_PML4: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_LOW_PDPT: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_LOW_PD: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_STACK_PT: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_HIGH_PDPT: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_HIGH_PD: GuestPage = GuestPage([0u64; 512]);
static mut GUEST_CODE_PT: GuestPage = GuestPage([0u64; 512]);
static mut HV_HOST_GDT: [u64; 8] = [0u64; 8];
static mut HV_HOST_TSS: [u8; 104] = [0u8; 104];
static mut VM1_GUEST_STACK: GuestStack = GuestStack([0u8; GUEST_STACK_BYTES]);
static mut VMX_WRAPPER_RESULT: LaunchResult = LaunchResult {
    entered: 0,
    launch_failed: 0,
    _pad: [0; 6],
    exit_reason: 0,
    exit_qualification: 0,
    guest_rip: 0,
    instr_err: 0,
};
static VM1_SNAPSHOT_META: Mutex<Option<Vm1SnapshotMeta>> = Mutex::new(None);
static VM1_RESTORE_META: Mutex<Option<Vm1SnapshotMeta>> = Mutex::new(None);

const VM1_SNAPSHOT_MAGIC: u32 = 0x3153_4D56; // "VMS1"
const VM1_SNAPSHOT_VERSION: u32 = 1;
const VM1_SNAPSHOT_PATH: &str = "vm/vm1.snapshot";

#[derive(Copy, Clone, Default)]
struct LaunchResult {
    entered: u8,
    launch_failed: u8,
    _pad: [u8; 6],
    exit_reason: u64,
    exit_qualification: u64,
    guest_rip: u64,
    instr_err: u64,
}

#[derive(Copy, Clone)]
struct Vm1SnapshotMeta {
    guest_cr3: u64,
    guest_rip: u64,
    guest_rsp: u64,
    code_base: u64,
    code_len: u64,
    exit_reason: u64,
    exit_qualification: u64,
    exit_guest_rip: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct Vm1SnapshotHeader {
    magic: u32,
    version: u32,
    guest_cr3: u64,
    guest_rip: u64,
    guest_rsp: u64,
    code_base: u64,
    code_len: u64,
    exit_reason: u64,
    exit_qualification: u64,
    exit_guest_rip: u64,
    guest_stack_bytes: u64,
    guest_page_bytes: u64,
}

#[derive(Copy, Clone, Debug)]
pub enum SaveError {
    NoRoot,
    NoSnapshot,
    BeginWrite,
    Io(crate::disc::block::Error),
}

#[derive(Copy, Clone, Debug)]
pub enum RestoreError {
    NoRoot,
    MissingFile,
    Read(crate::disc::block::Error),
    BadSnapshot,
    CodeMismatch,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum StartError {
    AlreadyRunning,
    VmxUnsupported,
    MissingGuestModule,
    SpawnFailed,
}

#[derive(Copy, Clone, Debug)]
pub struct HvStatus {
    pub vendor_intel: bool,
    pub has_msr: bool,
    pub has_vmx: bool,
    pub feature_control_locked: bool,
    pub feature_control_vmx_outside_smx: bool,
    pub vm1_running: bool,
    pub vm1_starting: bool,
    pub vm1_marker_seen: bool,
    pub guest_module_present: bool,
}

pub fn status() -> HvStatus {
    let (vendor_intel, has_msr, has_vmx, fc_locked, fc_vmx_outside_smx) = vmx_caps();
    HvStatus {
        vendor_intel,
        has_msr,
        has_vmx,
        feature_control_locked: fc_locked,
        feature_control_vmx_outside_smx: fc_vmx_outside_smx,
        vm1_running: VM1_RUNNING.load(Ordering::Acquire),
        vm1_starting: VM1_STARTING.load(Ordering::Acquire),
        vm1_marker_seen: VM1_MARKER_SEEN.load(Ordering::Acquire),
        guest_module_present: crate::limine::guest_kernel_bytes().is_some(),
    }
}

pub fn start(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Result<(), StartError> {
    if VM1_RUNNING.load(Ordering::Acquire) || VM1_STARTING.load(Ordering::Acquire) {
        return Err(StartError::AlreadyRunning);
    }

    // Try to enable usage of VMX if not already locked.
    let (compatible, has_msr, _, locked, _) = vmx_caps();
    if compatible && has_msr && !locked {
        // SAFETY: Only writes MSR if supported and unlocked.
        unsafe {
            let mut val = Msr::new(IA32_FEATURE_CONTROL).read();
            val |= IA32_FEATURE_CONTROL_LOCK | IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX;
            Msr::new(IA32_FEATURE_CONTROL).write(val);
        }
    }

    let caps = status();
    if !caps.vendor_intel
        || !caps.has_msr
        || !caps.has_vmx
        || !caps.feature_control_locked
        || !caps.feature_control_vmx_outside_smx
    {
        hvlogf(format_args!(
            "hv: start failed: vendor={} msr={} vmx={} locked={} outside_smx={}",
            caps.vendor_intel,
            caps.has_msr,
            caps.has_vmx,
            caps.feature_control_locked,
            caps.feature_control_vmx_outside_smx
        ));
        let r0 = __cpuid(0);
        hvlogf(format_args!(
            "hv: cpuid0 ebx=0x{:X} ecx=0x{:X} edx=0x{:X}",
            r0.ebx, r0.ecx, r0.edx
        ));
        let r1 = __cpuid(1);
        hvlogf(format_args!(
            "hv: cpuid1 ecx=0x{:X} edx=0x{:X}",
            r1.ecx, r1.edx
        ));
        return Err(StartError::VmxUnsupported);
    }

    if crate::limine::guest_kernel_bytes().is_none() {
        return Err(StartError::MissingGuestModule);
    }

    VM1_STOP_REQ.store(false, Ordering::Release);
    VM1_MARKER_SEEN.store(false, Ordering::Release);
    VM1_STARTING.store(true, Ordering::Release);

    if spawner.spawn(vm1_task(io)).is_err() {
        VM1_STARTING.store(false, Ordering::Release);
        return Err(StartError::SpawnFailed);
    }
    Ok(())
}

pub fn stop() -> bool {
    if VM1_RUNNING.load(Ordering::Acquire) || VM1_STARTING.load(Ordering::Acquire) {
        VM1_STOP_REQ.store(true, Ordering::Release);
        hvlogf(format_args!("hv: vm1 lifecycle: stop requested"));
        true
    } else {
        hvlogf(format_args!(
            "hv: vm1 lifecycle: stop ignored (not running)"
        ));
        false
    }
}

pub fn write_logs(io: &dyn ShellIo2) {
    let mut lines: [Option<HvLogEntry>; HV_LOG_CAP] = core::array::from_fn(|_| None);
    let mut n = 0usize;
    {
        let ring = HV_LOG_RING.lock();
        for e in ring.iter() {
            if n >= HV_LOG_CAP {
                break;
            }
            lines[n] = Some(e.clone());
            n += 1;
        }
    }

    if n == 0 {
        io.write_str("hv: log empty\r\n");
        return;
    }

    for i in 0..n {
        if let Some(e) = &lines[i] {
            io.write_fmt(format_args!("hvlog[{}]: {}\r\n", e.seq, e.msg.as_str()));
        }
    }
}

pub fn save_snapshot() -> Result<usize, SaveError> {
    let bytes = snapshot_bytes()?;
    crate::hv::store::save_bytes(bytes).map_err(map_store_save_error)
}

pub fn restore_snapshot() -> Result<usize, RestoreError> {
    let bytes = crate::hv::store::load_bytes().map_err(map_store_restore_error)?;

    restore_snapshot_bytes(bytes.as_slice())?;
    Ok(bytes.len())
}

fn map_store_save_error(err: crate::hv::store::VmStoreError) -> SaveError {
    match err {
        crate::hv::store::VmStoreError::ServiceOffline => SaveError::Io(crate::disc::block::Error::NotReady),
        crate::hv::store::VmStoreError::QueueFull => SaveError::Io(crate::disc::block::Error::NotReady),
        crate::hv::store::VmStoreError::Create(e)
        | crate::hv::store::VmStoreError::Format(e)
        | crate::hv::store::VmStoreError::BeginWrite(e)
        | crate::hv::store::VmStoreError::Write(e)
        | crate::hv::store::VmStoreError::Read(e) => SaveError::Io(e),
        crate::hv::store::VmStoreError::MissingSnapshot => SaveError::BeginWrite,
    }
}

fn map_store_restore_error(err: crate::hv::store::VmStoreError) -> RestoreError {
    match err {
        crate::hv::store::VmStoreError::MissingSnapshot => RestoreError::MissingFile,
        crate::hv::store::VmStoreError::ServiceOffline => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::QueueFull => {
            RestoreError::Read(crate::disc::block::Error::NotReady)
        }
        crate::hv::store::VmStoreError::Create(e)
        | crate::hv::store::VmStoreError::Format(e)
        | crate::hv::store::VmStoreError::BeginWrite(e)
        | crate::hv::store::VmStoreError::Read(e)
        | crate::hv::store::VmStoreError::Write(e) => RestoreError::Read(e),
    }
}

fn vmexit_is_preserve(lr: LaunchResult) -> bool {
    lr.entered != 0 && lr.launch_failed == 0 && (lr.exit_reason & 0xFFFF) == VMEXIT_REASON_VMCALL
}

fn snapshot_on_preserve_exit() {
    match snapshot_bytes() {
        Ok(bytes) => match crate::hv::store::save_bytes(bytes) {
            Ok(saved) => hvlogf(format_args!(
                "hv: vm1 reporting: preserve snapshot saved store=hv-ramdisk path=vm/vm1.snapshot bytes={}",
                saved
            )),
            Err(e) => hvlogf(format_args!(
                "hv: vm1 reporting: preserve snapshot save failed ({:?})",
                e
            )),
        },
        Err(e) => hvlogf(format_args!(
            "hv: vm1 reporting: preserve snapshot bytes failed ({:?})",
            e
        )),
    }
}

pub fn guest_boot_take() -> bool {
    VM1_GUEST_BOOT_ARMED.swap(false, Ordering::AcqRel)
}

#[task(pool_size = 1)]
async fn vm1_task(_io: &'static dyn ShellBackend2) {
    VM1_STARTING.store(false, Ordering::Release);
    VM1_RUNNING.store(true, Ordering::Release);
    hvlogf(format_args!("hv: vm1 lifecycle: starting"));

    let guest = crate::limine::guest_kernel_bytes();
    let guest_len = guest.map(|b| b.len()).unwrap_or(0);
    hvlogf(format_args!("hv: vm1 lifecycle: guest bytes={}", guest_len));
    if let Some(bytes) = guest {
        if let Some(entry) = guest_kernel_elf_entry(bytes) {
            hvlogf(format_args!(
                "hv: vm1 reporting: guest elf entry=0x{:016X} vmx_guest_entry=0x{:016X}",
                entry,
                guest_launch_rip()
            ));
        } else {
            hvlogf(format_args!(
                "hv: vm1 reporting: guest bytes present but ELF entry parse failed; vmx_guest_entry=0x{:016X}",
                guest_launch_rip()
            ));
        }
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: vmx preflight ok, stage=m1"
    ));
    hvlogf(format_args!(
        "hv: vm1 reporting: vlayer policy=integrity-first"
    ));
    match vmx_smoke() {
        Ok(()) => hvlogf(format_args!(
            "hv: vm1 reporting: vmx smoke ok (vmxon/vmclear/vmptrld/vmxoff)"
        )),
        Err(e) => hvlogf(format_args!("hv: vm1 reporting: vmx smoke failed ({})", e)),
    }
    match vmx_launch_once_with_ept() {
        Ok(lr) => {
            capture_snapshot_meta(lr);
            if vmexit_is_preserve(lr) {
                snapshot_on_preserve_exit();
            }
            hvlogf(format_args!(
                "hv: vm1 reporting: vmlaunch entered={} launch_failed={} exit_reason=0x{:X} exit_qual=0x{:X} guest_rip=0x{:016X}",
                lr.entered, lr.launch_failed, lr.exit_reason, lr.exit_qualification, lr.guest_rip
            ));
        }
        Err(e) => hvlogf(format_args!(
            "hv: vm1 reporting: vmlaunch/ept failed ({})",
            e
        )),
    }

    if let Some(bytes) = guest {
        if contains_bytes(bytes, MAIN_LOOP_MARKER) {
            VM1_MARKER_SEEN.store(true, Ordering::Release);
            hvlogf(format_args!(
                "hv: vm1 reporting: main: entering executor loop"
            ));
        } else {
            hvlogf(format_args!(
                "hv: vm1 reporting: guest image missing marker '{}'",
                "main: entering executor loop"
            ));
        }
    } else {
        hvlogf(format_args!(
            "hv: vm1 reporting: guest module missing during task startup"
        ));
    }

    while !VM1_STOP_REQ.load(Ordering::Acquire) {
        Timer::after(EmbassyDuration::from_millis(250)).await;
    }

    VM1_RUNNING.store(false, Ordering::Release);
    VM1_STARTING.store(false, Ordering::Release);
    VM1_STOP_REQ.store(false, Ordering::Release);
    hvlogf(format_args!("hv: vm1 lifecycle: stopped"));
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn vmx_caps() -> (bool, bool, bool, bool, bool) {
    // GenuineIntel
    let r0 = __cpuid(0);
    let vendor_intel = r0.ebx == 0x756e6547 && r0.edx == 0x49656e69 && r0.ecx == 0x6c65746e;

    // Also accept generic TCG if VMX is present (we can't check VMX here easily without r1, but checking vendor is enough for this flag)
    // TCGTCGTCGTCG: EBX=0x54474354 EDX=0x47435447 ECX=0x43544743
    // AuthenticAMD: EBX=0x68747541 EDX=0x69746e65 ECX=0x444d4163
    let known_compatible = vendor_intel
        || (r0.ebx == 0x54474354 && r0.edx == 0x47435447 && r0.ecx == 0x43544743) // TCGTCGTCGTCG
        || (r0.ebx == 0x68747541 && r0.edx == 0x69746e65 && r0.ecx == 0x444d4163); // AuthenticAMD (if has_vmx, emulated)

    let r1 = __cpuid(1);
    let has_msr = (r1.edx & (1 << 5)) != 0;
    let has_vmx = (r1.ecx & (1 << 5)) != 0;

    let (mut feature_control_locked, mut feature_control_vmx_outside_smx) = (false, false);
    // Relaxed vendor check for MSR reading
    if (known_compatible || has_vmx) && has_msr {
        // SAFETY: guarded by CPUID MSR capability bit on Intel CPUs.
        let val = unsafe { Msr::new(IA32_FEATURE_CONTROL).read() };
        feature_control_locked = (val & IA32_FEATURE_CONTROL_LOCK) != 0;
        feature_control_vmx_outside_smx = (val & IA32_FEATURE_CONTROL_VMX_OUTSIDE_SMX) != 0;
    }

    (
        known_compatible,
        has_msr,
        has_vmx,
        feature_control_locked,
        feature_control_vmx_outside_smx,
    )
}

fn vmx_launch_once_with_ept() -> Result<LaunchResult, &'static str> {
    let caps = status();
    if !caps.vendor_intel || !caps.has_msr || !caps.has_vmx {
        return Err("vmx unsupported");
    }

    let cr0_fixed0 = unsafe { Msr::new(IA32_VMX_CR0_FIXED0).read() };
    let cr0_fixed1 = unsafe { Msr::new(IA32_VMX_CR0_FIXED1).read() };
    let cr4_fixed0 = unsafe { Msr::new(IA32_VMX_CR4_FIXED0).read() };
    let cr4_fixed1 = unsafe { Msr::new(IA32_VMX_CR4_FIXED1).read() };

    let mut cr0 = Cr0::read().bits();
    let mut cr4 = Cr4::read().bits();
    cr0 = (cr0 | cr0_fixed0) & cr0_fixed1;
    cr4 = (cr4 | cr4_fixed0) & cr4_fixed1;
    cr4 |= Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS.bits();
    unsafe {
        Cr0::write(Cr0Flags::from_bits_truncate(cr0));
        Cr4::write(Cr4Flags::from_bits_truncate(cr4));
    }

    let basic = unsafe { Msr::new(IA32_VMX_BASIC).read() };
    let revision = (basic & 0x7fff_ffff) as u32;

    let vmxon_va = unsafe { core::ptr::addr_of_mut!(VMXON_REGION.0) } as *mut u8;
    let vmcs_va = unsafe { core::ptr::addr_of_mut!(VMCS_REGION.0) } as *mut u8;
    unsafe {
        core::ptr::write_bytes(vmxon_va, 0, VMX_PAGE_SIZE);
        core::ptr::write_bytes(vmcs_va, 0, VMX_PAGE_SIZE);
        *(vmxon_va as *mut u32) = revision;
        *(vmcs_va as *mut u32) = revision;
    }

    let vmxon_pa = kernel_va_to_pa(vmxon_va as u64).ok_or("vmxon pa")?;
    let vmcs_pa = kernel_va_to_pa(vmcs_va as u64).ok_or("vmcs pa")?;
    hvlogf(format_args!(
        "hv: vm1 reporting: vmlaunch prep revision=0x{:08X} vmxon_pa=0x{:016X} vmcs_pa=0x{:016X}",
        revision, vmxon_pa, vmcs_pa
    ));

    if !vmxon(vmxon_pa) {
        return Err("vmxon");
    }
    if !vmclear(vmcs_pa) {
        let _ = vmxoff();
        return Err("vmclear");
    }
    if !vmptrld(vmcs_pa) {
        let _ = vmxoff();
        return Err("vmptrld");
    }

    let eptp = match build_ept_identity_4g() {
        Ok(v) => v,
        Err(e) => {
            let _ = vmxoff();
            return Err(e);
        }
    };
    if let Err(e) = setup_vmcs_for_launch(eptp) {
        let _ = vmxoff();
        return Err(e);
    }

    let mut lr = LaunchResult::default();
    VM1_GUEST_BOOT_ARMED.store(true, Ordering::Release);
    vmlaunch_once_wrapper(&mut lr);
    VM1_GUEST_BOOT_ARMED.store(false, Ordering::Release);
    if lr.launch_failed != 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmlaunch failed instr_err={} rip=0x{:016X}",
            lr.instr_err,
            current_rip()
        ));
    } else if lr.entered != 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: first vmexit reason=0x{:X} qual=0x{:X} guest_rip=0x{:016X}",
            lr.exit_reason, lr.exit_qualification, lr.guest_rip
        ));
        if (lr.exit_reason & 0xFFFF) == 0xC {
            match handle_hlt_exit_resume_once() {
                Ok((lr2, rip_before, exit_len, rip_after)) => {
                    hvlogf(format_args!(
                        "hv: vm1 reporting: hlt-resume once rip=0x{:016X} len={} next=0x{:016X}",
                        rip_before, exit_len, rip_after
                    ));
                    if lr2.launch_failed != 0 {
                        hvlogf(format_args!(
                            "hv: vm1 reporting: vmresume failed instr_err={} rip=0x{:016X}",
                            lr2.instr_err,
                            current_rip()
                        ));
                    } else if lr2.entered != 0 {
                        hvlogf(format_args!(
                            "hv: vm1 reporting: second vmexit reason=0x{:X} qual=0x{:X} guest_rip=0x{:016X}",
                            lr2.exit_reason, lr2.exit_qualification, lr2.guest_rip
                        ));
                    }
                    lr = lr2;
                }
                Err(e) => hvlogf(format_args!(
                    "hv: vm1 reporting: hlt-resume once failed ({})",
                    e
                )),
            }
        }
    }
    if !vmxoff() {
        return Err("vmxoff");
    }
    Ok(lr)
}

fn setup_vmcs_for_launch(eptp: u64) -> Result<(), &'static str> {
    let basic = unsafe { Msr::new(IA32_VMX_BASIC).read() };
    let true_ctls = ((basic >> 55) & 1) != 0;
    let pin_msr = if true_ctls {
        IA32_VMX_TRUE_PINBASED_CTLS
    } else {
        0x481
    };
    let proc_msr = if true_ctls {
        IA32_VMX_TRUE_PROCBASED_CTLS
    } else {
        0x482
    };
    let exit_msr = if true_ctls {
        IA32_VMX_TRUE_EXIT_CTLS
    } else {
        0x483
    };
    let entry_msr = if true_ctls {
        IA32_VMX_TRUE_ENTRY_CTLS
    } else {
        0x484
    };

    let pin = adjust_vmx_ctrl(pin_msr, 0);
    let proc = adjust_vmx_ctrl(
        proc_msr,
        PROC_BASED_HLT_EXITING | PROC_BASED_ACTIVATE_SECONDARY,
    );
    let proc2 = adjust_vmx_ctrl(IA32_VMX_PROCBASED_CTLS2, PROC2_BASED_ENABLE_EPT);
    let exit = adjust_vmx_ctrl(exit_msr, EXIT_CTL_HOST_ADDR_SPACE_SIZE);
    let entry = adjust_vmx_ctrl(entry_msr, ENTRY_CTL_IA32E_MODE_GUEST);
    hvlogf(format_args!(
        "hv: vm1 reporting: vmcs controls pin=0x{:08X} proc=0x{:08X} proc2=0x{:08X} exit=0x{:08X} entry=0x{:08X}",
        pin as u32, proc as u32, proc2 as u32, exit as u32, entry as u32
    ));

    if (proc & PROC_BASED_ACTIVATE_SECONDARY) == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmcs ctrl unsupported: primary bit ACTIVATE_SECONDARY not available"
        ));
        return Err("secondary controls unsupported");
    }
    if (proc2 & PROC2_BASED_ENABLE_EPT) == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmcs ctrl unsupported: secondary bit ENABLE_EPT not available"
        ));
        return Err("ept unsupported");
    }

    vmwrite(VMCS_CTRL_PIN_BASED, pin)?;
    vmwrite(VMCS_CTRL_CPU_BASED, proc)?;
    vmwrite(VMCS_CTRL_SECONDARY, proc2)?;
    // Trap guest exceptions to the host until the guest has its own coherent IDT/ISR world.
    vmwrite(VMCS_CTRL_EXCEPTION_BITMAP, EXCEPTION_BITMAP_ALL)?;
    vmwrite(VMCS_CTRL_EXIT, exit)?;
    vmwrite(VMCS_CTRL_ENTRY, entry)?;
    vmwrite(VMCS_CTRL_EPT_POINTER, eptp)?;
    vmwrite(VMCS_CTRL_VMCS_LINK_POINTER, !0u64)?;

    let (host_cr3, _) = Cr3::read();
    let host_cr0 = Cr0::read().bits();
    let host_cr4 = Cr4::read().bits();
    let guest_rflags = rflags::read().bits();
    let mut tr_sel = read_tr_selector();
    let gdtr = sgdt();
    let idtr = sidt();
    let mut host_gdtr_base = gdtr.base.as_u64();
    let mut host_cs = (CS::get_reg().0 & !0x7) as u64;
    let mut host_ss = (SS::get_reg().0 & !0x7) as u64;
    let mut host_ds = (DS::get_reg().0 & !0x7) as u64;
    let mut host_es = (ES::get_reg().0 & !0x7) as u64;
    let mut host_fs = (FS::get_reg().0 & !0x7) as u64;
    let mut host_gs = (GS::get_reg().0 & !0x7) as u64;
    let tr_base: u64;

    if tr_sel == 0 {
        if let Some((busy_sel, 0xB)) = find_tss_selector(gdtr.base.as_u64(), gdtr.limit) {
            tr_sel = busy_sel;
            hvlogf(format_args!(
                "hv: vm1 reporting: host-state recovered: adopted busy tss selector=0x{:04X}",
                tr_sel
            ));
        } else if let Some((avail_sel, 0x9)) = find_tss_selector(gdtr.base.as_u64(), gdtr.limit) {
            load_tr_selector(avail_sel);
            tr_sel = read_tr_selector();
            if tr_sel == 0 {
                hvlogf(format_args!(
                    "hv: vm1 reporting: host-state invalid: tr selector null after ltr candidate=0x{:04X}",
                    avail_sel
                ));
                return Err("host tr ltr");
            }
            hvlogf(format_args!(
                "hv: vm1 reporting: host-state recovered: loaded tr selector=0x{:04X}",
                tr_sel
            ));
        } else {
            let synth = synthesize_host_gdt_tss();
            tr_sel = synth.tr_sel;
            host_gdtr_base = synth.gdt_base;
            host_cs = synth.cs_sel as u64;
            host_ss = synth.data_sel as u64;
            host_ds = synth.data_sel as u64;
            host_es = synth.data_sel as u64;
            host_fs = 0;
            host_gs = 0;
            tr_base = synth.tr_base;
            hvlogf(format_args!(
                "hv: vm1 reporting: host-state recovered: using synthetic hv gdt+tss tr=0x{:04X} tr_base=0x{:016X}",
                synth.tr_sel, synth.tr_base
            ));
            // Continue with synthesized host state.
            let fs_base = unsafe { Msr::new(IA32_FS_BASE).read() };
            let gs_base = unsafe { Msr::new(IA32_GS_BASE).read() };
            let sysenter_cs = unsafe { Msr::new(IA32_SYSENTER_CS).read() };
            let sysenter_esp = unsafe { Msr::new(IA32_SYSENTER_ESP).read() };
            let sysenter_eip = unsafe { Msr::new(IA32_SYSENTER_EIP).read() };
            let r0 = __cpuid(0);
            let r1 = __cpuid(1);
            let has_pat = (r1.edx & (1 << 16)) != 0;
            let has_perfmon = r0.eax >= 0xA && (__cpuid(0xA).eax & 0xFF) != 0;
            let pat = if has_pat {
                unsafe { Msr::new(IA32_PAT).read() }
            } else {
                0x0007_0406_0007_0406
            };
            let perf_global = if has_perfmon {
                unsafe { Msr::new(IA32_PERF_GLOBAL_CTRL).read() }
            } else {
                0
            };
            let efer = unsafe { Msr::new(IA32_EFER).read() };
            let host_tr = (tr_sel & !0x7) as u64;
            let host_sysenter_cs = sysenter_cs & 0xFFFF;

            if host_cs == 0 || host_ss == 0 || host_tr == 0 {
                hvlogf(format_args!(
                    "hv: vm1 reporting: host-state invalid selectors cs=0x{:04X} ss=0x{:04X} tr=0x{:04X}",
                    host_cs as u16, host_ss as u16, host_tr as u16
                ));
                return Err("host selectors");
            }
            if !is_canonical(tr_base)
                || !is_canonical(fs_base)
                || !is_canonical(gs_base)
                || !is_canonical(host_gdtr_base)
                || !is_canonical(idtr.base.as_u64())
            {
                hvlogf(format_args!(
                    "hv: vm1 reporting: host-state invalid bases tr=0x{:016X} fs=0x{:016X} gs=0x{:016X} gdtr=0x{:016X} idtr=0x{:016X}",
                    tr_base,
                    fs_base,
                    gs_base,
                    host_gdtr_base,
                    idtr.base.as_u64()
                ));
                return Err("host bases");
            }
            hvlogf(format_args!(
                "hv: vm1 reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
                host_cs as u16, host_ss as u16, host_tr as u16, tr_base
            ));

            vmwrite(VMCS_HOST_CR0, host_cr0)?;
            vmwrite(VMCS_HOST_CR3, host_cr3.start_address().as_u64())?;
            vmwrite(VMCS_HOST_CR4, host_cr4)?;
            vmwrite(VMCS_HOST_CS_SELECTOR, host_cs)?;
            vmwrite(VMCS_HOST_SS_SELECTOR, host_ss)?;
            vmwrite(VMCS_HOST_DS_SELECTOR, host_ds)?;
            vmwrite(VMCS_HOST_ES_SELECTOR, host_es)?;
            vmwrite(VMCS_HOST_FS_SELECTOR, host_fs)?;
            vmwrite(VMCS_HOST_GS_SELECTOR, host_gs)?;
            vmwrite(VMCS_HOST_TR_SELECTOR, host_tr)?;
            vmwrite(VMCS_HOST_FS_BASE, fs_base)?;
            vmwrite(VMCS_HOST_GS_BASE, gs_base)?;
            vmwrite(VMCS_HOST_TR_BASE, tr_base)?;
            vmwrite(VMCS_HOST_GDTR_BASE, host_gdtr_base)?;
            vmwrite(VMCS_HOST_IDTR_BASE, idtr.base.as_u64())?;
            vmwrite(VMCS_HOST_SYSENTER_CS, host_sysenter_cs)?;
            vmwrite(VMCS_HOST_SYSENTER_ESP, sysenter_esp)?;
            vmwrite(VMCS_HOST_SYSENTER_EIP, sysenter_eip)?;
            vmwrite(VMCS_HOST_IA32_PAT, pat)?;
            vmwrite(VMCS_HOST_IA32_EFER, efer)?;
            vmwrite(VMCS_HOST_IA32_PERF_GLOBAL_CTRL, perf_global)?;

            let restored = active_restore_meta();
            let guest_rip = restored.map(|m| m.guest_rip).unwrap_or_else(guest_launch_rip);
            let guest_rsp = restored.map(|m| m.guest_rsp).unwrap_or_else(guest_stack_top);
            let guest_cr3 = if restored.is_some() {
                current_guest_cr3_pa()?
            } else {
                build_guest_cr3(guest_rip, guest_rsp)?
            };
            vmwrite(VMCS_GUEST_CR0, host_cr0)?;
            vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
            vmwrite(VMCS_GUEST_CR4, host_cr4)?;
            vmwrite(VMCS_GUEST_RFLAGS, (guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
            vmwrite(VMCS_GUEST_RIP, guest_rip)?;
            vmwrite(VMCS_GUEST_RSP, guest_rsp)?;
            vmwrite(VMCS_GUEST_DR7, 0x400)?;
            vmwrite(VMCS_GUEST_IA32_DEBUGCTL, 0)?;
            vmwrite(VMCS_GUEST_SYSENTER_CS, sysenter_cs)?;
            vmwrite(VMCS_GUEST_SYSENTER_ESP, sysenter_esp)?;
            vmwrite(VMCS_GUEST_SYSENTER_EIP, sysenter_eip)?;
            vmwrite(VMCS_GUEST_IA32_PAT, pat)?;
            vmwrite(VMCS_GUEST_IA32_EFER, efer)?;
            vmwrite(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL, perf_global)?;
            vmwrite(VMCS_GUEST_ACTIVITY_STATE, 0)?;
            vmwrite(VMCS_GUEST_INTERRUPTIBILITY, 0)?;
            vmwrite(VMCS_GUEST_PENDING_DBG, 0)?;
            vmwrite(VMCS_GUEST_VMCS_PREEMPT_TIMER, 0)?;

            let cs = host_cs;
            let ss = host_ss;
            let ds = host_ds;
            let es = host_es;
            let fs = host_fs;
            let gs = host_gs;
            let tr = tr_sel as u64;
            vmwrite(VMCS_GUEST_CS_SELECTOR, cs)?;
            vmwrite(VMCS_GUEST_SS_SELECTOR, ss)?;
            vmwrite(VMCS_GUEST_DS_SELECTOR, ds)?;
            vmwrite(VMCS_GUEST_ES_SELECTOR, es)?;
            vmwrite(VMCS_GUEST_FS_SELECTOR, fs)?;
            vmwrite(VMCS_GUEST_GS_SELECTOR, gs)?;
            vmwrite(VMCS_GUEST_TR_SELECTOR, tr)?;
            vmwrite(VMCS_GUEST_LDTR_SELECTOR, 0)?;

            vmwrite(VMCS_GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_SS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_DS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_ES_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_FS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_GS_LIMIT, 0xFFFF_FFFF)?;
            vmwrite(VMCS_GUEST_TR_LIMIT, 0xFFFF)?;
            vmwrite(VMCS_GUEST_LDTR_LIMIT, 0)?;
            vmwrite(VMCS_GUEST_GDTR_LIMIT, gdtr.limit as u64)?;
            vmwrite(VMCS_GUEST_IDTR_LIMIT, idtr.limit as u64)?;

            vmwrite(VMCS_GUEST_CS_BASE, 0)?;
            vmwrite(VMCS_GUEST_SS_BASE, 0)?;
            vmwrite(VMCS_GUEST_DS_BASE, 0)?;
            vmwrite(VMCS_GUEST_ES_BASE, 0)?;
            vmwrite(VMCS_GUEST_FS_BASE, fs_base)?;
            vmwrite(VMCS_GUEST_GS_BASE, gs_base)?;
            vmwrite(VMCS_GUEST_TR_BASE, tr_base)?;
            vmwrite(VMCS_GUEST_LDTR_BASE, 0)?;
            vmwrite(VMCS_GUEST_GDTR_BASE, gdtr.base.as_u64())?;
            vmwrite(VMCS_GUEST_IDTR_BASE, idtr.base.as_u64())?;

            vmwrite(VMCS_GUEST_CS_AR, 0xA09B)?;
            vmwrite(VMCS_GUEST_SS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_DS_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_ES_AR, 0xC093)?;
            vmwrite(VMCS_GUEST_FS_AR, 0x10000)?;
            vmwrite(VMCS_GUEST_GS_AR, 0x10000)?;
            vmwrite(VMCS_GUEST_TR_AR, 0x008B)?;
            vmwrite(VMCS_GUEST_LDTR_AR, 0x10000)?;

            return Ok(());
        }
    }
    if tr_sel == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: host-state invalid: tr selector remains null after recovery"
        ));
        return Err("host tr selector");
    }
    tr_base = match tss_base_from_gdt(host_gdtr_base, tr_sel) {
        Some(v) => v,
        None => {
            hvlogf(format_args!(
                "hv: vm1 reporting: host-state invalid: unable to resolve tss base from gdt"
            ));
            return Err("host tr base");
        }
    };
    let fs_base = unsafe { Msr::new(IA32_FS_BASE).read() };
    let gs_base = unsafe { Msr::new(IA32_GS_BASE).read() };
    let sysenter_cs = unsafe { Msr::new(IA32_SYSENTER_CS).read() };
    let sysenter_esp = unsafe { Msr::new(IA32_SYSENTER_ESP).read() };
    let sysenter_eip = unsafe { Msr::new(IA32_SYSENTER_EIP).read() };
    let r0 = __cpuid(0);
    let r1 = __cpuid(1);
    let has_pat = (r1.edx & (1 << 16)) != 0;
    let has_perfmon = r0.eax >= 0xA && (__cpuid(0xA).eax & 0xFF) != 0;
    let pat = if has_pat {
        unsafe { Msr::new(IA32_PAT).read() }
    } else {
        // Architectural fallback.
        0x0007_0406_0007_0406
    };
    let perf_global = if has_perfmon {
        unsafe { Msr::new(IA32_PERF_GLOBAL_CTRL).read() }
    } else {
        0
    };
    let efer = unsafe { Msr::new(IA32_EFER).read() };

    let host_tr = (tr_sel & !0x7) as u64;
    let host_sysenter_cs = sysenter_cs & 0xFFFF;

    if host_cs == 0 || host_ss == 0 || host_tr == 0 {
        hvlogf(format_args!(
            "hv: vm1 reporting: host-state invalid selectors cs=0x{:04X} ss=0x{:04X} tr=0x{:04X}",
            host_cs as u16, host_ss as u16, host_tr as u16
        ));
        return Err("host selectors");
    }
    if !is_canonical(tr_base)
        || !is_canonical(fs_base)
        || !is_canonical(gs_base)
        || !is_canonical(host_gdtr_base)
        || !is_canonical(idtr.base.as_u64())
    {
        hvlogf(format_args!(
            "hv: vm1 reporting: host-state invalid bases tr=0x{:016X} fs=0x{:016X} gs=0x{:016X} gdtr=0x{:016X} idtr=0x{:016X}",
            tr_base,
            fs_base,
            gs_base,
            host_gdtr_base,
            idtr.base.as_u64()
        ));
        return Err("host bases");
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: host-state cs=0x{:04X} ss=0x{:04X} tr=0x{:04X} tr_base=0x{:016X}",
        host_cs as u16, host_ss as u16, host_tr as u16, tr_base
    ));

    let (host_cr3, _) = Cr3::read();
    vmwrite(VMCS_HOST_CR0, host_cr0)?;
    vmwrite(VMCS_HOST_CR3, host_cr3.start_address().as_u64())?;
    vmwrite(VMCS_HOST_CR4, host_cr4)?;
    vmwrite(VMCS_HOST_CS_SELECTOR, host_cs)?;
    vmwrite(VMCS_HOST_SS_SELECTOR, host_ss)?;
    vmwrite(VMCS_HOST_DS_SELECTOR, host_ds)?;
    vmwrite(VMCS_HOST_ES_SELECTOR, host_es)?;
    vmwrite(VMCS_HOST_FS_SELECTOR, host_fs)?;
    vmwrite(VMCS_HOST_GS_SELECTOR, host_gs)?;
    vmwrite(VMCS_HOST_TR_SELECTOR, host_tr)?;
    vmwrite(VMCS_HOST_FS_BASE, fs_base)?;
    vmwrite(VMCS_HOST_GS_BASE, gs_base)?;
    vmwrite(VMCS_HOST_TR_BASE, tr_base)?;
    vmwrite(VMCS_HOST_GDTR_BASE, host_gdtr_base)?;
    vmwrite(VMCS_HOST_IDTR_BASE, idtr.base.as_u64())?;
    vmwrite(VMCS_HOST_SYSENTER_CS, host_sysenter_cs)?;
    vmwrite(VMCS_HOST_SYSENTER_ESP, sysenter_esp)?;
    vmwrite(VMCS_HOST_SYSENTER_EIP, sysenter_eip)?;
    vmwrite(VMCS_HOST_IA32_PAT, pat)?;
    vmwrite(VMCS_HOST_IA32_EFER, efer)?;
    vmwrite(VMCS_HOST_IA32_PERF_GLOBAL_CTRL, perf_global)?;

    let restored = active_restore_meta();
    let guest_rip = restored.map(|m| m.guest_rip).unwrap_or_else(guest_launch_rip);
    let guest_rsp = restored.map(|m| m.guest_rsp).unwrap_or_else(guest_stack_top);
    let guest_cr3 = if restored.is_some() {
        current_guest_cr3_pa()?
    } else {
        build_guest_cr3(guest_rip, guest_rsp)?
    };
    vmwrite(VMCS_GUEST_CR0, host_cr0)?;
    vmwrite(VMCS_GUEST_CR3, guest_cr3)?;
    vmwrite(VMCS_GUEST_CR4, host_cr4)?;
    vmwrite(VMCS_GUEST_RFLAGS, (guest_rflags | RFLAGS_RESERVED_BIT1) & !RFLAGS_IF)?;
    vmwrite(VMCS_GUEST_RIP, guest_rip)?;
    vmwrite(VMCS_GUEST_RSP, guest_rsp)?;
    vmwrite(VMCS_GUEST_DR7, 0x400)?;
    vmwrite(VMCS_GUEST_IA32_DEBUGCTL, 0)?;
    vmwrite(VMCS_GUEST_SYSENTER_CS, sysenter_cs)?;
    vmwrite(VMCS_GUEST_SYSENTER_ESP, sysenter_esp)?;
    vmwrite(VMCS_GUEST_SYSENTER_EIP, sysenter_eip)?;
    vmwrite(VMCS_GUEST_IA32_PAT, pat)?;
    vmwrite(VMCS_GUEST_IA32_EFER, efer)?;
    vmwrite(VMCS_GUEST_IA32_PERF_GLOBAL_CTRL, perf_global)?;
    vmwrite(VMCS_GUEST_ACTIVITY_STATE, 0)?;
    vmwrite(VMCS_GUEST_INTERRUPTIBILITY, 0)?;
    vmwrite(VMCS_GUEST_PENDING_DBG, 0)?;
    vmwrite(VMCS_GUEST_VMCS_PREEMPT_TIMER, 0)?;

    // Flat segment model in long mode.
    let cs = CS::get_reg().0 as u64;
    let ss = SS::get_reg().0 as u64;
    let ds = DS::get_reg().0 as u64;
    let es = ES::get_reg().0 as u64;
    let fs = FS::get_reg().0 as u64;
    let gs = GS::get_reg().0 as u64;
    let tr = tr_sel as u64;
    vmwrite(VMCS_GUEST_CS_SELECTOR, cs)?;
    vmwrite(VMCS_GUEST_SS_SELECTOR, ss)?;
    vmwrite(VMCS_GUEST_DS_SELECTOR, ds)?;
    vmwrite(VMCS_GUEST_ES_SELECTOR, es)?;
    vmwrite(VMCS_GUEST_FS_SELECTOR, fs)?;
    vmwrite(VMCS_GUEST_GS_SELECTOR, gs)?;
    vmwrite(VMCS_GUEST_TR_SELECTOR, tr)?;
    vmwrite(VMCS_GUEST_LDTR_SELECTOR, 0)?;

    vmwrite(VMCS_GUEST_CS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_SS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_DS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_ES_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_FS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_GS_LIMIT, 0xFFFF_FFFF)?;
    vmwrite(VMCS_GUEST_TR_LIMIT, 0xFFFF)?;
    vmwrite(VMCS_GUEST_LDTR_LIMIT, 0)?;
    vmwrite(VMCS_GUEST_GDTR_LIMIT, gdtr.limit as u64)?;
    vmwrite(VMCS_GUEST_IDTR_LIMIT, idtr.limit as u64)?;

    vmwrite(VMCS_GUEST_CS_BASE, 0)?;
    vmwrite(VMCS_GUEST_SS_BASE, 0)?;
    vmwrite(VMCS_GUEST_DS_BASE, 0)?;
    vmwrite(VMCS_GUEST_ES_BASE, 0)?;
    vmwrite(VMCS_GUEST_FS_BASE, fs_base)?;
    vmwrite(VMCS_GUEST_GS_BASE, gs_base)?;
    vmwrite(VMCS_GUEST_TR_BASE, tr_base)?;
    vmwrite(VMCS_GUEST_LDTR_BASE, 0)?;
    vmwrite(VMCS_GUEST_GDTR_BASE, gdtr.base.as_u64())?;
    vmwrite(VMCS_GUEST_IDTR_BASE, idtr.base.as_u64())?;

    vmwrite(VMCS_GUEST_CS_AR, 0xA09B)?;
    vmwrite(VMCS_GUEST_SS_AR, if ss == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_DS_AR, if ds == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_ES_AR, if es == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_FS_AR, if fs == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_GS_AR, if gs == 0 { 0x10000 } else { 0xC093 })?;
    vmwrite(VMCS_GUEST_TR_AR, 0x008B)?;
    vmwrite(VMCS_GUEST_LDTR_AR, 0x10000)?;

    Ok(())
}

fn build_ept_identity_4g() -> Result<u64, &'static str> {
    let pml4 = unsafe { core::ptr::addr_of_mut!(EPT_PML4.0) };
    let pdpt = unsafe { core::ptr::addr_of_mut!(EPT_PDPT.0) };
    unsafe {
        core::ptr::write_bytes(pml4 as *mut u8, 0, VMX_PAGE_SIZE);
        core::ptr::write_bytes(pdpt as *mut u8, 0, VMX_PAGE_SIZE);
    }
    for i in 0..EPT_PDPT_ENTRIES {
        let pd = unsafe { core::ptr::addr_of_mut!(EPT_PD[i].0) };
        unsafe { core::ptr::write_bytes(pd as *mut u8, 0, VMX_PAGE_SIZE) };
    }

    let pml4_pa = kernel_va_to_pa(pml4 as u64).ok_or("ept pml4 pa")?;
    let pdpt_pa = kernel_va_to_pa(pdpt as u64).ok_or("ept pdpt pa")?;
    unsafe {
        (*pml4)[0] = (pdpt_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
    }

    for i in 0..EPT_PDPT_ENTRIES {
        let pd = unsafe { core::ptr::addr_of!(EPT_PD[i].0) };
        let pd_pa = kernel_va_to_pa(pd as u64).ok_or("ept pd pa")?;
        unsafe {
            (*pdpt)[i] = (pd_pa & 0x000F_FFFF_FFFF_F000) | 0x7;
        }
        for j in 0..EPT_PD_ENTRIES {
            let gpa = ((i as u64) << 30) | ((j as u64) << 21);
            let pde = (gpa & 0x000F_FFFF_FFE0_0000) | 0x7 | (1 << 7) | (6 << 3);
            unsafe {
                (*core::ptr::addr_of_mut!(EPT_PD[i].0))[j] = pde;
            }
        }
    }

    let eptp = (pml4_pa & 0x000F_FFFF_FFFF_F000) | 6 | (3 << 3);
    hvlogf(format_args!(
        "hv: vm1 reporting: ept v1 identity map ready eptp=0x{:016X}",
        eptp
    ));
    Ok(eptp)
}

fn guest_launch_rip() -> u64 {
    guest::entry as *const () as usize as u64
}

fn guest_stack_top() -> u64 {
    (GUEST_STACK_VA_BASE + GUEST_STACK_BYTES as u64) & !0xF
}

fn guest_kernel_elf_entry(bytes: &[u8]) -> Option<u64> {
    if bytes.len() < ELF64_HEADER_LEN {
        return None;
    }
    if bytes.get(0..4) != Some(b"\x7fELF") {
        return None;
    }
    if bytes.get(4).copied()? != 2 || bytes.get(5).copied()? != 1 {
        return None;
    }
    let raw: [u8; 8] = bytes.get(24..32)?.try_into().ok()?;
    Some(u64::from_le_bytes(raw))
}

fn adjust_vmx_ctrl(msr: u32, desired: u64) -> u64 {
    let caps = unsafe { Msr::new(msr).read() };
    let must_be_1 = caps & 0xFFFF_FFFF;
    let may_be_1 = (caps >> 32) & 0xFFFF_FFFF;
    // Intel SDM (VMX control MSRs): ctl = (desired | must_be_1) & may_be_1.
    ((desired & 0xFFFF_FFFF) | must_be_1) & may_be_1
}

fn build_guest_cr3(guest_rip: u64, guest_rsp: u64) -> Result<u64, &'static str> {
    unsafe {
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_PML4.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PD.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_STACK_PT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0));
        zero_guest_page(core::ptr::addr_of_mut!(GUEST_CODE_PT.0));

        let pml4_pa = kernel_va_to_pa(core::ptr::addr_of!(GUEST_PML4.0) as u64).ok_or("guest pml4 pa")?;
        let low_pdpt_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PDPT.0) as u64).ok_or("guest low pdpt pa")?;
        let low_pd_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_LOW_PD.0) as u64).ok_or("guest low pd pa")?;
        let stack_pt_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_STACK_PT.0) as u64).ok_or("guest stack pt pa")?;
        let high_pdpt_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_HIGH_PDPT.0) as u64).ok_or("guest high pdpt pa")?;
        let high_pd_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_HIGH_PD.0) as u64).ok_or("guest high pd pa")?;
        let code_pt_pa =
            kernel_va_to_pa(core::ptr::addr_of!(GUEST_CODE_PT.0) as u64).ok_or("guest code pt pa")?;

        map_table_entry(core::ptr::addr_of_mut!(GUEST_PML4.0), pml4_index(GUEST_STACK_VA_BASE), low_pdpt_pa);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0), pdpt_index(GUEST_STACK_VA_BASE), low_pd_pa);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_LOW_PD.0), pd_index(GUEST_STACK_VA_BASE), stack_pt_pa);
        map_region_4k(
            core::ptr::addr_of_mut!(GUEST_STACK_PT.0),
            page_align_down(GUEST_STACK_VA_BASE),
            kernel_va_to_pa(core::ptr::addr_of!(VM1_GUEST_STACK.0) as u64).ok_or("guest stack pa")?,
            GUEST_STACK_BYTES,
            PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE,
        )?;

        let code_base = page_align_down(guest_rip);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_PML4.0), pml4_index(code_base), high_pdpt_pa);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0), pdpt_index(code_base), high_pd_pa);
        map_table_entry(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0), pd_index(code_base), code_pt_pa);
        map_region_4k(
            core::ptr::addr_of_mut!(GUEST_CODE_PT.0),
            code_base,
            kernel_va_to_pa(code_base).ok_or("guest code pa")?,
            GUEST_CODE_WINDOW_BYTES,
            PT_ENTRY_PRESENT,
        )?;

        hvlogf(format_args!(
            "hv: vm1 reporting: guest-cr3=0x{:016X} code=0x{:016X} stack=0x{:016X}",
            pml4_pa, guest_rip, guest_rsp
        ));
        *VM1_SNAPSHOT_META.lock() = Some(Vm1SnapshotMeta {
            guest_cr3: pml4_pa,
            guest_rip,
            guest_rsp,
            code_base,
            code_len: GUEST_CODE_WINDOW_BYTES as u64,
            exit_reason: 0,
            exit_qualification: 0,
            exit_guest_rip: guest_rip,
        });
        Ok(pml4_pa)
    }
}

fn active_restore_meta() -> Option<Vm1SnapshotMeta> {
    *VM1_RESTORE_META.lock()
}

fn current_guest_cr3_pa() -> Result<u64, &'static str> {
    kernel_va_to_pa(unsafe { core::ptr::addr_of!(GUEST_PML4.0) as u64 }).ok_or("guest pml4 pa")
}

unsafe fn zero_guest_page(page: *mut [u64; 512]) {
    core::ptr::write_bytes(page as *mut u8, 0, PAGE_SIZE_4K);
}

fn map_table_entry(table: *mut [u64; 512], index: usize, next_pa: u64) {
    unsafe {
        (*table)[index] = (next_pa & 0x000F_FFFF_FFFF_F000) | PT_ENTRY_PRESENT | PT_ENTRY_WRITABLE;
    }
}

fn map_region_4k(
    pt: *mut [u64; 512],
    virt_base: u64,
    phys_base: u64,
    bytes: usize,
    flags: u64,
) -> Result<(), &'static str> {
    let pages = bytes.div_ceil(PAGE_SIZE_4K);
    let first_pt = pt_index(virt_base);
    if first_pt + pages > 512 {
        return Err("guest pt range");
    }
    for page in 0..pages {
        let phys = phys_base
            .checked_add((page * PAGE_SIZE_4K) as u64)
            .ok_or("guest phys overflow")?;
        unsafe {
            (*pt)[first_pt + page] = (phys & 0x000F_FFFF_FFFF_F000) | flags;
        }
    }
    Ok(())
}

fn page_align_down(addr: u64) -> u64 {
    addr & !((PAGE_SIZE_4K as u64) - 1)
}

fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1FF) as usize
}

fn pdpt_index(addr: u64) -> usize {
    ((addr >> 30) & 0x1FF) as usize
}

fn pd_index(addr: u64) -> usize {
    ((addr >> 21) & 0x1FF) as usize
}

fn pt_index(addr: u64) -> usize {
    ((addr >> 12) & 0x1FF) as usize
}

fn capture_snapshot_meta(lr: LaunchResult) {
    let mut meta = VM1_SNAPSHOT_META.lock();
    if let Some(mut m) = *meta {
        m.exit_reason = lr.exit_reason;
        m.exit_qualification = lr.exit_qualification;
        m.exit_guest_rip = lr.guest_rip;
        *meta = Some(m);
    }
}

fn restore_snapshot_bytes(bytes: &[u8]) -> Result<(), RestoreError> {
    let header_len = core::mem::size_of::<Vm1SnapshotHeader>();
    if bytes.len() < header_len {
        return Err(RestoreError::BadSnapshot);
    }

    let header = parse_snapshot_header(&bytes[..header_len])?;
    let expected = header_len + (7 * PAGE_SIZE_4K) + (header.guest_stack_bytes as usize) + (header.code_len as usize);
    if bytes.len() < expected || header.guest_stack_bytes as usize != GUEST_STACK_BYTES || header.guest_page_bytes as usize != PAGE_SIZE_4K {
        return Err(RestoreError::BadSnapshot);
    }

    let mut off = header_len;
    unsafe {
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_PML4.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PDPT.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_LOW_PD.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_STACK_PT.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PDPT.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_HIGH_PD.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;
        copy_into_guest_page(core::ptr::addr_of_mut!(GUEST_CODE_PT.0), &bytes[off..off + PAGE_SIZE_4K]);
        off += PAGE_SIZE_4K;

        core::ptr::copy_nonoverlapping(
            bytes[off..off + GUEST_STACK_BYTES].as_ptr(),
            core::ptr::addr_of_mut!(VM1_GUEST_STACK.0).cast::<u8>(),
            GUEST_STACK_BYTES,
        );
        off += GUEST_STACK_BYTES;
    }

    let code_end = off + header.code_len as usize;
    let live_code = unsafe { core::slice::from_raw_parts(header.code_base as *const u8, header.code_len as usize) };
    if live_code != &bytes[off..code_end] {
        return Err(RestoreError::CodeMismatch);
    }

    let guest_cr3 = current_guest_cr3_pa().map_err(|_| RestoreError::BadSnapshot)?;
    let restored = Vm1SnapshotMeta {
        guest_cr3,
        guest_rip: header.guest_rip,
        guest_rsp: header.guest_rsp,
        code_base: header.code_base,
        code_len: header.code_len,
        exit_reason: header.exit_reason,
        exit_qualification: header.exit_qualification,
        exit_guest_rip: header.exit_guest_rip,
    };
    *VM1_SNAPSHOT_META.lock() = Some(restored);
    *VM1_RESTORE_META.lock() = Some(restored);
    hvlogf(format_args!(
        "hv: vm1 reporting: restore armed guest_cr3=0x{:016X} guest_rip=0x{:016X} guest_rsp=0x{:016X}",
        restored.guest_cr3, restored.guest_rip, restored.guest_rsp
    ));
    Ok(())
}

fn parse_snapshot_header(bytes: &[u8]) -> Result<Vm1SnapshotHeader, RestoreError> {
    let mut off = 0usize;
    let magic = take_u32(bytes, &mut off)?;
    let version = take_u32(bytes, &mut off)?;
    let guest_cr3 = take_u64(bytes, &mut off)?;
    let guest_rip = take_u64(bytes, &mut off)?;
    let guest_rsp = take_u64(bytes, &mut off)?;
    let code_base = take_u64(bytes, &mut off)?;
    let code_len = take_u64(bytes, &mut off)?;
    let exit_reason = take_u64(bytes, &mut off)?;
    let exit_qualification = take_u64(bytes, &mut off)?;
    let exit_guest_rip = take_u64(bytes, &mut off)?;
    let guest_stack_bytes = take_u64(bytes, &mut off)?;
    let guest_page_bytes = take_u64(bytes, &mut off)?;
    if magic != VM1_SNAPSHOT_MAGIC || version != VM1_SNAPSHOT_VERSION {
        return Err(RestoreError::BadSnapshot);
    }
    Ok(Vm1SnapshotHeader {
        magic,
        version,
        guest_cr3,
        guest_rip,
        guest_rsp,
        code_base,
        code_len,
        exit_reason,
        exit_qualification,
        exit_guest_rip,
        guest_stack_bytes,
        guest_page_bytes,
    })
}

fn take_u32(bytes: &[u8], off: &mut usize) -> Result<u32, RestoreError> {
    let end = off.checked_add(4).ok_or(RestoreError::BadSnapshot)?;
    let raw: [u8; 4] = bytes.get(*off..end).ok_or(RestoreError::BadSnapshot)?.try_into().map_err(|_| RestoreError::BadSnapshot)?;
    *off = end;
    Ok(u32::from_le_bytes(raw))
}

fn take_u64(bytes: &[u8], off: &mut usize) -> Result<u64, RestoreError> {
    let end = off.checked_add(8).ok_or(RestoreError::BadSnapshot)?;
    let raw: [u8; 8] = bytes.get(*off..end).ok_or(RestoreError::BadSnapshot)?.try_into().map_err(|_| RestoreError::BadSnapshot)?;
    *off = end;
    Ok(u64::from_le_bytes(raw))
}

unsafe fn copy_into_guest_page(dst: *mut [u64; 512], src: &[u8]) {
    core::ptr::copy_nonoverlapping(src.as_ptr(), dst.cast::<u8>(), PAGE_SIZE_4K);
}

fn snapshot_bytes() -> Result<Vec<u8>, SaveError> {
    let Some(meta) = *VM1_SNAPSHOT_META.lock() else {
        return Err(SaveError::NoSnapshot);
    };

    let header = Vm1SnapshotHeader {
        magic: VM1_SNAPSHOT_MAGIC,
        version: VM1_SNAPSHOT_VERSION,
        guest_cr3: meta.guest_cr3,
        guest_rip: meta.guest_rip,
        guest_rsp: meta.guest_rsp,
        code_base: meta.code_base,
        code_len: meta.code_len,
        exit_reason: meta.exit_reason,
        exit_qualification: meta.exit_qualification,
        exit_guest_rip: meta.exit_guest_rip,
        guest_stack_bytes: GUEST_STACK_BYTES as u64,
        guest_page_bytes: PAGE_SIZE_4K as u64,
    };

    let total = core::mem::size_of::<Vm1SnapshotHeader>()
        + (7 * PAGE_SIZE_4K)
        + GUEST_STACK_BYTES
        + GUEST_CODE_WINDOW_BYTES;
    let mut out = Vec::with_capacity(total);
    push_bytes(
        &mut out,
        unsafe {
            core::slice::from_raw_parts(
                (&header as *const Vm1SnapshotHeader).cast::<u8>(),
                core::mem::size_of::<Vm1SnapshotHeader>(),
            )
        },
    );
    unsafe {
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_PML4.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_LOW_PDPT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_LOW_PD.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_STACK_PT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_HIGH_PDPT.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_HIGH_PD.0));
        push_guest_page(&mut out, core::ptr::addr_of!(GUEST_CODE_PT.0));
        push_bytes(
            &mut out,
            core::slice::from_raw_parts(
                core::ptr::addr_of!(VM1_GUEST_STACK.0).cast::<u8>(),
                GUEST_STACK_BYTES,
            ),
        );
        push_bytes(
            &mut out,
            core::slice::from_raw_parts(meta.code_base as *const u8, GUEST_CODE_WINDOW_BYTES),
        );
    }
    Ok(out)
}

fn push_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(bytes);
}

unsafe fn push_guest_page(out: &mut Vec<u8>, page: *const [u64; 512]) {
    push_bytes(out, core::slice::from_raw_parts(page.cast::<u8>(), PAGE_SIZE_4K));
}

fn vmwrite(field: u64, val: u64) -> Result<(), &'static str> {
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmwrite {field}, {val}",
            "setna {fail}",
            val = in(reg) val,
            field = in(reg) field,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        if fail == 0 {
            Ok(())
        } else {
            let err = vmread(VMCS_VM_INSTRUCTION_ERROR).unwrap_or(!0u64);
            hvlogf(format_args!(
                "hv: vm1 reporting: vmwrite failed field=0x{:X} val=0x{:016X} instr_err={} rip=0x{:016X}",
                field,
                val,
                err,
                current_rip()
            ));
            Err("vmwrite")
        }
    }
}

fn vmread(field: u64) -> Option<u64> {
    unsafe {
        let mut out: u64 = 0;
        let mut fail: u8;
        core::arch::asm!(
            "vmread {out}, {field}",
            "setna {fail}",
            field = in(reg) field,
            out = lateout(reg) out,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        if fail == 0 { Some(out) } else { None }
    }
}

struct HvSyntheticHostState {
    gdt_base: u64,
    tr_base: u64,
    tr_sel: u16,
    cs_sel: u16,
    data_sel: u16,
}

fn synthesize_host_gdt_tss() -> HvSyntheticHostState {
    let gdt = core::ptr::addr_of_mut!(HV_HOST_GDT);
    let tss = core::ptr::addr_of_mut!(HV_HOST_TSS);
    let tss_base = tss as u64;
    let tss_limit = (core::mem::size_of::<[u8; 104]>() as u64) - 1;

    // 64-bit busy TSS descriptor (type 0xB), split across two 8-byte slots.
    let tss_low = (tss_limit & 0xFFFF)
        | ((tss_base & 0xFFFF) << 16)
        | (((tss_base >> 16) & 0xFF) << 32)
        | (0xBu64 << 40)
        | (1u64 << 47)
        | (((tss_limit >> 16) & 0xF) << 48)
        | (((tss_base >> 24) & 0xFF) << 56);
    let tss_high = (tss_base >> 32) & 0xFFFF_FFFF;

    unsafe {
        (*gdt)[0] = 0;
        (*gdt)[1] = HV_GDT_DESC_CODE64;
        (*gdt)[2] = HV_GDT_DESC_DATA64;
        (*gdt)[3] = tss_low;
        (*gdt)[4] = tss_high;
    }

    HvSyntheticHostState {
        gdt_base: gdt as u64,
        tr_base: tss_base,
        tr_sel: HV_GDT_SEL_TSS,
        cs_sel: HV_GDT_SEL_CODE,
        data_sel: HV_GDT_SEL_DATA,
    }
}

fn read_tr_selector() -> u16 {
    let mut tr: u16 = 0;
    unsafe {
        core::arch::asm!(
            "str {0:x}",
            out(reg) tr,
            options(nostack, preserves_flags),
        );
    }
    tr
}

fn load_tr_selector(sel: u16) {
    unsafe {
        core::arch::asm!(
            "ltr {sel:x}",
            sel = in(reg) sel,
            options(nostack),
        );
    }
}

fn find_tss_selector(gdt_base: u64, gdt_limit: u16) -> Option<(u16, u8)> {
    let bytes = (gdt_limit as usize).checked_add(1)?;
    let entries = bytes / 8;
    if entries < 2 {
        return None;
    }

    for i in 1..entries {
        let ptr = (gdt_base as usize).checked_add(i.checked_mul(8)?)? as *const u64;
        let low = unsafe { core::ptr::read_unaligned(ptr) };
        let present = ((low >> 47) & 1) != 0;
        let system = ((low >> 44) & 1) == 0;
        let ty = ((low >> 40) & 0xF) as u8;
        // 64-bit TSS descriptor types: 0x9 (available), 0xB (busy).
        if present && system && (ty == 0x9 || ty == 0xB) {
            return Some(((i as u16) << 3, ty));
        }
    }
    None
}

fn read_rsp() -> u64 {
    let rsp: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, rsp",
            out(reg) rsp,
            options(nostack, preserves_flags),
        );
    }
    rsp
}

fn is_canonical(addr: u64) -> bool {
    let high = addr >> 48;
    high == 0 || high == 0xFFFF
}

fn tss_base_from_gdt(gdt_base: u64, tr_sel: u16) -> Option<u64> {
    let index = (tr_sel as usize) & !0x7;
    let ptr = (gdt_base as usize).checked_add(index)? as *const u8;
    let low = unsafe { core::ptr::read_unaligned(ptr as *const u64) };
    let high = unsafe { core::ptr::read_unaligned(ptr.add(8) as *const u64) };
    let base_low = (low >> 16) & 0xFF_FFFF;
    let base_mid2 = (low >> 56) & 0xFF;
    let base_high = high & 0xFFFF_FFFF;
    Some(base_low | (base_mid2 << 24) | (base_high << 32))
}

fn vmlaunch_once_wrapper(out: &mut LaunchResult) {
    unsafe {
        VMX_WRAPPER_RESULT = LaunchResult::default();
        core::arch::asm!(
            // Preserve the host callee-saved GPR set across guest execution.
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            // Program host return state late so HOST_RIP can use a local label.
            "mov rax, rsp",
            "mov rcx, {host_rsp_field}",
            "vmwrite rcx, rax",
            "lea rax, [rip + 2f]",
            "mov rcx, {host_rip_field}",
            "vmwrite rcx, rax",

            "vmlaunch",
            "setna al",
            "lea r11, [rip + {result_base}]",
            "mov byte ptr [r11 + {launch_failed_off}], al",
            "cmp al, 0",
            "je 4f",
            // VM-instruction failure path.
            "mov rcx, {vm_instr_err_field}",
            "vmread rax, rcx",
            "mov [r11 + {instr_err_off}], rax",
            "jmp 3f",

            "4:",
            "jmp 3f",

            // VM-exit landing path.
            "2:",
            "lea r11, [rip + {result_base}]",
            "mov byte ptr [r11 + {entered_off}], 1",
            "mov rcx, {exit_reason_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_reason_off}], rax",
            "mov rcx, {exit_qual_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_qual_off}], rax",
            "mov rcx, {guest_rip_field}",
            "vmread rax, rcx",
            "mov [r11 + {guest_rip_off}], rax",
            "3:",
            "cld",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            host_rsp_field = const VMCS_HOST_RSP,
            host_rip_field = const VMCS_HOST_RIP,
            vm_instr_err_field = const VMCS_VM_INSTRUCTION_ERROR,
            exit_reason_field = const VMCS_EXIT_REASON,
            exit_qual_field = const VMCS_EXIT_QUALIFICATION,
            guest_rip_field = const VMCS_VMEXIT_GUEST_RIP,
            result_base = sym VMX_WRAPPER_RESULT,
            entered_off = const core::mem::offset_of!(LaunchResult, entered),
            launch_failed_off = const core::mem::offset_of!(LaunchResult, launch_failed),
            exit_reason_off = const core::mem::offset_of!(LaunchResult, exit_reason),
            exit_qual_off = const core::mem::offset_of!(LaunchResult, exit_qualification),
            guest_rip_off = const core::mem::offset_of!(LaunchResult, guest_rip),
            instr_err_off = const core::mem::offset_of!(LaunchResult, instr_err),
            out("rax") _,
            out("rcx") _,
            out("r11") _,
            clobber_abi("sysv64"),
        );
        *out = VMX_WRAPPER_RESULT;
    }
}

fn vmresume_once_wrapper(out: &mut LaunchResult) {
    unsafe {
        VMX_WRAPPER_RESULT = LaunchResult::default();
        core::arch::asm!(
            "push rbx",
            "push rbp",
            "push r12",
            "push r13",
            "push r14",
            "push r15",
            // Refresh host return state so VM-exit lands in this wrapper.
            "mov rax, rsp",
            "mov rcx, {host_rsp_field}",
            "vmwrite rcx, rax",
            "lea rax, [rip + 2f]",
            "mov rcx, {host_rip_field}",
            "vmwrite rcx, rax",

            "vmresume",
            "setna al",
            "lea r11, [rip + {result_base}]",
            "mov byte ptr [r11 + {launch_failed_off}], al",
            "cmp al, 0",
            "je 4f",
            "mov rcx, {vm_instr_err_field}",
            "vmread rax, rcx",
            "mov [r11 + {instr_err_off}], rax",
            "jmp 3f",

            "4:",
            "jmp 3f",

            "2:",
            "lea r11, [rip + {result_base}]",
            "mov byte ptr [r11 + {entered_off}], 1",
            "mov rcx, {exit_reason_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_reason_off}], rax",
            "mov rcx, {exit_qual_field}",
            "vmread rax, rcx",
            "mov [r11 + {exit_qual_off}], rax",
            "mov rcx, {guest_rip_field}",
            "vmread rax, rcx",
            "mov [r11 + {guest_rip_off}], rax",
            "3:",
            "cld",
            "pop r15",
            "pop r14",
            "pop r13",
            "pop r12",
            "pop rbp",
            "pop rbx",
            host_rsp_field = const VMCS_HOST_RSP,
            host_rip_field = const VMCS_HOST_RIP,
            vm_instr_err_field = const VMCS_VM_INSTRUCTION_ERROR,
            exit_reason_field = const VMCS_EXIT_REASON,
            exit_qual_field = const VMCS_EXIT_QUALIFICATION,
            guest_rip_field = const VMCS_VMEXIT_GUEST_RIP,
            result_base = sym VMX_WRAPPER_RESULT,
            entered_off = const core::mem::offset_of!(LaunchResult, entered),
            launch_failed_off = const core::mem::offset_of!(LaunchResult, launch_failed),
            exit_reason_off = const core::mem::offset_of!(LaunchResult, exit_reason),
            exit_qual_off = const core::mem::offset_of!(LaunchResult, exit_qualification),
            guest_rip_off = const core::mem::offset_of!(LaunchResult, guest_rip),
            instr_err_off = const core::mem::offset_of!(LaunchResult, instr_err),
            out("rax") _,
            out("rcx") _,
            out("r11") _,
            clobber_abi("sysv64"),
        );
        *out = VMX_WRAPPER_RESULT;
    }
}

fn handle_hlt_exit_resume_once() -> Result<(LaunchResult, u64, u64, u64), &'static str> {
    let rip_before = vmread(VMCS_VMEXIT_GUEST_RIP).ok_or("vmread guest_rip")?;
    let exit_len = vmread(VMCS_VMEXIT_INSTRUCTION_LEN).ok_or("vmread exit_len")?;
    let rip_after = rip_before.wrapping_add(exit_len);
    vmwrite(VMCS_GUEST_RIP, rip_after)?;
    let mut lr = LaunchResult::default();
    vmresume_once_wrapper(&mut lr);
    Ok((lr, rip_before, exit_len, rip_after))
}

fn vmx_smoke() -> Result<(), &'static str> {
    let caps = status();
    if !caps.vendor_intel || !caps.has_msr || !caps.has_vmx {
        return Err("vmx unsupported");
    }

    let cr0_fixed0 = unsafe { Msr::new(IA32_VMX_CR0_FIXED0).read() };
    let cr0_fixed1 = unsafe { Msr::new(IA32_VMX_CR0_FIXED1).read() };
    let cr4_fixed0 = unsafe { Msr::new(IA32_VMX_CR4_FIXED0).read() };
    let cr4_fixed1 = unsafe { Msr::new(IA32_VMX_CR4_FIXED1).read() };

    let mut cr0 = Cr0::read().bits();
    let mut cr4 = Cr4::read().bits();
    cr0 = (cr0 | cr0_fixed0) & cr0_fixed1;
    cr4 = (cr4 | cr4_fixed0) & cr4_fixed1;
    cr4 |= Cr4Flags::VIRTUAL_MACHINE_EXTENSIONS.bits();
    unsafe {
        Cr0::write(Cr0Flags::from_bits_truncate(cr0));
        Cr4::write(Cr4Flags::from_bits_truncate(cr4));
    }

    let basic = unsafe { Msr::new(IA32_VMX_BASIC).read() };
    let revision = (basic & 0x7fff_ffff) as u32;

    let vmxon_va = unsafe { core::ptr::addr_of_mut!(VMXON_REGION.0) } as *mut u8;
    let vmcs_va = unsafe { core::ptr::addr_of_mut!(VMCS_REGION.0) } as *mut u8;
    unsafe {
        core::ptr::write_bytes(vmxon_va, 0, VMX_PAGE_SIZE);
        core::ptr::write_bytes(vmcs_va, 0, VMX_PAGE_SIZE);
        *(vmxon_va as *mut u32) = revision;
        *(vmcs_va as *mut u32) = revision;
    }

    let vmxon_pa = kernel_va_to_pa(vmxon_va as u64).ok_or("vmxon pa")?;
    let vmcs_pa = kernel_va_to_pa(vmcs_va as u64).ok_or("vmcs pa")?;
    hvlogf(format_args!(
        "hv: vm1 reporting: vmx setup revision=0x{:08X} vmxon_pa=0x{:016X} vmcs_pa=0x{:016X}",
        revision, vmxon_pa, vmcs_pa
    ));

    let rip = current_rip();
    if !vmxon(vmxon_pa) {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmxon failed rip=0x{:016X} pa=0x{:016X}",
            rip, vmxon_pa
        ));
        return Err("vmxon");
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: vmxon ok rip=0x{:016X}",
        rip
    ));

    let rip = current_rip();
    if !vmclear(vmcs_pa) {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmclear failed rip=0x{:016X} pa=0x{:016X}",
            rip, vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmclear");
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: vmclear ok rip=0x{:016X}",
        rip
    ));

    let rip = current_rip();
    if !vmptrld(vmcs_pa) {
        hvlogf(format_args!(
            "hv: vm1 reporting: vmptrld failed rip=0x{:016X} pa=0x{:016X}",
            rip, vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmptrld");
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: vmptrld ok rip=0x{:016X}",
        rip
    ));

    let ptr = match vmptrst() {
        Some(v) => v,
        None => {
            let rip = current_rip();
            hvlogf(format_args!(
                "hv: vm1 reporting: vmptrst failed rip=0x{:016X}",
                rip
            ));
            let _ = vmxoff();
            return Err("vmptrst");
        }
    };
    if ptr != vmcs_pa {
        let rip = current_rip();
        hvlogf(format_args!(
            "hv: vm1 reporting: vmptrst mismatch rip=0x{:016X} got=0x{:016X} want=0x{:016X}",
            rip, ptr, vmcs_pa
        ));
        let _ = vmxoff();
        return Err("vmptrst mismatch");
    }
    hvlogf(format_args!(
        "hv: vm1 reporting: vmptrst ok current_vmcs=0x{:016X}",
        ptr
    ));

    if !vmxoff() {
        let rip = current_rip();
        hvlogf(format_args!(
            "hv: vm1 reporting: vmxoff failed rip=0x{:016X}",
            rip
        ));
        return Err("vmxoff");
    }
    hvlogf(format_args!("hv: vm1 reporting: vmxoff ok"));
    Ok(())
}

fn kernel_va_to_pa(va: u64) -> Option<u64> {
    let (virt_base, phys_base) = crate::limine::executable_address_bases()?;
    let offset = va.checked_sub(virt_base)?;
    phys_base.checked_add(offset)
}

fn vmxon(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmxon [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

fn vmclear(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmclear [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

fn vmptrld(pa: u64) -> bool {
    let pa_ptr = pa;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmptrld [{pa}]",
            "setna {fail}",
            pa = in(reg) &pa_ptr,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

fn vmptrst() -> Option<u64> {
    let mut out: u64 = !0u64;
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmptrst [{out}]",
            "setna {fail}",
            out = in(reg) &mut out,
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        if fail == 0 { Some(out) } else { None }
    }
}

fn vmxoff() -> bool {
    unsafe {
        let mut fail: u8;
        core::arch::asm!(
            "vmxoff",
            "setna {fail}",
            fail = lateout(reg_byte) fail,
            options(nostack, preserves_flags),
        );
        fail == 0
    }
}

fn current_rip() -> u64 {
    let rip: u64;
    unsafe {
        core::arch::asm!(
            "lea {}, [rip + 0]",
            out(reg) rip,
            options(nomem, nostack, preserves_flags),
        );
    }
    rip
}

fn hvlogf(args: core::fmt::Arguments<'_>) {
    let mut line: String<HV_LOG_LINE> = String::new();
    let _ = line.write_fmt(args);
    if line.is_empty() {
        return;
    }

    let seq = HV_LOG_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    {
        let mut ring = HV_LOG_RING.lock();
        if ring.is_full() {
            let _ = ring.pop_front();
        }
        let _ = ring.push_back(HvLogEntry {
            seq,
            msg: line.clone(),
        });
    }

    crate::log!("{}\n", line.as_str());
}
