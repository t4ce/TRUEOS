//! RTL8125B's legacy single-vector interrupt registers.
//!
//! Register/event definitions and mask/defer/rearm order follow Linux v6.12
//! drivers/net/ethernet/realtek/r8169_main.c: rtl8125_registers,
//! rtl_set_irq_mask, rtl8169_interrupt and rtl8169_poll.

pub(super) const MASK: usize = 0x38;
pub(super) const STATUS: usize = 0x3c;
pub(super) const OWNED_EVENTS: u32 = 0x2f;

pub(super) trait Registers {
    fn read(&mut self, offset: usize) -> u32;
    fn write(&mut self, offset: usize, value: u32);
}

pub(super) fn mask(registers: &mut impl Registers) {
    registers.write(MASK, 0);
    let _ = registers.read(MASK);
}

/// Return the owned causes. The ISR leaves delivery masked for task context.
pub(super) fn dispatch(registers: &mut impl Registers) -> Result<u32, ()> {
    let status = registers.read(STATUS);
    if status & 0xffff == 0xffff {
        mask(registers);
        return Err(());
    }
    let owned = status & OWNED_EVENTS;
    if owned != 0 {
        mask(registers);
        registers.write(STATUS, owned);
        let _ = registers.read(STATUS);
    }
    Ok(owned)
}

/// True means another task drain is necessary; delivery remains masked.
/// The driver must also check its descriptor rings after this readback.
pub(super) fn rearm(registers: &mut impl Registers, interrupted: impl FnOnce() -> bool) -> Result<bool, ()> {
    registers.write(MASK, OWNED_EVENTS);
    let enabled = registers.read(MASK);
    let status = registers.read(STATUS);
    if status & 0xffff == 0xffff {
        mask(registers);
        return Err(());
    }
    // An ISR can legitimately mask/ack between the arm write and readback.
    // Its generation or in-flight handler proves that this is readiness.
    if enabled == 0 && interrupted() { return Ok(true); }
    if enabled != OWNED_EVENTS { mask(registers); return Err(()); }
    if status & OWNED_EVENTS != 0 {
        mask(registers);
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Default)]
    struct Device { mask: u32, status: u32, acked: u32, arrive_on_arm: u32, interrupt_on_arm: bool, inaccessible: bool }
    impl Registers for Device {
        fn read(&mut self, offset: usize) -> u32 {
            if self.inaccessible { return u32::MAX; }
            match offset { MASK => self.mask, STATUS => self.status, _ => panic!("register") }
        }
        fn write(&mut self, offset: usize, value: u32) {
            match offset {
                MASK => { self.mask = value; if value != 0 { self.status |= self.arrive_on_arm; if self.interrupt_on_arm { self.mask = 0; self.status = 0; } } }
                STATUS => { self.acked |= value; self.status &= !value; }
                _ => panic!("register"),
            }
        }
    }
    #[test]
    fn interrupt_masks_before_ack_and_preserves_unowned_causes() {
        let mut device = Device { mask: OWNED_EVENTS, status: 0x85, ..Default::default() };
        assert_eq!(dispatch(&mut device), Ok(5));
        assert_eq!((device.mask, device.acked, device.status), (0, 5, 0x80));
    }
    #[test]
    fn interrupt_arriving_during_rearm_cannot_be_slept_through() {
        let mut device = Device { arrive_on_arm: 1, ..Default::default() };
        assert_eq!(rearm(&mut device, || false), Ok(true));
        assert_eq!(device.mask, 0);
        assert_eq!(device.status, 1);
    }
    #[test]
    fn quiet_rearm_preserves_only_owned_enable_bits() {
        let mut device = Device { status: 0x80, ..Default::default() };
        assert_eq!(rearm(&mut device, || false), Ok(false));
        assert_eq!(device.mask, OWNED_EVENTS);
        assert_eq!(device.status, 0x80);
    }
    #[test]
    fn inaccessible_device_remains_masked() {
        let mut device = Device { inaccessible: true, ..Default::default() };
        assert_eq!(dispatch(&mut device), Err(()));
        assert_eq!(device.mask, 0);
        assert_eq!(rearm(&mut device, || false), Err(()));
        assert_eq!(device.mask, 0);
    }
    #[test]
    fn isr_mask_and_ack_during_arm_is_readiness_before_or_after_generation_publication() {
        for (active_handlers, generation_changed) in [(1, false), (0, true)] {
            let mut device = Device { interrupt_on_arm: true, ..Default::default() };
            assert_eq!(rearm(&mut device, || active_handlers != 0 || generation_changed), Ok(true));
            assert_eq!(device.mask, 0);
        }
    }
}
