use core::fmt;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use heapless::Vec;
use x86_64::registers::control::Cr2;
use x86_64::registers::control::{Cr0, Cr0Flags};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};
use x86_64::{instructions::hlt, instructions::interrupts};

static IDT: spin::Once<InterruptDescriptorTable> = spin::Once::new();
static IN_HANDLER: AtomicUsize = AtomicUsize::new(0);

#[inline(always)]
fn idt() -> &'static InterruptDescriptorTable {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        crate::chronos::interrupt_install(&mut idt);
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.device_not_available
            .set_handler_fn(device_not_available_handler);
        idt.general_protection_fault
            .set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
        idt.x87_floating_point
            .set_handler_fn(x87_floating_point_handler);
        idt.simd_floating_point
            .set_handler_fn(simd_floating_point_handler);
        idt.double_fault.set_handler_fn(double_fault_handler);
        idt
    })
}

struct DebugconWriter;

impl fmt::Write for DebugconWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            unsafe { crate::portio::outb(0xE9, b) };
        }
        Ok(())
    }
}

fn debugcon_print(args: fmt::Arguments<'_>) {
    let _ = fmt::write(&mut DebugconWriter, args);
}

macro_rules! dprintln {
    ($($tt:tt)*) => {
        debugcon_print(format_args!($($tt)*));
        debugcon_print(format_args!("\n"));
    };
}

macro_rules! faultln {
    ($($tt:tt)*) => {
        dprintln!($($tt)*);
        crate::hv::log_active_blueprint_console_line(format_args!($($tt)*));
    };
}

pub(crate) fn init() {
    load_this_cpu();
}

/// Load the exception IDT for the current CPU.
///
/// Note: `lidt` is per-CPU state, so APs must call this too.
pub(crate) fn load_this_cpu() {
    idt().load();
}

fn enter_handler_or_halt() {
    if IN_HANDLER.fetch_add(1, Ordering::SeqCst) != 0 {
        interrupts::disable();
        loop {
            hlt();
        }
    }
}

fn halt_loop() -> ! {
    interrupts::disable();
    loop {
        hlt();
    }
}

#[panic_handler]
fn panic(info: &PanicInfo<'_>) -> ! {
    enter_handler_or_halt();
    interrupts::disable();

    dprintln!("\n\x1b[31m=== KERNEL PANIC ===\x1b[0m");

    if let Some(loc) = info.location() {
        dprintln!("Location: {}:{}:{}", loc.file(), loc.line(), loc.column());
        crate::log!("Location: {}:{}:{}\n", loc.file(), loc.line(), loc.column());
    } else {
        crate::log!("Location: unknown\n");
    }

    let args = info.message();
    dprintln!("Reason: {}", args);
    crate::log!("Reason: {}\n", args);

    print_backtrace(64);

    if crate::cpu::can_restart_current_worker_ap_from_panic() {
        dprintln!("PANIC PANIC PANIC: restarting disposable worker AP");
        crate::cpu::restart_current_worker_ap_from_panic();
    }

    loop {
        hlt();
    }
}

fn log_fault_frame(label: &str, stack_frame: &InterruptStackFrame) {
    faultln!("\n=== {} ===", label);
    faultln!(
        "RIP={:#x} CS={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.code_segment.0
    );
    faultln!("RSP={:#x} SS={:#x}", stack_frame.stack_pointer.as_u64(), stack_frame.stack_segment.0);
    faultln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());
    faultln!(
        "CPU: lapic={} cpu={}",
        crate::percpu::this_cpu().lapic_id(),
        crate::percpu::this_cpu().cpu_index()
    );
}

fn log_fault_alloc_trace() {
    let trace = crate::allocators::last_alloc_trace();
    if trace.seq == 0 {
        return;
    }
    faultln!(
        "alloc-trace: seq={} caller=0x{:016X} caller1=0x{:016X} caller2=0x{:016X} size={} align={} stage={} head=0x{:016X} block=0x{:016X} block_size={} next=0x{:016X} payload=0x{:016X} aligned_used={}",
        trace.seq,
        trace.caller_rip,
        trace.caller_rip_1,
        trace.caller_rip_2,
        trace.layout_size,
        trace.layout_align,
        trace.stage,
        trace.head_ptr,
        trace.block_ptr,
        trace.block_size,
        trace.block_next,
        trace.payload_start,
        trace.aligned_used,
    );
}

#[inline]
fn is_canonical_addr(v: usize) -> bool {
    let sign = (v >> 47) & 1;
    let high = v >> 48;
    if sign == 0 { high == 0 } else { high == 0xFFFF }
}

fn dump_stack_words(sp: usize, words: usize) {
    if sp == 0 || !is_canonical_addr(sp) {
        dprintln!("stack dump: invalid sp=0x{:016x}", sp as u64);
        return;
    }
    let ptr = sp as *const usize;
    dprintln!("stack dump @0x{:016x}:", sp as u64);
    for i in 0..words {
        let p = unsafe { ptr.add(i) };
        let addr = p as usize;
        if !is_canonical_addr(addr) {
            break;
        }
        let v = unsafe { core::ptr::read_volatile(p) };
        dprintln!("  [rsp+0x{:02x}] = 0x{:016x}", i * core::mem::size_of::<usize>(), v as u64);
    }
}

/// Simple frame-pointer-based stack frame capture.
#[derive(Copy, Clone, Debug)]
pub struct Frame {
    pub rbp: usize,
    pub rip: usize,
}

const MAX_FRAMES: usize = 64;

/// Collect up to `max_frames` frames using the canonical x86_64 RBP chain.
/// Stops on null/zero RIP, non-forward RBP, or misaligned RBP to avoid loops.
pub fn collect_backtrace(max_frames: usize) -> Vec<Frame, MAX_FRAMES> {
    let limit = core::cmp::min(max_frames, MAX_FRAMES);

    let mut rbp: *const usize;
    unsafe {
        core::arch::asm!("mov {}, rbp", out(reg) rbp, options(nomem, nostack, preserves_flags));
    }

    let mut frames = Vec::<Frame, MAX_FRAMES>::new();
    while frames.len() < limit {
        if rbp.is_null() {
            break;
        }

        // Each frame: [saved_rbp, return_rip]. Bail if unreadable/corrupt.
        let saved_rbp = unsafe { core::ptr::read(rbp) } as usize;
        let ret_addr = unsafe { core::ptr::read(rbp.add(1)) } as usize;

        if ret_addr == 0 {
            break;
        }

        let _ = frames.push(Frame {
            rbp: rbp as usize,
            rip: ret_addr,
        });

        // Basic sanity: enforce forward progress and 16-byte alignment of caller frame.
        if saved_rbp <= rbp as usize {
            break;
        }
        if (saved_rbp & 0xF) != 0 {
            break;
        }

        rbp = saved_rbp as *const usize;
    }

    frames
}

/// Print a stack trace to debugcon and VGA log.
pub fn print_backtrace(max_frames: usize) {
    let frames = collect_backtrace(max_frames);
    crate::log!("stack trace ({} frames)\n", frames.len());
    for (idx, frame) in frames.iter().enumerate() {
        crate::log!("  #{:<2} rbp=0x{:016X} rip=0x{:016X}\n", idx, frame.rbp, frame.rip);
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    enter_handler_or_halt();
    interrupts::disable();

    log_fault_frame("#UD Invalid Opcode", &stack_frame);
    log_fault_alloc_trace();

    halt_loop();
}

extern "x86-interrupt" fn device_not_available_handler(stack_frame: InterruptStackFrame) {
    let mut cr0 = Cr0::read();
    if cr0.contains(Cr0Flags::TASK_SWITCHED) {
        cr0.remove(Cr0Flags::TASK_SWITCHED);
        unsafe { Cr0::write(cr0) };

        // Re-establish the default FP/SSE control state and continue.
        unsafe {
            core::arch::asm!("fninit", options(nostack, preserves_flags));
            let mxcsr: u32 = 0x1F80;
            core::arch::asm!(
                "ldmxcsr [{mxcsr_ptr}]",
                mxcsr_ptr = in(reg) &mxcsr,
                options(nostack, preserves_flags, readonly),
            );
        }
        return;
    }

    enter_handler_or_halt();
    interrupts::disable();
    log_fault_frame("#NM Device Not Available", &stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    enter_handler_or_halt();
    interrupts::disable();

    dprintln!("\n=== #GP General Protection Fault ===");
    dprintln!("error_code={:#x}", error_code);
    log_fault_frame("#GP General Protection Fault", &stack_frame);

    halt_loop();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    enter_handler_or_halt();
    interrupts::disable();

    dprintln!("\n=== #PF Page Fault ===");
    dprintln!("CR2={:#x}", Cr2::read_raw());
    dprintln!("error_code={:?}", error_code);
    dprintln!(
        "RIP={:#x} CS={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.code_segment.0
    );
    dprintln!(
        "RSP={:#x} SS={:#x}",
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());
    dprintln!(
        "CPU: lapic={} cpu={}",
        crate::percpu::this_cpu().lapic_id(),
        crate::percpu::this_cpu().cpu_index()
    );
    if stack_frame.instruction_pointer.as_u64() == 0 {
        dprintln!("hint: RIP=0 null instruction fetch (null fn ptr / clobbered return address)");
    }
    dump_stack_words(stack_frame.stack_pointer.as_u64() as usize, 16);

    halt_loop();
}

extern "x86-interrupt" fn x87_floating_point_handler(stack_frame: InterruptStackFrame) {
    enter_handler_or_halt();
    interrupts::disable();
    log_fault_frame("#MF x87 Floating-Point", &stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn simd_floating_point_handler(stack_frame: InterruptStackFrame) {
    enter_handler_or_halt();
    interrupts::disable();
    log_fault_frame("#XM SIMD Floating-Point", &stack_frame);
    halt_loop();
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) -> ! {
    enter_handler_or_halt();
    interrupts::disable();

    dprintln!("\n=== #DF Double Fault ===");
    dprintln!("error_code={:#x}", error_code);
    dprintln!(
        "RIP={:#x} CS={:#x}",
        stack_frame.instruction_pointer.as_u64(),
        stack_frame.code_segment.0
    );
    dprintln!(
        "RSP={:#x} SS={:#x}",
        stack_frame.stack_pointer.as_u64(),
        stack_frame.stack_segment.0
    );
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());

    halt_loop();
}
