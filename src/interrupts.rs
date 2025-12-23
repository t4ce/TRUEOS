use spin::Once;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

use crate::debugcon_write_byte;

const USB_IRQ_VECTOR: u8 = 0x2B; // traditional IRQ11 after PIC remap

static IDT: Once<InterruptDescriptorTable> = Once::new();

pub fn install() {
    IDT.call_once(|| {
        let mut idt = InterruptDescriptorTable::new();
        idt[USB_IRQ_VECTOR].set_handler_fn(usb_irq_handler);
        idt
    });

    IDT.get().expect("IDT initialized").load();
}

extern "x86-interrupt" fn usb_irq_handler(_stack_frame: InterruptStackFrame) {
    debugcon_write_byte(b'!');
}
