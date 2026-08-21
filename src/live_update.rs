//! RAM-only TRUEOS generation replacement (`update live`).
//!
//! The old kernel is treated as a disposable in-memory boot loader:
//! 1. validate and stage a compatible TRUEOS ELF in a PMM-reserved arena;
//! 2. checkpoint active replicatable VMX applications to TRUEOSFS;
//! 3. park every AP through an HHDM-resident trampoline and execute VMXOFF;
//! 4. contain PCI DMA, replace only the kernel PML4 slot, flush the TLB, and jump;
//! 5. bring the new kernel up from copied immutable Limine boot facts and restore VMs.
//!
//! No candidate-kernel bytes are written to the ESP or TRUEOSFS. Persistent
//! storage is used only for VM application checkpoints selected by this handoff.

use alloc::{format, string::String, vec::Vec};
use core::{
    convert::Infallible,
    fmt,
    mem::{offset_of, size_of},
    ptr,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
};

use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Instant, Timer};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::shell2::{MatrixTarget, matrix_target_interrupted, print_matrix_target_line};

pub(crate) const RENDEZVOUS_VECTOR: u8 = 0x43;

const PAGE_SIZE: usize = 4096;
const PAGE_MASK: u64 = !(PAGE_SIZE as u64 - 1);
const TWO_MIB: usize = 2 * 1024 * 1024;
const ONE_GIB: usize = 1024 * 1024 * 1024;
const MAX_KERNEL_FILE_BYTES: usize = 256 * 1024 * 1024;
const MAX_KERNEL_SPAN_BYTES: usize = 1024 * 1024 * 1024;
const AP_TRANSITION_STACK_BYTES: usize = 2 * 1024 * 1024;
const VM_CHECKPOINT_TIMEOUT_MS: u64 = 20_000;
const AP_RENDEZVOUS_TIMEOUT_MS: u64 = 5_000;
const POST_BOOT_SERVICE_TIMEOUT_MS: u64 = 30_000;
const POST_BOOT_TLB_TIMEOUT_MS: u64 = 2_000;
const MAX_TRAMPOLINE_BYTES: usize = 64 * 1024;
const MAX_TRANSITION_GDT_BYTES: usize = 256;
const PCI_DMA_DRAIN_MS: u64 = 10;
const ABORT_DRAIN_TIMEOUT_MS: u64 = 2_000;

const LIVE_MANIFEST_MAGIC0: u64 = 0x5452_5545_4F53_4C55; // "TRUEOSLU"
const LIVE_MANIFEST_MAGIC1: u64 = 0x4C49_5645_5550_4454; // "LIVEUPDT"
const LIVE_ABI_VERSION: u64 = 1;

const HANDOFF_MAGIC0: u64 = 0x5452_5545_5741_524D; // "TRUEWARM"
const HANDOFF_MAGIC1: u64 = 0x4655_4C4C_464F_5247; // "FULLFORG"
const HANDOFF_STATE_COMMITTED: u64 = 1;

const HANDOFF_VALIDATION_UNCHECKED: u8 = 0;
const HANDOFF_VALIDATION_CHECKING: u8 = 1;
const HANDOFF_VALIDATION_INVALID: u8 = 2;
const HANDOFF_VALIDATION_VALID: u8 = 3;

const TRANSITION_PARK: u64 = 1;
const TRANSITION_ABORT: u64 = 2;
const TRANSITION_SWITCH_STACKS: u64 = 3;
const TRANSITION_COMMIT: u64 = 4;

const VM_ID_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
const CPU_SLOT_LIMIT: usize = crate::percpu::CPU_SLOT_LIMIT;
const RESTORE_WORDS: usize = (VM_ID_LIMIT + 63) / 64;

#[derive(Clone, Copy)]
#[repr(C)]
pub(crate) struct WarmReservedRange {
    pub(crate) phys_start: u64,
    pub(crate) length: u64,
}

impl WarmReservedRange {
    const EMPTY: Self = Self {
        phys_start: 0,
        length: 0,
    };

    fn valid(self) -> bool {
        self.length != 0 && self.phys_start.checked_add(self.length).is_some()
    }
}

const SHELL_NOTICE: &[u8] =
    b"\r\nlive-update: step=20/20 new kernel accepted this fresh TCP connection; VM restore path was armed\r\n";

unsafe extern "C" {
    static __limine_requests_start: u8;
    static __limine_requests_end: u8;
    static __live_update_trampoline_start: u8;
    static __live_update_trampoline_end: u8;
}

#[derive(Clone, Copy)]
#[repr(C, align(64))]
struct WarmHandoff {
    magic0: u64,
    magic1: u64,
    abi_version: u64,
    state: u64,
    generation: u64,
    candidate_hash: u64,
    arena_phys: u64,
    arena_len: u64,
    kernel_virt_base: u64,
    kernel_phys_base: u64,
    kernel_len: u64,
    kernel_file_phys: u64,
    kernel_file_len: u64,
    hhdm_base: u64,
    expected_aps: u64,
    transition_slot: u64,
    vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],
    restore_mask: [u64; RESTORE_WORDS],
    resume_mask: [u64; RESTORE_WORDS],
    checksum: u64,
}

impl WarmHandoff {
    const EMPTY: Self = Self {
        magic0: 0,
        magic1: 0,
        abi_version: 0,
        state: 0,
        generation: 0,
        candidate_hash: 0,
        arena_phys: 0,
        arena_len: 0,
        kernel_virt_base: 0,
        kernel_phys_base: 0,
        kernel_len: 0,
        kernel_file_phys: 0,
        kernel_file_len: 0,
        hhdm_base: 0,
        expected_aps: 0,
        transition_slot: 0,
        vm_heap_ranges: [WarmReservedRange::EMPTY; VM_ID_LIMIT],
        restore_mask: [0; RESTORE_WORDS],
        resume_mask: [0; RESTORE_WORDS],
        checksum: 0,
    };

    fn valid(&self) -> bool {
        self.magic0 == HANDOFF_MAGIC0
            && self.magic1 == HANDOFF_MAGIC1
            && self.abi_version == LIVE_ABI_VERSION
            && self.state == HANDOFF_STATE_COMMITTED
            && self.arena_len != 0
            && self.kernel_len != 0
            && self.hhdm_base != 0
            && self.checksum == handoff_checksum(self)
    }
}

#[used]
#[unsafe(link_section = ".live_update_handoff")]
static mut LIVE_HANDOFF: WarmHandoff = WarmHandoff::EMPTY;

static WARM_HANDOFF_VALIDATION: AtomicU8 = AtomicU8::new(HANDOFF_VALIDATION_UNCHECKED);
static LIVE_UPDATE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static WARM_APS_RELEASED: AtomicBool = AtomicBool::new(false);
static SHELL_NOTICE_PENDING: AtomicBool = AtomicBool::new(false);
static ACTIVE_CONTROL_HHDM: AtomicU64 = AtomicU64::new(0);
static RENDEZVOUS_ISR_ACTIVE: AtomicU64 = AtomicU64::new(0);
static POST_BOOT_TLB_ACTIVE: AtomicBool = AtomicBool::new(false);
static POST_BOOT_TLB_ACKS: AtomicU64 = AtomicU64::new(0);
static AP_TRANSITION_ENTERED: [AtomicBool; CPU_SLOT_LIMIT] =
    [const { AtomicBool::new(false) }; CPU_SLOT_LIMIT];
static WARM_VM_RANGE_CLAIMED: [AtomicBool; VM_ID_LIMIT] =
    [const { AtomicBool::new(false) }; VM_ID_LIMIT];

#[repr(C, align(64))]
struct TransitionControl {
    command: AtomicU64,
    arrived: AtomicU64,
    stacked: AtomicU64,
    failures: AtomicU64,
    cr3: u64,
    root_hhdm: u64,
    kernel_slot: u64,
    new_slot_entry: u64,
    transition_slot: u64,
    transition_slot_entry: u64,
    stack_base_hhdm: u64,
    stack_stride: u64,
    bsp_entry: u64,
    ap_entry: u64,
    ap_park_hhdm: u64,
    bsp_commit_hhdm: u64,
    expected_aps: u64,
    transition_gdt: [u8; MAX_TRANSITION_GDT_BYTES],
    transition_gdtr: [u8; 10],
}

#[derive(Debug)]
pub enum LiveUpdateError {
    Busy,
    Interrupted,
    BadElf(&'static str),
    Incompatible(&'static str),
    OutOfMemory,
    ArithmeticOverflow,
    VmNotReplicatable(u8),
    VmCheckpointRequest(u8),
    VmCheckpointTimeout(u8),
    VmCheckpointStore(u8),
    ApRendezvous(&'static str),
}

impl fmt::Display for LiveUpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => write!(f, "another live update is already active"),
            Self::Interrupted => write!(f, "interrupted"),
            Self::BadElf(reason) => write!(f, "bad candidate ELF ({reason})"),
            Self::Incompatible(reason) => write!(f, "incompatible candidate ({reason})"),
            Self::OutOfMemory => write!(f, "not enough contiguous RAM for candidate generation"),
            Self::ArithmeticOverflow => write!(f, "candidate layout arithmetic overflow"),
            Self::VmNotReplicatable(vm) => {
                write!(f, "vm{vm} is active but not checkpoint-replicatable")
            }
            Self::VmCheckpointRequest(vm) => write!(f, "vm{vm} checkpoint request failed"),
            Self::VmCheckpointTimeout(vm) => write!(f, "vm{vm} checkpoint timed out"),
            Self::VmCheckpointStore(vm) => write!(f, "vm{vm} persistent checkpoint failed"),
            Self::ApRendezvous(reason) => write!(f, "AP rendezvous failed ({reason})"),
        }
    }
}

#[derive(Clone, Copy)]
struct ElfSection {
    addr: u64,
    size: usize,
}

#[derive(Clone, Copy)]
struct LoadSegment {
    vaddr: u64,
    flags: u32,
    offset: usize,
    file_size: usize,
    mem_size: usize,
}

struct ParsedElf {
    entry: u64,
    min_vaddr: u64,
    max_vaddr: u64,
    loads: Vec<LoadSegment>,
    limine_requests: ElfSection,
    live_manifest: ElfSection,
}

struct TablePool {
    next_phys: u64,
    end_phys: u64,
    hhdm: u64,
}

impl TablePool {
    unsafe fn alloc_zeroed(&mut self) -> Result<u64, LiveUpdateError> {
        let phys = align_up_u64(self.next_phys, PAGE_SIZE as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let end = phys
            .checked_add(PAGE_SIZE as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        if end > self.end_phys {
            return Err(LiveUpdateError::OutOfMemory);
        }
        let virt = self
            .hhdm
            .checked_add(phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)? as *mut u8;
        ptr::write_bytes(virt, 0, PAGE_SIZE);
        self.next_phys = end;
        Ok(phys)
    }

    unsafe fn table_ptr(&self, phys: u64) -> Result<*mut u64, LiveUpdateError> {
        let virt = self
            .hhdm
            .checked_add(phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        Ok(virt as *mut u64)
    }

    unsafe fn child_table(
        &mut self,
        parent_phys: u64,
        index: usize,
    ) -> Result<u64, LiveUpdateError> {
        let parent = self.table_ptr(parent_phys)?;
        let entry = ptr::read_volatile(parent.add(index));
        if entry & 1 != 0 {
            return Ok(entry & PAGE_MASK);
        }
        let child = self.alloc_zeroed()?;
        ptr::write_volatile(parent.add(index), child | 0x003);
        Ok(child)
    }
}

struct StagedCandidate {
    arena_phys: u64,
    arena_len: usize,
    control_hhdm: u64,
    handoff_hhdm: u64,
    expected_aps: u64,
    transition_installed: bool,
    committed: bool,
}

impl StagedCandidate {
    fn control(&self) -> &'static TransitionControl {
        unsafe { &*(self.control_hhdm as *const TransitionControl) }
    }

    fn handoff_mut(&mut self) -> &'static mut WarmHandoff {
        unsafe { &mut *(self.handoff_hhdm as *mut WarmHandoff) }
    }

    fn set_vm_plan(
        &mut self,
        restore_mask: [u64; RESTORE_WORDS],
        resume_mask: [u64; RESTORE_WORDS],
        vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],
    ) {
        let handoff = self.handoff_mut();
        handoff.restore_mask = restore_mask;
        handoff.resume_mask = resume_mask;
        handoff.vm_heap_ranges = vm_heap_ranges;
        handoff.checksum = handoff_checksum(handoff);
    }

    fn mark_committed(&mut self) {
        let handoff = self.handoff_mut();
        handoff.state = HANDOFF_STATE_COMMITTED;
        handoff.checksum = handoff_checksum(handoff);
        self.committed = true;
    }
}

impl Drop for StagedCandidate {
    fn drop(&mut self) {
        if !self.committed {
            if self.transition_installed {
                unsafe { clear_transition_mapping(self) };
            }
            let _ = crate::phys::free_phys_range(self.arena_phys, self.arena_len);
        }
    }
}

struct CheckpointPlan {
    restore_mask: [u64; RESTORE_WORDS],
    resume_mask: [u64; RESTORE_WORDS],
    vm_heap_ranges: [WarmReservedRange; VM_ID_LIMIT],
    paused_by_update: Vec<u8>,
}

struct LiveUpdateRunGuard;

impl LiveUpdateRunGuard {
    fn acquire() -> Result<Self, LiveUpdateError> {
        LIVE_UPDATE_IN_PROGRESS
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| LiveUpdateError::Busy)
    }
}

impl Drop for LiveUpdateRunGuard {
    fn drop(&mut self) {
        LIVE_UPDATE_IN_PROGRESS.store(false, Ordering::Release);
    }
}

pub(crate) fn interrupt_install(idt: &mut InterruptDescriptorTable) {
    idt[RENDEZVOUS_VECTOR].set_handler_fn(live_update_rendezvous_isr);
}

/// Emit a transition marker without allocating, taking a lock, or relying on
/// Embassy/network progress. Steps 12-17 remain observable on COM1 and the
/// QEMU-style debug port after the ordinary runtime has been frozen.
#[inline]
fn transition_marker(marker: &'static [u8]) {
    crate::uart1_com1::write_bytes(marker);
    for &byte in marker {
        unsafe { crate::portio::outb(0xE9, byte) };
    }
}

#[allow(non_snake_case)]
extern "x86-interrupt" fn live_update_rendezvous_isr(_frame: InterruptStackFrame) {
    crate::remote_work_wake::local_eoi();

    if POST_BOOT_TLB_ACTIVE.load(Ordering::Acquire) {
        unsafe { reload_cr3() };
        POST_BOOT_TLB_ACKS.fetch_add(1, Ordering::AcqRel);
        return;
    }

    let control_addr = ACTIVE_CONTROL_HHDM.load(Ordering::Acquire);
    if control_addr == 0 {
        return;
    }
    RENDEZVOUS_ISR_ACTIVE.fetch_add(1, Ordering::AcqRel);
    // Pair pointer publication with abort-side invalidation. If abort won the
    // race before this ISR became visible, do not dereference the arena.
    if ACTIVE_CONTROL_HHDM.load(Ordering::Acquire) != control_addr {
        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    let control = unsafe { &*(control_addr as *const TransitionControl) };
    if control.command.load(Ordering::Acquire) != TRANSITION_PARK {
        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        return;
    }

    let slot = crate::percpu::current_slot();
    if slot == 0 || slot >= AP_TRANSITION_ENTERED.len() {
        control.failures.fetch_add(1, Ordering::AcqRel);
        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        return;
    }
    if AP_TRANSITION_ENTERED[slot].swap(true, Ordering::AcqRel) {
        RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
        return;
    }

    control.arrived.fetch_add(1, Ordering::AcqRel);
    let left_vmx = match crate::hv::leave_vmx_root_for_current_cpu_contract() {
        Ok(left) => left,
        Err(_) => {
            control.failures.fetch_add(1, Ordering::AcqRel);
            AP_TRANSITION_ENTERED[slot].store(false, Ordering::Release);
            control.arrived.fetch_sub(1, Ordering::AcqRel);
            RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
            return;
        }
    };
    let lapic_id = crate::percpu::this_cpu().lapic_id();
    let park: extern "C" fn(*const TransitionControl, usize, u32) =
        unsafe { core::mem::transmute(control.ap_park_hhdm as usize) };
    park(control, slot, lapic_id);

    // The dedicated transition mapping returns only when the BSP aborts
    // before the irreversible stack-switch phase. `arrived` is released last:
    // once the BSP observes zero it may unmap and free the control arena.
    if left_vmx && crate::hv::enter_vmx_root_for_current_cpu_contract().is_err() {
        control.failures.fetch_add(1, Ordering::AcqRel);
    }
    AP_TRANSITION_ENTERED[slot].store(false, Ordering::Release);
    control.arrived.fetch_sub(1, Ordering::AcqRel);
    RENDEZVOUS_ISR_ACTIVE.fetch_sub(1, Ordering::AcqRel);
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".live_update_trampoline")]
#[unsafe(naked)]
unsafe extern "C" fn trueos_live_update_ap_park_trampoline(
    _control: *const TransitionControl,
    _slot: usize,
    _lapic_id: u32,
) {
    core::arch::naked_asm!(
        "mov r8, rdi",
        "mov r9, rsi",
        "mov r10, rdx",
        "1:",
        "mov rax, qword ptr [r8 + {command}]",
        "cmp rax, {abort}",
        "je 9f",
        "cmp rax, {switch_stacks}",
        "je 3f",
        "pause",
        "jmp 1b",
        "3:",
        "mov rax, r9",
        "inc rax",
        "imul rax, qword ptr [r8 + {stack_stride}]",
        "add rax, qword ptr [r8 + {stack_base}]",
        "mov rsp, rax",
        "and rsp, -16",
        "lgdt [r8 + {transition_gdtr}]",
        "lock inc qword ptr [r8 + {stacked}]",
        "4:",
        "mov rax, qword ptr [r8 + {command}]",
        "cmp rax, {commit}",
        "jne 5f",
        // Flush global and non-global translations after the BSP replaces the
        // shared root PML4 kernel entry.
        "mov rcx, cr4",
        "mov rdx, rcx",
        "and rcx, -129",
        "mov cr4, rcx",
        "mov rax, qword ptr [r8 + {cr3}]",
        "mov cr3, rax",
        "mov cr4, rdx",
        "mov rax, qword ptr [r8 + {ap_entry}]",
        "mov rdi, r10",
        "mov rsi, r9",
        // A direct jump into an extern-C function needs the same stack
        // alignment the callee would observe after CALL.
        "sub rsp, 8",
        "jmp rax",
        "5:",
        "pause",
        "jmp 4b",
        "9:",
        "ret",
        command = const offset_of!(TransitionControl, command),
        stacked = const offset_of!(TransitionControl, stacked),
        cr3 = const offset_of!(TransitionControl, cr3),
        stack_base = const offset_of!(TransitionControl, stack_base_hhdm),
        stack_stride = const offset_of!(TransitionControl, stack_stride),
        ap_entry = const offset_of!(TransitionControl, ap_entry),
        transition_gdtr = const offset_of!(TransitionControl, transition_gdtr),
        abort = const TRANSITION_ABORT,
        switch_stacks = const TRANSITION_SWITCH_STACKS,
        commit = const TRANSITION_COMMIT,
    );
}

#[unsafe(no_mangle)]
#[unsafe(link_section = ".live_update_trampoline")]
#[unsafe(naked)]
unsafe extern "C" fn trueos_live_update_bsp_commit_trampoline(
    _control: *const TransitionControl,
) -> ! {
    core::arch::naked_asm!(
        "mov r8, rdi",
        "mov rsp, qword ptr [r8 + {stack_base}]",
        "add rsp, qword ptr [r8 + {stack_stride}]",
        "and rsp, -16",
        "lgdt [r8 + {transition_gdtr}]",
        "mov rax, qword ptr [r8 + {root_hhdm}]",
        "mov rcx, qword ptr [r8 + {kernel_slot}]",
        "mov rdx, qword ptr [r8 + {new_slot_entry}]",
        "mov qword ptr [rax + rcx * 8], rdx",
        "mfence",
        // Toggling CR4.PGE guarantees stale global kernel translations are
        // discarded before execution enters the replacement image.
        "mov rcx, cr4",
        "mov rdx, rcx",
        "and rcx, -129",
        "mov cr4, rcx",
        "mov rax, qword ptr [r8 + {cr3}]",
        "mov cr3, rax",
        "mov cr4, rdx",
        "mov rax, {commit}",
        "mov qword ptr [r8 + {command}], rax",
        "mfence",
        // Discard generation-N per-CPU identity before candidate Rust runs.
        "mov ecx, 0xC0000101",
        "xor eax, eax",
        "xor edx, edx",
        "wrmsr",
        "mov rax, qword ptr [r8 + {bsp_entry}]",
        "jmp rax",
        command = const offset_of!(TransitionControl, command),
        cr3 = const offset_of!(TransitionControl, cr3),
        root_hhdm = const offset_of!(TransitionControl, root_hhdm),
        kernel_slot = const offset_of!(TransitionControl, kernel_slot),
        new_slot_entry = const offset_of!(TransitionControl, new_slot_entry),
        stack_base = const offset_of!(TransitionControl, stack_base_hhdm),
        stack_stride = const offset_of!(TransitionControl, stack_stride),
        bsp_entry = const offset_of!(TransitionControl, bsp_entry),
        transition_gdtr = const offset_of!(TransitionControl, transition_gdtr),
        commit = const TRANSITION_COMMIT,
    );
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_live_update_ap_entry(lapic_id: u32, slot: u32) -> ! {
    // The candidate BSP may publish its fresh global PERCPU_READY before this
    // AP has installed a candidate PerCpu. Clear the generation-N GS pointer
    // so any defensive identity probe sees "not initialized" rather than an
    // unmapped old-kernel allocation.
    core::arch::asm!(
        "wrmsr",
        in("ecx") 0xC000_0101u32,
        in("eax") 0u32,
        in("edx") 0u32,
        options(nostack, preserves_flags),
    );
    while !WARM_APS_RELEASED.load(Ordering::Acquire) {
        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
    }
    crate::cpu::warm_ap_start(lapic_id, slot)
}

pub fn warm_boot_active() -> bool {
    warm_handoff().is_some()
}

pub fn warm_generation() -> Option<u64> {
    warm_handoff().map(|handoff| handoff.generation)
}

pub fn warm_hhdm_offset() -> Option<u64> {
    warm_handoff().map(|handoff| handoff.hhdm_base)
}

pub fn warm_kernel_bases() -> Option<(u64, u64)> {
    warm_handoff().map(|handoff| (handoff.kernel_virt_base, handoff.kernel_phys_base))
}

pub fn for_each_warm_reserved_phys_range(mut visit: impl FnMut(u64, u64)) {
    let Some(handoff) = warm_handoff() else {
        return;
    };
    visit(handoff.arena_phys, handoff.arena_len);
    for range in handoff.vm_heap_ranges {
        if range.valid() {
            visit(range.phys_start, range.length);
        }
    }
}

pub fn claim_warm_vm_heap_range(phys_start: u64, length: u64) -> bool {
    let Some(handoff) = warm_handoff() else {
        return false;
    };
    for (index, range) in handoff.vm_heap_ranges.iter().copied().enumerate() {
        if range.phys_start == phys_start && range.length == length && range.valid() {
            return WARM_VM_RANGE_CLAIMED[index]
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
        }
    }
    false
}

pub fn warm_kernel_file_bytes() -> Option<&'static [u8]> {
    let handoff = warm_handoff()?;
    let virt = handoff.hhdm_base.checked_add(handoff.kernel_file_phys)?;
    let len = usize::try_from(handoff.kernel_file_len).ok()?;
    Some(unsafe { core::slice::from_raw_parts(virt as *const u8, len) })
}

pub fn log_boot_mode() {
    if let Some(handoff) = warm_handoff() {
        crate::log_info!(
            target: "global";
            "live-update: step=18/20 candidate-kmain-entered generation={} candidate_hash=0x{:016X} arena=0x{:016X}+0x{:X} expected_aps={} mode=fullforget-warm\n",
            handoff.generation,
            handoff.candidate_hash,
            handoff.arena_phys,
            handoff.arena_len,
            handoff.expected_aps,
        );
    }
}

pub fn release_warm_aps() {
    if let Some(handoff) = warm_handoff() {
        WARM_APS_RELEASED.store(true, Ordering::Release);
        crate::log_info!(
            target: "global";
            "live-update: step=19/20 parked-APs-released count={} generation={}\n",
            handoff.expected_aps,
            handoff.generation,
        );
    }
}

pub fn spawn_post_boot(spawner: Spawner) {
    let Some(handoff) = warm_handoff().copied() else {
        return;
    };

    match restore_after_live_update_task(
        spawner,
        handoff.restore_mask,
        handoff.resume_mask,
        handoff.transition_slot,
        handoff.generation,
    ) {
        Ok(token) => {
            spawner.spawn(token);
            SHELL_NOTICE_PENDING.store(true, Ordering::Release);
            crate::log_info!(
                target: "global";
                "live-update: step=20/20 post-boot-restore-armed generation={} fresh_tcp_notice=armed\n",
                handoff.generation,
            );
        }
        Err(error) => crate::log_warn!(
            target: "global";
            "live-update: restore task unavailable generation={} error={:?}\n",
            handoff.generation,
            error,
        ),
    }
}

pub fn take_shell_notice() -> Option<&'static [u8]> {
    SHELL_NOTICE_PENDING
        .swap(false, Ordering::AcqRel)
        .then_some(SHELL_NOTICE)
}

pub fn rearm_shell_notice() {
    if warm_boot_active() {
        SHELL_NOTICE_PENDING.store(true, Ordering::Release);
    }
}

#[trueos_executor::task]
async fn restore_after_live_update_task(
    spawner: Spawner,
    restore_mask: [u64; RESTORE_WORDS],
    resume_mask: [u64; RESTORE_WORDS],
    transition_slot: u64,
    generation: u64,
) {
    let topology_deadline = Instant::now()
        .as_millis()
        .saturating_add(POST_BOOT_SERVICE_TIMEOUT_MS);
    while !crate::workers::all_topology_spawners_registered()
        && Instant::now().as_millis() < topology_deadline
    {
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }
    cleanup_transition_mapping_after_boot(transition_slot as usize).await;

    if restore_mask.iter().all(|word| *word == 0) {
        crate::log_info!(
            target: "global";
            "live-update: generation={} has no VM checkpoints to restore\n",
            generation,
        );
        return;
    }

    crate::r::readiness::wait_for(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED).await;
    let deadline = Instant::now()
        .as_millis()
        .saturating_add(POST_BOOT_SERVICE_TIMEOUT_MS);
    while !crate::hv::store::online() && Instant::now().as_millis() < deadline {
        Timer::after(EmbassyDuration::from_millis(25)).await;
    }

    for vm_id in 0..VM_ID_LIMIT {
        if !mask_contains(&restore_mask, vm_id) {
            continue;
        }
        let vm_id = vm_id as u8;
        let name = checkpoint_name(vm_id);

        let _ = crate::hv::eject(vm_id);
        match crate::hv::try_begin_restore(vm_id) {
            Ok(true) => {}
            Ok(false) => {
                crate::log_warn!(
                    target: "global";
                    "live-update: vm{} restore already pending name={}\n",
                    vm_id,
                    name,
                );
                continue;
            }
            Err(error) => {
                crate::log_warn!(
                    target: "global";
                    "live-update: vm{} restore admission failed name={} error={:?}\n",
                    vm_id,
                    name,
                    error,
                );
                continue;
            }
        }

        let image = match crate::hv::store::load_persistent_async(name.as_str()).await {
            Ok(image) => image,
            Err(error) => {
                crate::log_warn!(
                    target: "global";
                    "live-update: vm{} checkpoint load failed name={} error={:?}\n",
                    vm_id,
                    name,
                    error,
                );
                crate::hv::finish_restore(vm_id);
                continue;
            }
        };
        if let Err(error) = crate::hv::store::save_bytes_async(vm_id, image.snapshot.clone()).await
        {
            crate::log_warn!(
                target: "global";
                "live-update: vm{} warm-store seed failed name={} error={:?}\n",
                vm_id,
                name,
                error,
            );
            crate::hv::finish_restore(vm_id);
            continue;
        }
        if let Err(error) = crate::hv::restore_persistent_image(vm_id, &image, None) {
            crate::log_warn!(
                target: "global";
                "live-update: vm{} envelope import failed name={} error={:?}\n",
                vm_id,
                name,
                error,
            );
            crate::hv::finish_restore(vm_id);
            continue;
        }

        if mask_contains(&resume_mask, vm_id as usize) {
            match crate::hv::start(vm_id, &spawner, None) {
                Ok(()) => crate::log_info!(
                    target: "global";
                    "live-update: vm{} restored and resume scheduled name={} generation={}\n",
                    vm_id,
                    name,
                    generation,
                ),
                Err(error) => crate::log_warn!(
                    target: "global";
                    "live-update: vm{} restored but resume failed name={} error={:?}\n",
                    vm_id,
                    name,
                    error,
                ),
            }
        } else {
            crate::log_info!(
                target: "global";
                "live-update: vm{} restored in retained-pause state name={} generation={}\n",
                vm_id,
                name,
                generation,
            );
        }
        crate::hv::finish_restore(vm_id);
    }
}

pub async fn stage_and_swap(
    kernel: Vec<u8>,
    spawner: Spawner,
    target: MatrixTarget,
) -> Result<Infallible, LiveUpdateError> {
    let _run_guard = LiveUpdateRunGuard::acquire()?;
    if matrix_target_interrupted(&target) {
        return Err(LiveUpdateError::Interrupted);
    }

    print_matrix_target_line(
        &target,
        "update live: step=06/20 validating and staging candidate in RAM; disk image remains unchanged",
    );
    let mut staged = stage_candidate(kernel.as_slice())?;
    print_matrix_target_line(
        &target,
        format!(
            "update live: step=07/20 candidate-staged arena=0x{:016X}+{} MiB APs={}",
            staged.arena_phys,
            staged.arena_len / (1024 * 1024),
            staged.expected_aps,
        )
        .as_str(),
    );
    drop(kernel);

    print_matrix_target_line(
        &target,
        "update live: step=08/20 checkpointing active VMX apps to TRUEOSFS",
    );
    let checkpoint = checkpoint_active_vms(&spawner, &target).await?;
    let checkpoint_count: u32 = checkpoint
        .restore_mask
        .iter()
        .map(|word| word.count_ones())
        .sum();
    staged.set_vm_plan(checkpoint.restore_mask, checkpoint.resume_mask, checkpoint.vm_heap_ranges);
    print_matrix_target_line(
        &target,
        format!("update live: step=09/20 VM checkpoints committed count={}", checkpoint_count,)
            .as_str(),
    );

    if matrix_target_interrupted(&target) {
        resume_checkpointed_vms(&spawner, &target, checkpoint.paused_by_update.as_slice()).await;
        return Err(LiveUpdateError::Interrupted);
    }

    // Snapshot BDFs before APs are parked. The irreversible path uses this
    // immutable list and never takes the ordinary PCI registry/config locks.
    let pci_snapshot = crate::pci::fullforget_snapshot();
    print_matrix_target_line(
        &target,
        format!(
            "update live: step=10/20 PCI snapshot captured functions={} containment=lock-free",
            pci_snapshot.len(),
        )
        .as_str(),
    );

    print_matrix_target_line(
        &target,
        "update live: step=11/20 irreversible rendezvous next; TCP must disconnect; COM1 continues with steps 12-17",
    );
    // Flush the final user-facing line while every normal runtime service is
    // still schedulable. After AP rendezvous succeeds the path takes no locks,
    // performs no allocation, and never returns to the old kernel.
    Timer::after(EmbassyDuration::from_millis(100)).await;

    if let Err(error) = rendezvous_aps(&mut staged) {
        resume_checkpointed_vms(&spawner, &target, checkpoint.paused_by_update.as_slice()).await;
        return Err(error);
    }

    staged.mark_committed();
    unsafe { commit_fullforget(&staged, pci_snapshot.as_slice()) }
}

async fn checkpoint_active_vms(
    spawner: &Spawner,
    target: &MatrixTarget,
) -> Result<CheckpointPlan, LiveUpdateError> {
    let mut restore_mask = [0u64; RESTORE_WORDS];
    let mut resume_mask = [0u64; RESTORE_WORDS];
    let mut vm_heap_ranges = [WarmReservedRange::EMPTY; VM_ID_LIMIT];
    let mut paused_by_update = Vec::new();

    for vm_index in 0..VM_ID_LIMIT {
        let vm_id = vm_index as u8;
        let state = crate::hv::vm_state(vm_id);
        if !state.supported || !(state.running || state.starting || state.pause_latched) {
            continue;
        }

        if state.running || state.starting {
            mask_insert(&mut resume_mask, vm_index);
            if !state.replicatable {
                return checkpoint_abort(
                    spawner,
                    target,
                    &paused_by_update,
                    LiveUpdateError::VmNotReplicatable(vm_id),
                )
                .await;
            }
            match crate::hv::request_replicatable_snapshot(vm_id) {
                Ok(true) => {
                    if !paused_by_update.contains(&vm_id) {
                        paused_by_update.push(vm_id);
                    }
                    mask_insert(&mut resume_mask, vm_index);
                    print_matrix_target_line(
                        target,
                        format!("update live: vm{} PreparePause snapshot requested", vm_id)
                            .as_str(),
                    );
                }
                Ok(false) if state.prepare_pause_pending => {}
                Ok(false) | Err(_) => {
                    return checkpoint_abort(
                        spawner,
                        target,
                        &paused_by_update,
                        LiveUpdateError::VmCheckpointRequest(vm_id),
                    )
                    .await;
                }
            }
        } else if !state.pause_snapshot_ready || !crate::hv::store::has_committed_vm(vm_id) {
            return checkpoint_abort(
                spawner,
                target,
                &paused_by_update,
                LiveUpdateError::VmCheckpointRequest(vm_id),
            )
            .await;
        }

        let deadline = Instant::now()
            .as_millis()
            .saturating_add(VM_CHECKPOINT_TIMEOUT_MS);
        loop {
            if matrix_target_interrupted(target) {
                return checkpoint_abort(
                    spawner,
                    target,
                    &paused_by_update,
                    LiveUpdateError::Interrupted,
                )
                .await;
            }
            let state = crate::hv::vm_state(vm_id);
            if state.pause_latched
                && state.pause_snapshot_ready
                && crate::hv::store::has_committed_vm(vm_id)
            {
                break;
            }
            if Instant::now().as_millis() >= deadline {
                return checkpoint_abort(
                    spawner,
                    target,
                    &paused_by_update,
                    LiveUpdateError::VmCheckpointTimeout(vm_id),
                )
                .await;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        let name = checkpoint_name(vm_id);
        match crate::hv::store::store_persistent_async(vm_id, name.as_str()).await {
            Ok(bytes) => {
                let Some(stats) = crate::allocators::hv_guest_heap_stats_if_configured(vm_id)
                else {
                    return checkpoint_abort(
                        spawner,
                        target,
                        &paused_by_update,
                        LiveUpdateError::VmCheckpointStore(vm_id),
                    )
                    .await;
                };
                let heap_len = stats.heap_end.saturating_sub(stats.heap_start);
                if stats.phys_start == 0 || heap_len == 0 {
                    return checkpoint_abort(
                        spawner,
                        target,
                        &paused_by_update,
                        LiveUpdateError::VmCheckpointStore(vm_id),
                    )
                    .await;
                }
                vm_heap_ranges[vm_index] = WarmReservedRange {
                    phys_start: stats.phys_start as u64,
                    length: heap_len as u64,
                };
                mask_insert(&mut restore_mask, vm_index);
                print_matrix_target_line(
                    target,
                    format!("update live: vm{} checkpointed as {} ({} bytes)", vm_id, name, bytes)
                        .as_str(),
                );
            }
            Err(error) => {
                print_matrix_target_line(
                    target,
                    format!("update live: vm{} persistent checkpoint failed ({:?})", vm_id, error)
                        .as_str(),
                );
                return checkpoint_abort(
                    spawner,
                    target,
                    &paused_by_update,
                    LiveUpdateError::VmCheckpointStore(vm_id),
                )
                .await;
            }
        }
    }

    Ok(CheckpointPlan {
        restore_mask,
        resume_mask,
        vm_heap_ranges,
        paused_by_update,
    })
}

async fn checkpoint_abort(
    spawner: &Spawner,
    target: &MatrixTarget,
    touched_vms: &[u8],
    error: LiveUpdateError,
) -> Result<CheckpointPlan, LiveUpdateError> {
    resume_checkpointed_vms(spawner, target, touched_vms).await;
    Err(error)
}

async fn resume_checkpointed_vms(spawner: &Spawner, target: &MatrixTarget, vm_ids: &[u8]) {
    for &vm_id in vm_ids {
        // A PreparePause may cross its Ready boundary just after an update
        // cancellation. Wait briefly for that boundary so the compensating
        // start cannot race and leave the VM paused after this task returns.
        let deadline = Instant::now().as_millis().saturating_add(2_000);
        loop {
            let state = crate::hv::vm_state(vm_id);
            if state.pause_latched || !state.prepare_pause_pending {
                break;
            }
            if Instant::now().as_millis() >= deadline {
                break;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }

        match crate::hv::start(vm_id, spawner, None) {
            Ok(()) => print_matrix_target_line(
                target,
                format!("update live: vm{} resumed after pre-commit abort", vm_id).as_str(),
            ),
            Err(crate::hv::StartError::AlreadyRunning) => {}
            Err(error) => print_matrix_target_line(
                target,
                format!("update live: vm{} resume after abort failed ({:?})", vm_id, error)
                    .as_str(),
            ),
        }
    }
}

fn rendezvous_aps(staged: &mut StagedCandidate) -> Result<(), LiveUpdateError> {
    unsafe { install_transition_mapping(staged)? };
    transition_marker(b"live-update: step=12/20 transition-map-installed\n");
    let control = staged.control();
    control.arrived.store(0, Ordering::Release);
    control.stacked.store(0, Ordering::Release);
    control.failures.store(0, Ordering::Release);

    let interrupts_were_enabled = x86_64::instructions::interrupts::are_enabled();
    x86_64::instructions::interrupts::disable();
    ACTIVE_CONTROL_HHDM.store(staged.control_hhdm, Ordering::Release);
    control.command.store(TRANSITION_PARK, Ordering::Release);

    for slot in 1..crate::percpu::total_slots() {
        if !crate::remote_work_wake::send_fixed_x2apic_ipi(slot as u32, RENDEZVOUS_VECTOR) {
            control.failures.fetch_add(1, Ordering::AcqRel);
            abort_rendezvous(staged, interrupts_were_enabled);
            return Err(LiveUpdateError::ApRendezvous("IPI delivery unavailable"));
        }
    }
    transition_marker(b"live-update: step=13/20 rendezvous-ipis-sent\n");

    // Once an AP has entered the transition trampoline it may be holding an
    // arbitrary old-generation lock. Do not yield to Embassy or execute any
    // normal service code from this point forward. A bounded lock-free spin is
    // the only safe pre-commit rendezvous.
    let timeout_ticks = AP_RENDEZVOUS_TIMEOUT_MS
        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))
        .saturating_add(999)
        / 1000;
    let deadline = embassy_time_driver::now().saturating_add(timeout_ticks);
    while control.arrived.load(Ordering::Acquire) < staged.expected_aps {
        if control.failures.load(Ordering::Acquire) != 0 {
            abort_rendezvous(staged, interrupts_were_enabled);
            return Err(LiveUpdateError::ApRendezvous("AP reported transition failure"));
        }
        if embassy_time_driver::now() >= deadline {
            abort_rendezvous(staged, interrupts_were_enabled);
            return Err(LiveUpdateError::ApRendezvous("timeout"));
        }
        core::hint::spin_loop();
    }
    transition_marker(b"live-update: step=14/20 all-APs-parked\n");

    // Success intentionally leaves BSP interrupts disabled. The caller performs
    // only handoff bookkeeping before entering the non-returning commit path.
    Ok(())
}

fn abort_rendezvous(staged: &mut StagedCandidate, interrupts_were_enabled: bool) {
    transition_marker(b"live-update: transition-abort-requested\n");
    let control = staged.control();
    control.command.store(TRANSITION_ABORT, Ordering::Release);
    // Prevent a late IPI from acquiring the control pointer while the existing
    // ISR/trampoline population drains.
    ACTIVE_CONTROL_HHDM.store(0, Ordering::Release);
    let timeout_ticks = ABORT_DRAIN_TIMEOUT_MS
        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))
        .saturating_add(999)
        / 1000;
    let deadline = embassy_time_driver::now().saturating_add(timeout_ticks);
    while control.arrived.load(Ordering::Acquire) != 0
        || RENDEZVOUS_ISR_ACTIVE.load(Ordering::Acquire) != 0
    {
        if embassy_time_driver::now() >= deadline {
            // An AP still executes transition code. Unmapping/freeing the arena
            // would create an immediate use-after-free, so fail-stop and require
            // the same physical reset the operator was already prepared to use.
            transition_marker(b"live-update: fail-stop=abort-drain-timeout\n");
            loop {
                unsafe {
                    core::arch::asm!("cli", "hlt", options(nomem, nostack));
                }
            }
        }
        core::hint::spin_loop();
    }
    unsafe { clear_transition_mapping(staged) };
    if interrupts_were_enabled {
        x86_64::instructions::interrupts::enable();
    }
}

unsafe fn install_transition_mapping(staged: &mut StagedCandidate) -> Result<(), LiveUpdateError> {
    if staged.transition_installed {
        return Ok(());
    }
    let control = staged.control();
    let root = control.root_hhdm as *mut u64;
    let slot = usize::try_from(control.transition_slot)
        .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;
    if slot >= 512 {
        return Err(LiveUpdateError::Incompatible("transition PML4 slot is invalid"));
    }
    let old = ptr::read_volatile(root.add(slot));
    if old & 1 != 0 {
        return Err(LiveUpdateError::Incompatible("transition PML4 slot became occupied"));
    }
    ptr::write_volatile(root.add(slot), control.transition_slot_entry);
    core::arch::asm!("mfence", options(nostack, preserves_flags));
    reload_cr3();
    staged.transition_installed = true;
    Ok(())
}

unsafe fn clear_transition_mapping(staged: &mut StagedCandidate) {
    if !staged.transition_installed {
        return;
    }
    let control = staged.control();
    let slot = control.transition_slot as usize;
    if slot < 512 {
        let root = control.root_hhdm as *mut u64;
        let current = ptr::read_volatile(root.add(slot));
        if current == control.transition_slot_entry {
            ptr::write_volatile(root.add(slot), 0);
            core::arch::asm!("mfence", options(nostack, preserves_flags));
            reload_cr3();
        }
    }
    staged.transition_installed = false;
}

async fn cleanup_transition_mapping_after_boot(slot: usize) {
    if slot < 256 || slot >= 512 {
        crate::log_warn!(
            target: "global";
            "live-update: transition mapping cleanup skipped invalid_slot={}\n",
            slot,
        );
        return;
    }
    let Some(hhdm) = warm_hhdm_offset() else {
        return;
    };
    let cr3 = read_cr3();
    let Some(root_hhdm) = hhdm.checked_add(cr3 & PAGE_MASK) else {
        return;
    };

    unsafe {
        let root = root_hhdm as *mut u64;
        ptr::write_volatile(root.add(slot), 0);
        core::arch::asm!("mfence", options(nostack, preserves_flags));
        reload_cr3();
    }

    POST_BOOT_TLB_ACKS.store(0, Ordering::Release);
    POST_BOOT_TLB_ACTIVE.store(true, Ordering::Release);
    let expected = crate::percpu::total_slots().saturating_sub(1) as u64;
    let mut sent = 0u64;
    for cpu_slot in 1..crate::percpu::total_slots() {
        if crate::remote_work_wake::send_fixed_x2apic_ipi(cpu_slot as u32, RENDEZVOUS_VECTOR) {
            sent = sent.saturating_add(1);
        }
    }
    let deadline = Instant::now()
        .as_millis()
        .saturating_add(POST_BOOT_TLB_TIMEOUT_MS);
    while POST_BOOT_TLB_ACKS.load(Ordering::Acquire) < sent && Instant::now().as_millis() < deadline
    {
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    POST_BOOT_TLB_ACTIVE.store(false, Ordering::Release);
    let acknowledgements = POST_BOOT_TLB_ACKS.load(Ordering::Acquire);
    crate::log_info!(
        target: "global";
        "live-update: transition mapping retired slot={} tlb_acks={}/{} topology_aps={}\n",
        slot,
        acknowledgements,
        sent,
        expected,
    );
}

#[inline]
fn read_cr3() -> u64 {
    let value: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) value,
            options(nomem, nostack, preserves_flags)
        );
    }
    value
}

#[inline]
unsafe fn reload_cr3() {
    let value = read_cr3();
    core::arch::asm!(
        "mov cr3, {}",
        in(reg) value,
        options(nostack, preserves_flags)
    );
}

unsafe fn find_empty_transition_slot(
    root_hhdm: u64,
    kernel_slot: usize,
    hhdm_slot: usize,
) -> Result<usize, LiveUpdateError> {
    let root = root_hhdm as *const u64;
    for slot in (256usize..512).rev() {
        if slot == kernel_slot || slot == hhdm_slot {
            continue;
        }
        if ptr::read_volatile(root.add(slot)) & 1 == 0 {
            return Ok(slot);
        }
    }
    Err(LiveUpdateError::Incompatible("no empty high-half PML4 slot for transition trampoline"))
}

fn canonical_pml4_slot_base(slot: usize) -> u64 {
    let low = (slot as u64) << 39;
    if slot & 0x100 != 0 {
        low | 0xffff_0000_0000_0000
    } else {
        low
    }
}

unsafe fn commit_fullforget(
    staged: &StagedCandidate,
    pci_snapshot: &[crate::pci::FullforgetPciFunction],
) -> ! {
    let control = staged.control();
    x86_64::instructions::interrupts::disable();

    // This path deliberately bypasses the normal PCI configuration lock. Every
    // AP is parked, so no other CPU can race the CF8/CFC transaction; bypassing
    // also avoids deadlock if an AP happened to be interrupted while owning the
    // normal lock.
    transition_marker(b"live-update: step=15a/20 pci-quiesce-begin\n");
    let dma_failures = crate::pci::fullforget_quiesce_unlocked(pci_snapshot);
    if dma_failures != 0 {
        // Proceeding with a requester that still owns Bus Master Enable would
        // let an old-generation DMA engine overwrite replacement-kernel RAM.
        transition_marker(b"live-update: fail-stop=pci-bus-master-still-enabled\n");
        loop {
            core::arch::asm!("cli", "hlt", options(nomem, nostack));
        }
    }
    transition_marker(b"live-update: step=15b/20 pci-quiesce-ok\n");
    let drain_ticks = PCI_DMA_DRAIN_MS
        .saturating_mul(embassy_time_driver::TICK_HZ.max(1))
        .saturating_add(999)
        / 1000;
    let drain_deadline = embassy_time_driver::now().saturating_add(drain_ticks);
    while embassy_time_driver::now() < drain_deadline {
        core::hint::spin_loop();
    }
    transition_marker(b"live-update: step=16/20 dma-drain-complete\n");

    control
        .command
        .store(TRANSITION_SWITCH_STACKS, Ordering::SeqCst);
    transition_marker(b"live-update: step=17a/20 AP-stack-switch-commanded\n");
    while control.stacked.load(Ordering::Acquire) < staged.expected_aps {
        core::arch::asm!("pause", options(nomem, nostack, preserves_flags));
    }
    transition_marker(b"live-update: step=17b/20 AP-transition-stacks-ready\n");

    let commit: extern "C" fn(*const TransitionControl) -> ! =
        core::mem::transmute(control.bsp_commit_hhdm as usize);
    transition_marker(b"live-update: step=17c/20 BSP-commit-trampoline-enter\n");
    commit(control)
}

fn stage_candidate(kernel: &[u8]) -> Result<StagedCandidate, LiveUpdateError> {
    if kernel.len() > MAX_KERNEL_FILE_BYTES {
        return Err(LiveUpdateError::Incompatible("kernel file exceeds live-update cap"));
    }
    let elf = parse_elf(kernel)?;
    let span = usize::try_from(elf.max_vaddr - elf.min_vaddr)
        .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;
    if span == 0 || span > MAX_KERNEL_SPAN_BYTES {
        return Err(LiveUpdateError::Incompatible("kernel PT_LOAD span exceeds cap"));
    }
    if !range_in_load(&elf.loads, elf.entry, 1, 0x1) {
        return Err(LiveUpdateError::BadElf("entry is not backed by an executable PT_LOAD"));
    }
    if !range_in_load(&elf.loads, elf.limine_requests.addr, elf.limine_requests.size, 0x2) {
        return Err(LiveUpdateError::Incompatible(".limine_requests is not in a writable PT_LOAD"));
    }
    if !range_in_load(&elf.loads, elf.live_manifest.addr, elf.live_manifest.size, 0) {
        return Err(LiveUpdateError::Incompatible(".live_update_slot is not backed by PT_LOAD"));
    }

    let hhdm = crate::limine::hhdm_offset()
        .ok_or(LiveUpdateError::Incompatible("missing HHDM response"))?;
    let (current_virt, _) = crate::limine::executable_address_bases()
        .ok_or(LiveUpdateError::Incompatible("missing executable address response"))?;
    let kernel_slot = pml4_index(elf.min_vaddr);
    if kernel_slot != pml4_index(current_virt) {
        return Err(LiveUpdateError::Incompatible("candidate uses a different kernel PML4 slot"));
    }
    let hhdm_slot = pml4_index(hhdm);
    if hhdm_slot == kernel_slot {
        return Err(LiveUpdateError::Incompatible("HHDM collides with kernel PML4 slot"));
    }

    let cr3: u64;
    unsafe {
        core::arch::asm!(
            "mov {}, cr3",
            out(reg) cr3,
            options(nomem, nostack, preserves_flags)
        );
    }
    let root_phys = cr3 & PAGE_MASK;
    let root_hhdm = hhdm
        .checked_add(root_phys)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    let transition_slot = unsafe { find_empty_transition_slot(root_hhdm, kernel_slot, hhdm_slot)? };
    let transition_base = canonical_pml4_slot_base(transition_slot);

    let trampoline_start = ptr::addr_of!(__live_update_trampoline_start) as usize;
    let trampoline_end = ptr::addr_of!(__live_update_trampoline_end) as usize;
    let trampoline_len = trampoline_end
        .checked_sub(trampoline_start)
        .ok_or(LiveUpdateError::Incompatible("invalid transition trampoline bounds"))?;
    if trampoline_len == 0 || trampoline_len > MAX_TRAMPOLINE_BYTES {
        return Err(LiveUpdateError::Incompatible("transition trampoline size is invalid"));
    }
    let ap_park_offset = (trueos_live_update_ap_park_trampoline as usize)
        .checked_sub(trampoline_start)
        .ok_or(LiveUpdateError::Incompatible("AP trampoline is outside transition section"))?;
    let bsp_commit_offset = (trueos_live_update_bsp_commit_trampoline as usize)
        .checked_sub(trampoline_start)
        .ok_or(LiveUpdateError::Incompatible("BSP trampoline is outside transition section"))?;
    if ap_park_offset >= trampoline_len || bsp_commit_offset >= trampoline_len {
        return Err(LiveUpdateError::Incompatible(
            "transition trampoline symbol is outside copied section",
        ));
    }

    let cpu_count = crate::percpu::total_slots().max(1);
    if cpu_count > CPU_SLOT_LIMIT {
        return Err(LiveUpdateError::Incompatible("CPU topology exceeds transition table"));
    }

    let load_offset = 0usize;
    let file_offset = align_up_usize(span, PAGE_SIZE)?;
    let stack_offset = align_up_usize(
        file_offset
            .checked_add(kernel.len())
            .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        PAGE_SIZE,
    )?;
    let stack_bytes = (cpu_count + 1)
        .checked_mul(AP_TRANSITION_STACK_BYTES)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    let control_offset = align_up_usize(
        stack_offset
            .checked_add(stack_bytes)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        PAGE_SIZE,
    )?;
    let trampoline_offset = align_up_usize(
        control_offset
            .checked_add(PAGE_SIZE)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        PAGE_SIZE,
    )?;
    let trampoline_map_len = align_up_usize(trampoline_len, PAGE_SIZE)?;
    let tables_offset = align_up_usize(
        trampoline_offset
            .checked_add(trampoline_map_len)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        PAGE_SIZE,
    )?;

    let kernel_pt_pages = ceil_div(span, TWO_MIB).saturating_add(2);
    let kernel_pd_pages = ceil_div(span, ONE_GIB).saturating_add(2);
    let transition_pt_pages = ceil_div(trampoline_map_len, TWO_MIB).saturating_add(1);
    let transition_pd_pages = ceil_div(trampoline_map_len, ONE_GIB).saturating_add(1);
    let table_pages = 2usize
        .checked_add(kernel_pt_pages)
        .and_then(|value| value.checked_add(kernel_pd_pages))
        .and_then(|value| value.checked_add(transition_pt_pages))
        .and_then(|value| value.checked_add(transition_pd_pages))
        .and_then(|value| value.checked_add(8))
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    let table_bytes = table_pages
        .checked_mul(PAGE_SIZE)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    let arena_len = align_up_usize(
        tables_offset
            .checked_add(table_bytes)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        TWO_MIB,
    )?;

    let arena_phys = crate::phys::alloc_phys_range(arena_len, TWO_MIB, 0x0100_0000, None)
        .ok_or(LiveUpdateError::OutOfMemory)?;
    let arena_hhdm = hhdm
        .checked_add(arena_phys)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;

    let staged = (|| unsafe {
        let load_hhdm = arena_hhdm
            .checked_add(load_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        ptr::write_bytes(load_hhdm as *mut u8, 0, span);
        for segment in &elf.loads {
            let dst_offset = usize::try_from(segment.vaddr - elf.min_vaddr)
                .map_err(|_| LiveUpdateError::ArithmeticOverflow)?;
            let dst = load_hhdm
                .checked_add(dst_offset as u64)
                .ok_or(LiveUpdateError::ArithmeticOverflow)? as *mut u8;
            ptr::copy_nonoverlapping(kernel.as_ptr().add(segment.offset), dst, segment.file_size);
            if segment.mem_size > segment.file_size {
                ptr::write_bytes(
                    dst.add(segment.file_size),
                    0,
                    segment.mem_size - segment.file_size,
                );
            }
        }

        let file_phys = arena_phys
            .checked_add(file_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let file_hhdm = hhdm
            .checked_add(file_phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        ptr::copy_nonoverlapping(kernel.as_ptr(), file_hhdm as *mut u8, kernel.len());

        copy_limine_requests(&elf, load_hhdm)?;
        let manifest_hhdm = loaded_addr(load_hhdm, elf.min_vaddr, elf.live_manifest.addr)?;
        let manifest = core::slice::from_raw_parts(manifest_hhdm as *const u64, 6);
        if manifest[0] != LIVE_MANIFEST_MAGIC0
            || manifest[1] != LIVE_MANIFEST_MAGIC1
            || manifest[2] != LIVE_ABI_VERSION
        {
            return Err(LiveUpdateError::Incompatible("missing live-update ABI manifest"));
        }
        let ap_entry = manifest[3];
        let handoff_addr = manifest[4];
        let handoff_size =
            usize::try_from(manifest[5]).map_err(|_| LiveUpdateError::ArithmeticOverflow)?;
        if handoff_size != size_of::<WarmHandoff>() {
            return Err(LiveUpdateError::Incompatible("handoff structure size mismatch"));
        }
        if !range_in_load(&elf.loads, ap_entry, 1, 0x1) {
            return Err(LiveUpdateError::Incompatible("AP entry is not in an executable PT_LOAD"));
        }
        if !range_in_load(&elf.loads, handoff_addr, handoff_size, 0x2) {
            return Err(LiveUpdateError::Incompatible("handoff slot is not in a writable PT_LOAD"));
        }
        let handoff_hhdm = loaded_addr(load_hhdm, elf.min_vaddr, handoff_addr)?;

        let trampoline_phys = arena_phys
            .checked_add(trampoline_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let trampoline_hhdm = hhdm
            .checked_add(trampoline_phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        ptr::write_bytes(trampoline_hhdm as *mut u8, 0, trampoline_map_len);
        ptr::copy_nonoverlapping(
            trampoline_start as *const u8,
            trampoline_hhdm as *mut u8,
            trampoline_len,
        );

        let table_phys = arena_phys
            .checked_add(tables_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let table_end = table_phys
            .checked_add(table_bytes as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let mut pool = TablePool {
            next_phys: table_phys,
            end_phys: table_end,
            hhdm,
        };
        let kernel_pdpt_phys = build_slot_page_tables(
            &mut pool,
            elf.min_vaddr,
            elf.max_vaddr,
            arena_phys + load_offset as u64,
        )?;
        let transition_pdpt_phys = build_slot_page_tables(
            &mut pool,
            transition_base,
            transition_base
                .checked_add(trampoline_map_len as u64)
                .ok_or(LiveUpdateError::ArithmeticOverflow)?,
            trampoline_phys,
        )?;

        let control_phys = arena_phys
            .checked_add(control_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let control_hhdm = hhdm
            .checked_add(control_phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let stack_phys = arena_phys
            .checked_add(stack_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let stack_base_hhdm = hhdm
            .checked_add(stack_phys)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let ap_park_transition = transition_base
            .checked_add(ap_park_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let bsp_commit_transition = transition_base
            .checked_add(bsp_commit_offset as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        let (transition_gdt, transition_gdtr) = snapshot_transition_gdt(control_hhdm)?;

        (control_hhdm as *mut TransitionControl).write(TransitionControl {
            command: AtomicU64::new(0),
            arrived: AtomicU64::new(0),
            stacked: AtomicU64::new(0),
            failures: AtomicU64::new(0),
            cr3,
            root_hhdm,
            kernel_slot: kernel_slot as u64,
            new_slot_entry: kernel_pdpt_phys | 0x003,
            transition_slot: transition_slot as u64,
            transition_slot_entry: transition_pdpt_phys | 0x003,
            stack_base_hhdm,
            stack_stride: AP_TRANSITION_STACK_BYTES as u64,
            bsp_entry: elf.entry,
            ap_entry,
            ap_park_hhdm: ap_park_transition,
            bsp_commit_hhdm: bsp_commit_transition,
            expected_aps: cpu_count.saturating_sub(1) as u64,
            transition_gdt,
            transition_gdtr,
        });

        let current_generation = warm_generation().unwrap_or(0);
        let mut handoff = WarmHandoff {
            magic0: HANDOFF_MAGIC0,
            magic1: HANDOFF_MAGIC1,
            abi_version: LIVE_ABI_VERSION,
            state: 0,
            generation: current_generation.saturating_add(1),
            candidate_hash: fnv1a64(kernel),
            arena_phys,
            arena_len: arena_len as u64,
            kernel_virt_base: elf.min_vaddr,
            kernel_phys_base: arena_phys + load_offset as u64,
            kernel_len: span as u64,
            kernel_file_phys: file_phys,
            kernel_file_len: kernel.len() as u64,
            hhdm_base: hhdm,
            expected_aps: cpu_count.saturating_sub(1) as u64,
            transition_slot: transition_slot as u64,
            vm_heap_ranges: [WarmReservedRange::EMPTY; VM_ID_LIMIT],
            restore_mask: [0; RESTORE_WORDS],
            resume_mask: [0; RESTORE_WORDS],
            checksum: 0,
        };
        handoff.checksum = handoff_checksum(&handoff);
        (handoff_hhdm as *mut WarmHandoff).write(handoff);

        Ok(StagedCandidate {
            arena_phys,
            arena_len,
            control_hhdm,
            handoff_hhdm,
            expected_aps: cpu_count.saturating_sub(1) as u64,
            transition_installed: false,
            committed: false,
        })
    })();

    if staged.is_err() {
        let _ = crate::phys::free_phys_range(arena_phys, arena_len);
    }
    staged
}

unsafe fn build_slot_page_tables(
    pool: &mut TablePool,
    min_vaddr: u64,
    max_vaddr: u64,
    load_phys: u64,
) -> Result<u64, LiveUpdateError> {
    let pdpt_phys = pool.alloc_zeroed()?;
    let mut vaddr = min_vaddr;
    while vaddr < max_vaddr {
        let pdpt_index = ((vaddr >> 30) & 0x1ff) as usize;
        let pd_index = ((vaddr >> 21) & 0x1ff) as usize;
        let pt_index = ((vaddr >> 12) & 0x1ff) as usize;
        let pd_phys = pool.child_table(pdpt_phys, pdpt_index)?;
        let pt_phys = pool.child_table(pd_phys, pd_index)?;
        let pt = pool.table_ptr(pt_phys)?;
        let page_phys = load_phys
            .checked_add(vaddr - min_vaddr)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        ptr::write_volatile(pt.add(pt_index), page_phys | 0x003);
        vaddr = vaddr
            .checked_add(PAGE_SIZE as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    }
    Ok(pdpt_phys)
}

unsafe fn copy_limine_requests(elf: &ParsedElf, load_hhdm: u64) -> Result<(), LiveUpdateError> {
    let current_start = ptr::addr_of!(__limine_requests_start) as usize;
    let current_end = ptr::addr_of!(__limine_requests_end) as usize;
    let current_len = current_end
        .checked_sub(current_start)
        .ok_or(LiveUpdateError::Incompatible("invalid current Limine request bounds"))?;
    if current_len != elf.limine_requests.size {
        return Err(LiveUpdateError::Incompatible(
            "Limine request layout changed; use a firmware reboot",
        ));
    }
    let destination = loaded_addr(load_hhdm, elf.min_vaddr, elf.limine_requests.addr)?;
    ptr::copy_nonoverlapping(current_start as *const u8, destination as *mut u8, current_len);
    Ok(())
}

#[repr(C, packed)]
struct RawDescriptorTablePointer {
    limit: u16,
    base: u64,
}

fn snapshot_transition_gdt(
    control_hhdm: u64,
) -> Result<([u8; MAX_TRANSITION_GDT_BYTES], [u8; 10]), LiveUpdateError> {
    let mut current = RawDescriptorTablePointer { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!(
            "sgdt [{}]",
            in(reg) ptr::addr_of_mut!(current),
            options(nostack, preserves_flags),
        );
    }
    let current_limit = unsafe { ptr::read_unaligned(ptr::addr_of!(current.limit)) };
    let current_base = unsafe { ptr::read_unaligned(ptr::addr_of!(current.base)) };
    let bytes = usize::from(current_limit).saturating_add(1);
    if bytes == 0 || bytes > MAX_TRANSITION_GDT_BYTES || current_base == 0 {
        return Err(LiveUpdateError::Incompatible("current GDT does not fit transition contract"));
    }
    let mut gdt = [0u8; MAX_TRANSITION_GDT_BYTES];
    unsafe {
        ptr::copy_nonoverlapping(current_base as *const u8, gdt.as_mut_ptr(), bytes);
    }
    let copied_base = control_hhdm
        .checked_add(offset_of!(TransitionControl, transition_gdt) as u64)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    let copied = RawDescriptorTablePointer {
        limit: (bytes - 1) as u16,
        base: copied_base,
    };
    let mut gdtr = [0u8; 10];
    unsafe {
        ptr::copy_nonoverlapping(ptr::addr_of!(copied).cast::<u8>(), gdtr.as_mut_ptr(), gdtr.len());
    }
    Ok((gdt, gdtr))
}

fn range_in_load(loads: &[LoadSegment], addr: u64, size: usize, required_flags: u32) -> bool {
    let Some(end) = addr.checked_add(size as u64) else {
        return false;
    };
    loads.iter().any(|load| {
        let Some(load_end) = load.vaddr.checked_add(load.mem_size as u64) else {
            return false;
        };
        addr >= load.vaddr && end <= load_end && (load.flags & required_flags) == required_flags
    })
}

fn parse_elf(bytes: &[u8]) -> Result<ParsedElf, LiveUpdateError> {
    if bytes.len() < 64 || bytes.get(0..4) != Some(b"\x7FELF") {
        return Err(LiveUpdateError::BadElf("missing ELF magic"));
    }
    if bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
        return Err(LiveUpdateError::BadElf("requires ELF64 little-endian v1"));
    }
    if read_u16(bytes, 16)? != 2 || read_u16(bytes, 18)? != 0x3e {
        return Err(LiveUpdateError::BadElf("requires x86_64 ET_EXEC"));
    }

    let entry = read_u64(bytes, 24)?;
    let phoff = usize_from_u64(read_u64(bytes, 32)?)?;
    let shoff = usize_from_u64(read_u64(bytes, 40)?)?;
    let phentsize = read_u16(bytes, 54)? as usize;
    let phnum = read_u16(bytes, 56)? as usize;
    let shentsize = read_u16(bytes, 58)? as usize;
    let shnum = read_u16(bytes, 60)? as usize;
    let shstrndx = read_u16(bytes, 62)? as usize;
    if phentsize < 56 || phnum == 0 {
        return Err(LiveUpdateError::BadElf("missing program headers"));
    }
    if shentsize < 64 || shnum == 0 || shstrndx >= shnum {
        return Err(LiveUpdateError::BadElf("section table is required"));
    }

    let mut loads = Vec::new();
    let mut min_vaddr = u64::MAX;
    let mut max_vaddr = 0u64;
    for index in 0..phnum {
        let base = checked_table_offset(phoff, phentsize, index, bytes.len())?;
        if read_u32(bytes, base)? != 1 {
            continue;
        }
        let flags = read_u32(bytes, base + 4)?;
        let offset = usize_from_u64(read_u64(bytes, base + 8)?)?;
        let vaddr = read_u64(bytes, base + 16)?;
        let file_size = usize_from_u64(read_u64(bytes, base + 32)?)?;
        let mem_size = usize_from_u64(read_u64(bytes, base + 40)?)?;
        if file_size > mem_size {
            return Err(LiveUpdateError::BadElf("PT_LOAD filesz exceeds memsz"));
        }
        checked_range(offset, file_size, bytes.len())?;
        let segment_end = vaddr
            .checked_add(mem_size as u64)
            .ok_or(LiveUpdateError::ArithmeticOverflow)?;
        min_vaddr = min_vaddr.min(vaddr & PAGE_MASK);
        max_vaddr = max_vaddr.max(
            align_up_u64(segment_end, PAGE_SIZE as u64)
                .ok_or(LiveUpdateError::ArithmeticOverflow)?,
        );
        loads.push(LoadSegment {
            vaddr,
            flags,
            offset,
            file_size,
            mem_size,
        });
    }
    if loads.is_empty() || min_vaddr >= max_vaddr {
        return Err(LiveUpdateError::BadElf("no usable PT_LOAD"));
    }
    if entry < min_vaddr || entry >= max_vaddr {
        return Err(LiveUpdateError::BadElf("entry is outside PT_LOAD span"));
    }
    if pml4_index(min_vaddr) != pml4_index(max_vaddr - 1) {
        return Err(LiveUpdateError::Incompatible("kernel spans more than one PML4 slot"));
    }

    let shstr_base = checked_table_offset(shoff, shentsize, shstrndx, bytes.len())?;
    let shstr_offset = usize_from_u64(read_u64(bytes, shstr_base + 24)?)?;
    let shstr_size = usize_from_u64(read_u64(bytes, shstr_base + 32)?)?;
    checked_range(shstr_offset, shstr_size, bytes.len())?;
    let shstr = &bytes[shstr_offset..shstr_offset + shstr_size];

    let mut limine_requests = None;
    let mut live_manifest = None;
    for index in 0..shnum {
        let base = checked_table_offset(shoff, shentsize, index, bytes.len())?;
        let name_offset = read_u32(bytes, base)? as usize;
        let section_type = read_u32(bytes, base + 4)?;
        let name = elf_string(shstr, name_offset)?;
        let addr = read_u64(bytes, base + 16)?;
        let offset = usize_from_u64(read_u64(bytes, base + 24)?)?;
        let size = usize_from_u64(read_u64(bytes, base + 32)?)?;
        if section_type != 8 {
            checked_range(offset, size, bytes.len())?;
        }
        let section = ElfSection { addr, size };
        match name {
            ".limine_requests" => limine_requests = Some(section),
            ".live_update_slot" => live_manifest = Some(section),
            _ => {}
        }
    }

    let limine_requests = limine_requests
        .ok_or(LiveUpdateError::Incompatible("candidate has no .limine_requests section"))?;
    let live_manifest = live_manifest
        .ok_or(LiveUpdateError::Incompatible("candidate has no .live_update_slot section"))?;
    if live_manifest.size < 6 * size_of::<u64>() {
        return Err(LiveUpdateError::Incompatible("live-update manifest is truncated"));
    }

    Ok(ParsedElf {
        entry,
        min_vaddr,
        max_vaddr,
        loads,
        limine_requests,
        live_manifest,
    })
}

fn warm_handoff() -> Option<&'static WarmHandoff> {
    let handoff = unsafe { &*ptr::addr_of!(LIVE_HANDOFF) };
    loop {
        match WARM_HANDOFF_VALIDATION.load(Ordering::Acquire) {
            HANDOFF_VALIDATION_VALID => return Some(handoff),
            HANDOFF_VALIDATION_INVALID => return None,
            HANDOFF_VALIDATION_UNCHECKED => {
                if WARM_HANDOFF_VALIDATION
                    .compare_exchange(
                        HANDOFF_VALIDATION_UNCHECKED,
                        HANDOFF_VALIDATION_CHECKING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    let valid = handoff.valid();
                    WARM_HANDOFF_VALIDATION.store(
                        if valid {
                            HANDOFF_VALIDATION_VALID
                        } else {
                            HANDOFF_VALIDATION_INVALID
                        },
                        Ordering::Release,
                    );
                    return valid.then_some(handoff);
                }
            }
            HANDOFF_VALIDATION_CHECKING => core::hint::spin_loop(),
            _ => {
                WARM_HANDOFF_VALIDATION.store(HANDOFF_VALIDATION_INVALID, Ordering::Release);
                return None;
            }
        }
    }
}

fn handoff_checksum(handoff: &WarmHandoff) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for value in [
        handoff.magic0,
        handoff.magic1,
        handoff.abi_version,
        handoff.state,
        handoff.generation,
        handoff.candidate_hash,
        handoff.arena_phys,
        handoff.arena_len,
        handoff.kernel_virt_base,
        handoff.kernel_phys_base,
        handoff.kernel_len,
        handoff.kernel_file_phys,
        handoff.kernel_file_len,
        handoff.hhdm_base,
        handoff.expected_aps,
        handoff.transition_slot,
    ] {
        hash = fnv1a64_value(hash, value);
    }
    for range in handoff.vm_heap_ranges {
        hash = fnv1a64_value(hash, range.phys_start);
        hash = fnv1a64_value(hash, range.length);
    }
    for value in handoff.restore_mask {
        hash = fnv1a64_value(hash, value);
    }
    for value in handoff.resume_mask {
        hash = fnv1a64_value(hash, value);
    }
    hash
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn fnv1a64_value(mut hash: u64, value: u64) -> u64 {
    for byte in value.to_le_bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    hash
}

fn checkpoint_name(vm_id: u8) -> String {
    format!("live-update-vm-{vm_id:02}")
}

fn mask_insert(mask: &mut [u64; RESTORE_WORDS], index: usize) {
    if let Some(word) = mask.get_mut(index / 64) {
        *word |= 1u64 << (index % 64);
    }
}

fn mask_contains(mask: &[u64; RESTORE_WORDS], index: usize) -> bool {
    mask.get(index / 64)
        .map(|word| (*word & (1u64 << (index % 64))) != 0)
        .unwrap_or(false)
}

fn pml4_index(addr: u64) -> usize {
    ((addr >> 39) & 0x1ff) as usize
}

fn loaded_addr(load_hhdm: u64, min_vaddr: u64, addr: u64) -> Result<u64, LiveUpdateError> {
    let offset = addr
        .checked_sub(min_vaddr)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    load_hhdm
        .checked_add(offset)
        .ok_or(LiveUpdateError::ArithmeticOverflow)
}

fn ceil_div(value: usize, divisor: usize) -> usize {
    value / divisor + usize::from(value % divisor != 0)
}

fn align_up_usize(value: usize, align: usize) -> Result<usize, LiveUpdateError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(LiveUpdateError::ArithmeticOverflow);
    }
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or(LiveUpdateError::ArithmeticOverflow)
}

fn align_up_u64(value: u64, align: u64) -> Option<u64> {
    if align == 0 || !align.is_power_of_two() {
        return None;
    }
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
}

fn usize_from_u64(value: u64) -> Result<usize, LiveUpdateError> {
    usize::try_from(value).map_err(|_| LiveUpdateError::ArithmeticOverflow)
}

fn checked_table_offset(
    base: usize,
    stride: usize,
    index: usize,
    total: usize,
) -> Result<usize, LiveUpdateError> {
    let offset = stride
        .checked_mul(index)
        .and_then(|offset| base.checked_add(offset))
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    checked_range(offset, stride, total)?;
    Ok(offset)
}

fn checked_range(offset: usize, length: usize, total: usize) -> Result<(), LiveUpdateError> {
    let end = offset
        .checked_add(length)
        .ok_or(LiveUpdateError::ArithmeticOverflow)?;
    if end > total {
        return Err(LiveUpdateError::BadElf("file range is out of bounds"));
    }
    Ok(())
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, LiveUpdateError> {
    let slice = bytes
        .get(offset..offset + 2)
        .ok_or(LiveUpdateError::BadElf("truncated u16"))?;
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, LiveUpdateError> {
    let slice = bytes
        .get(offset..offset + 4)
        .ok_or(LiveUpdateError::BadElf("truncated u32"))?;
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, LiveUpdateError> {
    let slice = bytes
        .get(offset..offset + 8)
        .ok_or(LiveUpdateError::BadElf("truncated u64"))?;
    Ok(u64::from_le_bytes([
        slice[0], slice[1], slice[2], slice[3], slice[4], slice[5], slice[6], slice[7],
    ]))
}

fn elf_string(table: &[u8], offset: usize) -> Result<&str, LiveUpdateError> {
    let tail = table
        .get(offset..)
        .ok_or(LiveUpdateError::BadElf("section-name offset is out of bounds"))?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(tail.len());
    core::str::from_utf8(&tail[..end])
        .map_err(|_| LiveUpdateError::BadElf("section-name table is not UTF-8"))
}
