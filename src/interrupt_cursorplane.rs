// Currently this interrupt only logs a marker, but it is intended as a ready-to-use
// hook for future cursor-plane or similar deferred work.
// i think this comment is a LIE because well, it interfered with GFX and UI2 + loadscreen bad when inited in main.rs
use x86_64::structures::idt::InterruptStackFrame;

const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;
const PIC_EOI: u8 = 0x20;

const PIC1_OFFSET: u8 = 32;
const PIC2_OFFSET: u8 = 40;
pub(crate) const INTERRUPT_CURSORPLANE_VECTOR: u8 = PIC2_OFFSET;

const CMOS_ADDR: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;
const CMOS_NMI_DISABLE: u8 = 0x80;
const RTC_REG_B: u8 = 0x0B;
const RTC_REG_C: u8 = 0x0C;
const RTC_REG_B_UIE: u8 = 1 << 4;

#[inline(always)]
unsafe fn cmos_read(reg: u8) -> u8 {
    crate::portio::outb(CMOS_ADDR, CMOS_NMI_DISABLE | reg);
    crate::portio::inb(CMOS_DATA)
}

#[inline(always)]
unsafe fn cmos_write(reg: u8, value: u8) {
    crate::portio::outb(CMOS_ADDR, CMOS_NMI_DISABLE | reg);
    crate::portio::outb(CMOS_DATA, value);
}

#[inline(always)]
unsafe fn pic_remap(master_offset: u8, slave_offset: u8) {
    let master_mask = crate::portio::inb(PIC1_DATA);
    let slave_mask = crate::portio::inb(PIC2_DATA);

    crate::portio::outb(PIC1_CMD, 0x11);
    crate::portio::outb(PIC2_CMD, 0x11);

    crate::portio::outb(PIC1_DATA, master_offset);
    crate::portio::outb(PIC2_DATA, slave_offset);

    crate::portio::outb(PIC1_DATA, 0x04);
    crate::portio::outb(PIC2_DATA, 0x02);

    crate::portio::outb(PIC1_DATA, 0x01);
    crate::portio::outb(PIC2_DATA, 0x01);

    crate::portio::outb(PIC1_DATA, master_mask);
    crate::portio::outb(PIC2_DATA, slave_mask);
}

pub(crate) fn init_bsp() {
    unsafe {
        pic_remap(PIC1_OFFSET, PIC2_OFFSET);

        // Unmask only the cascade line on the master and IRQ8 on the slave.
        crate::portio::outb(PIC1_DATA, 0xFB);
        crate::portio::outb(PIC2_DATA, 0xFE);

        let reg_b = cmos_read(RTC_REG_B);
        cmos_write(RTC_REG_B, reg_b | RTC_REG_B_UIE);

        // Reading register C acknowledges any pending RTC interrupt source.
        let _ = cmos_read(RTC_REG_C);
    }

    x86_64::instructions::interrupts::enable();
}

#[allow(non_snake_case)]
pub(crate) extern "x86-interrupt" fn INTERRUPT_CURSORPLANE(_stack_frame: InterruptStackFrame) {
    unsafe {
        // Reading register C acknowledges the RTC update-ended interrupt.
        let _ = cmos_read(RTC_REG_C);
        crate::portio::outb(0xE9, b'?');
        crate::portio::outb(PIC2_CMD, PIC_EOI);
        crate::portio::outb(PIC1_CMD, PIC_EOI);
    }
}
