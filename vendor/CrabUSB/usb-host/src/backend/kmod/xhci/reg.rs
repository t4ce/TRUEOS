use core::{
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
    ptr::NonNull,
};

use mbarrier::mb;
use xhci::accessor::Mapper;

use super::SlotId;

#[derive(Debug, Clone, Copy)]
pub struct MemMapper;
impl Mapper for MemMapper {
    unsafe fn map(&mut self, phys_start: usize, _bytes: usize) -> NonZeroUsize {
        unsafe { NonZeroUsize::new_unchecked(phys_start) }
    }
    fn unmap(&mut self, _virt_start: usize, _bytes: usize) {}
}

type Registers = xhci::Registers<MemMapper>;
// type RegistersExtList = xhci::extended_capabilities::List<MemMapper>;
// type SupportedProtocol = xhci::extended_capabilities::XhciSupportedProtocol<MemMapper>;
pub(crate) type XhciRegistersShared = alloc::sync::Arc<spin::RwLock<XhciRegisters>>;

pub(crate) struct XhciRegisters {
    pub mmio_base: usize,
    reg: Registers,
}

impl Clone for XhciRegisters {
    fn clone(&self) -> Self {
        Self {
            mmio_base: self.mmio_base,
            reg: self.new_reg(),
        }
    }
}

impl XhciRegisters {
    const PORTSC_RO_MASK: u32 = (1 << 0) | (1 << 3) | (0x0f << 10) | (1 << 30);
    const PORTSC_RWS_MASK: u32 = (0x0f << 5) | (1 << 9) | (0x03 << 14) | (0x07 << 25);
    const PORTSC_CHANGE_MASK: u32 = 0x7f << 17;

    pub fn new(mmio_base: NonNull<u8>) -> Self {
        let mmio_base = mmio_base.as_ptr() as usize;
        let mapper = MemMapper {};
        let reg = unsafe { Registers::new(mmio_base, mapper) };
        Self { mmio_base, reg }
    }

    fn new_reg(&self) -> Registers {
        let mapper = MemMapper {};
        unsafe { Registers::new(self.mmio_base, mapper) }
    }

    /// Write PORTSC without replaying read-one-to-clear or read-one-to-set
    /// fields from the value sampled before the write.
    ///
    /// `set_bits` is reserved for deliberate actions such as PP/PR/WPR.
    /// `acknowledge_changes` selects only currently asserted RW1C bits.
    pub fn write_portsc_neutral(
        &mut self,
        index: usize,
        set_bits: u32,
        acknowledge_changes: u32,
    ) -> Option<(u32, u32, u32)> {
        if index >= self.port_register_set.len() {
            return None;
        }
        let caplength = usize::from(self.capability.caplength.read_volatile().get());
        let address = self.mmio_base + caplength + 0x400 + index * 0x10;
        let before = unsafe { core::ptr::read_volatile(address as *const u32) };
        let neutral = (before & Self::PORTSC_RO_MASK) | (before & Self::PORTSC_RWS_MASK);
        let requested =
            neutral | set_bits | (before & acknowledge_changes & Self::PORTSC_CHANGE_MASK);
        unsafe {
            core::ptr::write_volatile(address as *mut u32, requested);
        }
        mb();
        let after = unsafe { core::ptr::read_volatile(address as *const u32) };
        Some((before, requested, after))
    }

    pub fn disable_irq_guard(&mut self) -> DisableIrqGuard {
        let mut enable = true;
        self.operational.usbcmd.update_volatile(|r| {
            enable = r.interrupter_enable();
            r.clear_interrupter_enable();
        });
        DisableIrqGuard {
            reg: self.new_reg(),
            enable,
        }
    }
}

impl Deref for XhciRegisters {
    type Target = Registers;

    fn deref(&self) -> &Self::Target {
        &self.reg
    }
}

impl DerefMut for XhciRegisters {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.reg
    }
}

pub struct DisableIrqGuard {
    reg: Registers,
    enable: bool,
}
impl Drop for DisableIrqGuard {
    fn drop(&mut self) {
        if self.enable {
            self.reg.operational.usbcmd.update_volatile(|r| {
                r.set_interrupter_enable();
            });
        }
    }
}

pub struct SlotBell {
    slot_id: SlotId,
    reg: XhciRegisters,
}

impl SlotBell {
    pub fn new(slot_id: SlotId, reg: XhciRegisters) -> Self {
        Self { slot_id, reg }
    }

    pub fn ring(&mut self, bell: xhci::registers::doorbell::Register) {
        self.reg
            .doorbell
            .write_volatile_at(self.slot_id.as_usize(), bell);
    }
}
