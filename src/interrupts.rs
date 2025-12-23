use core::fmt::Debug;

use spin::Once;
use x86_64::instructions::hlt;
use x86_64::instructions::interrupts;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{debugconf, gdt};

static IDT: Once<InterruptDescriptorTable> = Once::new();

pub fn install() {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt.page_fault.set_handler_fn(page_fault_handler);
        unsafe {
            idt.double_fault
                .set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        idt
    });

    IDT.get().expect("IDT initialized").load();
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    interrupts::disable();

    let addr = match Cr2::read() {
        Ok(virt) => virt.as_u64(),
        Err(err) => {
            debugconf!("CR2 read error: {:?}\n", err);
            0
        }
    };
    debugconf!(
        "EXCEPTION: PAGE FAULT addr=0x{:X} error={:?} stack={:?}\n",
        addr,
        error_code,
        DisplayStack(&stack_frame)
    );

    halt_forever();
}

extern "x86-interrupt" fn double_fault_handler(stack_frame: InterruptStackFrame, _error: u64) -> ! {
    interrupts::disable();
    debugconf!(
        "EXCEPTION: DOUBLE FAULT stack={:?}\n",
        DisplayStack(&stack_frame)
    );
    halt_forever();
}

fn halt_forever() -> ! {
    loop {
        hlt();
    }
}

struct DisplayStack<'a>(&'a InterruptStackFrame);

impl Debug for DisplayStack<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InterruptStackFrame")
            .field("instruction_pointer", &self.0.instruction_pointer)
            .field("code_segment", &self.0.code_segment)
            .field("cpu_flags", &self.0.cpu_flags)
            .field("stack_pointer", &self.0.stack_pointer)
            .field("stack_segment", &self.0.stack_segment)
            .finish()
    }
}
