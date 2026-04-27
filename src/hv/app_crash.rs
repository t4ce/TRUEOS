use alloc::{format, string::String};
use core::fmt::Write;

use crate::hv::vmx::{
    GuestRegisters, LaunchResult, VMCS_GUEST_CR0, VMCS_GUEST_CR3, VMCS_GUEST_CR4,
    VMCS_GUEST_IA32_EFER, VMCS_GUEST_LINEAR_ADDRESS, VMCS_GUEST_PHYSICAL_ADDRESS, VMCS_GUEST_RSP,
    VMCS_VMEXIT_INSTRUCTION_LEN, VMCS_VMEXIT_INTERRUPTION_ERROR_CODE,
    VMCS_VMEXIT_INTERRUPTION_INFO, vmread,
};

use super::{BlueprintLaunchState, hvlogf};

#[derive(Copy, Clone)]
pub enum CrashOutcome<'a> {
    Vmexit(LaunchResult),
    LaunchError(&'a str),
}

pub struct PendingCrashReport {
    pub path: String,
    pub report: String,
}

#[derive(Copy, Clone, Default)]
struct GuestCrashState {
    rsp: u64,
    cr0: u64,
    cr3: u64,
    cr4: u64,
    efer: u64,
    linear: u64,
    physical: u64,
    intr_info: u64,
    intr_error: u64,
    instr_len: u64,
    regs: GuestRegisters,
}

fn sanitize_path_component(input: &str) -> String {
    let mut out = String::new();
    for ch in input.chars() {
        let safe = matches!(ch, 'a'..='z' | 'A'..='Z' | '0'..='9' | '.' | '-' | '_');
        out.push(if safe { ch } else { '_' });
    }
    if out.is_empty() {
        out.push_str("blueprint");
    }
    out
}

fn module_hash64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in bytes {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn timestamp_seconds() -> u64 {
    crate::time::unix_time_seconds().unwrap_or_else(crate::time::uptime_seconds)
}

fn crash_path(state: &BlueprintLaunchState, vm_id: u8, timestamp: u64) -> String {
    let name = sanitize_path_component(state.archive.as_str());
    format!("appcrash/{}-{}-vm{}.txt", name.as_str(), timestamp, vm_id)
}

fn collect_guest_state() -> GuestCrashState {
    GuestCrashState {
        rsp: vmread(VMCS_GUEST_RSP).unwrap_or(0),
        cr0: vmread(VMCS_GUEST_CR0).unwrap_or(0),
        cr3: vmread(VMCS_GUEST_CR3).unwrap_or(0),
        cr4: vmread(VMCS_GUEST_CR4).unwrap_or(0),
        efer: vmread(VMCS_GUEST_IA32_EFER).unwrap_or(0),
        linear: vmread(VMCS_GUEST_LINEAR_ADDRESS).unwrap_or(0),
        physical: vmread(VMCS_GUEST_PHYSICAL_ADDRESS).unwrap_or(0),
        intr_info: vmread(VMCS_VMEXIT_INTERRUPTION_INFO).unwrap_or(0),
        intr_error: vmread(VMCS_VMEXIT_INTERRUPTION_ERROR_CODE).unwrap_or(0),
        instr_len: vmread(VMCS_VMEXIT_INSTRUCTION_LEN).unwrap_or(0),
        regs: crate::hv::vmx::guest_registers(),
    }
}

fn append_guest_state(report: &mut String, state: GuestCrashState) {
    let regs = state.regs;
    let _ = writeln!(
        report,
        "guest rsp=0x{:016X} cr0=0x{:016X} cr3=0x{:016X} cr4=0x{:016X} efer=0x{:016X}",
        state.rsp, state.cr0, state.cr3, state.cr4, state.efer
    );
    let _ = writeln!(
        report,
        "guest linear=0x{:016X} physical=0x{:016X} intr_info=0x{:08X} intr_error=0x{:X} instr_len={}",
        state.linear, state.physical, state.intr_info as u32, state.intr_error, state.instr_len
    );
    let _ = writeln!(
        report,
        "regs rax=0x{:016X} rbx=0x{:016X} rcx=0x{:016X} rdx=0x{:016X}",
        regs.rax, regs.rbx, regs.rcx, regs.rdx
    );
    let _ = writeln!(
        report,
        "regs rsi=0x{:016X} rdi=0x{:016X} rbp=0x{:016X}",
        regs.rsi, regs.rdi, regs.rbp
    );
    let _ = writeln!(
        report,
        "regs r8=0x{:016X} r9=0x{:016X} r10=0x{:016X} r11=0x{:016X}",
        regs.r8, regs.r9, regs.r10, regs.r11
    );
    let _ = writeln!(
        report,
        "regs r12=0x{:016X} r13=0x{:016X} r14=0x{:016X} r15=0x{:016X}",
        regs.r12, regs.r13, regs.r14, regs.r15
    );
}

fn build_report(vm_id: u8, state: &BlueprintLaunchState, outcome: CrashOutcome<'_>) -> String {
    let timestamp = timestamp_seconds();
    let mut report = String::new();
    let _ = writeln!(report, "TRUEOS app VM crash report");
    let _ = writeln!(report, "timestamp_seconds={}", timestamp);
    let _ = writeln!(report, "vm_id={}", vm_id);
    let _ = writeln!(report, "blueprint_archive={}", state.archive.as_str());
    let _ = writeln!(report, "app_args_count={}", state.app_args.len());
    for (idx, arg) in state.app_args.iter().enumerate() {
        let _ = writeln!(report, "app_arg{}={}", idx, arg.as_str());
    }
    let _ = writeln!(report, "module_bytes={}", state.module_bytes.len());
    let _ =
        writeln!(report, "module_hash64=0x{:016X}", module_hash64(state.module_bytes.as_slice()));

    match outcome {
        CrashOutcome::Vmexit(lr) => {
            let _ = writeln!(report, "kind=vmexit");
            let _ = writeln!(
                report,
                "entered={} launch_failed={} exit_reason=0x{:X} exit_qual=0x{:X} guest_rip=0x{:016X} instr_err={}",
                lr.entered,
                lr.launch_failed,
                lr.exit_reason,
                lr.exit_qualification,
                lr.guest_rip,
                lr.instr_err
            );
            append_guest_state(&mut report, collect_guest_state());
            let trace = crate::allocators::last_alloc_trace();
            if trace.seq != 0 {
                let _ = writeln!(
                    report,
                    "alloc_trace seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X}",
                    trace.seq,
                    trace.caller_rip,
                    trace.caller_rip_1,
                    trace.caller_rip_2,
                    trace.layout_size,
                    trace.layout_align,
                    trace.stage,
                    trace.head_ptr,
                    trace.block_ptr
                );
            }
        }
        CrashOutcome::LaunchError(err) => {
            let _ = writeln!(report, "kind=launch-error");
            let _ = writeln!(report, "error={}", err);
        }
    }

    let _ = writeln!(report, "symbolize_hint=addr2line -e TRUEOS.full.elf <guest_rip>");
    let _ = writeln!(report, "dwarf_stack=not-captured-yet");
    report
}

pub fn prepare(
    vm_id: u8,
    state: &BlueprintLaunchState,
    outcome: CrashOutcome<'_>,
) -> PendingCrashReport {
    let timestamp = timestamp_seconds();
    let path = crash_path(state, vm_id, timestamp);
    let report = build_report(vm_id, state, outcome);
    PendingCrashReport { path, report }
}

pub async fn write(vm_id: u8, pending: PendingCrashReport) {
    let path = pending.path;
    let report = pending.report;
    let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
        hvlogf(format_args!(
            "hv: vm{} appcrash no trueosfs root path={} bytes={}",
            vm_id,
            path.as_str(),
            report.len()
        ));
        return;
    };

    match crate::r::fs::trueosfs::file_in_async(disk, path.as_str(), report.as_bytes()).await {
        Ok(true) => hvlogf(format_args!(
            "hv: vm{} appcrash saved path={} bytes={}",
            vm_id,
            path.as_str(),
            report.len()
        )),
        Ok(false) => hvlogf(format_args!(
            "hv: vm{} appcrash write skipped path={} bytes={}",
            vm_id,
            path.as_str(),
            report.len()
        )),
        Err(e) => hvlogf(format_args!(
            "hv: vm{} appcrash write failed path={} err={:?}",
            vm_id,
            path.as_str(),
            e
        )),
    }
}
