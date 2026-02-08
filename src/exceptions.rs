use core::fmt;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use heapless::Vec;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{
    InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode,
};
use x86_64::{instructions::hlt, instructions::interrupts};

static IDT: spin::Once<InterruptDescriptorTable> = spin::Once::new();
static IN_HANDLER: AtomicUsize = AtomicUsize::new(0);

#[inline(always)]
fn idt() -> &'static InterruptDescriptorTable {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        idt.general_protection_fault
            .set_handler_fn(general_protection_fault_handler);
        idt.page_fault.set_handler_fn(page_fault_handler);
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
        crate::log!(
            "  #{:<2} rbp=0x{:016X} rip=0x{:016X}\n",
            idx,
            frame.rbp,
            frame.rip
        );
    }
}

extern "x86-interrupt" fn invalid_opcode_handler(stack_frame: InterruptStackFrame) {
    enter_handler_or_halt();
    interrupts::disable();

    dprintln!("\n=== #UD Invalid Opcode ===");
    dprintln!("RIP={:#x} CS={:#x}", stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment.0);
    dprintln!("RSP={:#x} SS={:#x}", stack_frame.stack_pointer.as_u64(), stack_frame.stack_segment.0);
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());

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
    dprintln!("RIP={:#x} CS={:#x}", stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment.0);
    dprintln!("RSP={:#x} SS={:#x}", stack_frame.stack_pointer.as_u64(), stack_frame.stack_segment.0);
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());

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
    dprintln!("RIP={:#x} CS={:#x}", stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment.0);
    dprintln!("RSP={:#x} SS={:#x}", stack_frame.stack_pointer.as_u64(), stack_frame.stack_segment.0);
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());

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
    dprintln!("RIP={:#x} CS={:#x}", stack_frame.instruction_pointer.as_u64(), stack_frame.code_segment.0);
    dprintln!("RSP={:#x} SS={:#x}", stack_frame.stack_pointer.as_u64(), stack_frame.stack_segment.0);
    dprintln!("RFLAGS={:#x}", stack_frame.cpu_flags.bits());

    halt_loop();
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    unsafe { core::arch::asm!("cli", options(nomem, nostack)) };
    print_backtrace(64);
    let mut counter: u64 = 0;
    loop {
        counter = counter.wrapping_add(1);
        if counter % 100_000_000 == 0 {
            crate::globalog::debugcon_write_byte_raw(b'!');
        }
    }
}
