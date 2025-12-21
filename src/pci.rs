use core::arch::asm;
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, Ordering};

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;
const CONFIG_ACCESS_ENABLE: u32 = 0x8000_0000;

static CONFIG_LOCK: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceLocation {
    bus: u8,
    slot: u8,
    function: u8,
}

impl DeviceLocation {
    pub fn new(bus: u8, slot: u8, function: u8) -> Option<Self> {
        if slot < 32 && function < 8 {
            Some(Self {
                bus,
                slot,
                function,
            })
        } else {
            None
        }
    }

    pub const fn bus(self) -> u8 {
        self.bus
    }

    pub const fn slot(self) -> u8 {
        self.slot
    }

    pub const fn function(self) -> u8 {
        self.function
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigAccessError {
}

#[derive(Clone, Copy, Debug)]
pub struct ConfigSpace;

impl ConfigSpace {
    pub const fn new() -> Self {
        Self
    }
    pub fn read_u16(&self, location: DeviceLocation, offset: u8) -> Result<u16, ConfigAccessError> {
        let aligned = offset & !0x03;
        let value = self.read_aligned_u32(location, aligned)?;
        let shift = ((offset & 0x03) as u32) * 8;
        Ok(((value >> shift) & 0xFFFF) as u16)
    }
 
    pub fn read_u8(&self, location: DeviceLocation, offset: u8) -> Result<u8, ConfigAccessError> {
        let aligned = offset & !0x03;
        let value = self.read_aligned_u32(location, aligned)?;
        let shift = ((offset & 0x03) as u32) * 8;
        Ok(((value >> shift) & 0xFF) as u8)
    }

    fn read_aligned_u32(
        &self,
        location: DeviceLocation,
        offset: u8,
    ) -> Result<u32, ConfigAccessError> {
        debug_assert_eq!(offset & 0x03, 0);
        let address = compose_address(location, offset);
        let _guard = ConfigLockGuard::lock();
        unsafe {
            outl(CONFIG_ADDRESS_PORT, address);
            Ok(inl(CONFIG_DATA_PORT))
        }
    }
}

static CONFIG: ConfigSpace = ConfigSpace::new();

fn compose_address(location: DeviceLocation, offset: u8) -> u32 {
    CONFIG_ACCESS_ENABLE
        | ((location.bus as u32) << 16)
        | ((location.slot as u32) << 11)
        | ((location.function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

struct ConfigLockGuard;

impl ConfigLockGuard {
    #[inline(always)]
    fn lock() -> Self {
        while CONFIG_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            spin_loop();
        }
        Self
    }
}

impl Drop for ConfigLockGuard {
    fn drop(&mut self) {
        CONFIG_LOCK.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct PciFunction {
    bus: u8,
    slot: u8,
    function: u8,
    vendor_id: u16,
    device_id: u16,
    class_code: u8,
    subclass: u8,
    prog_if: u8,
    header_type: u8,
}

fn read_function(location: DeviceLocation) -> Option<PciFunction> {
    let vendor_id = CONFIG.read_u16(location, 0x00).ok()?;
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = CONFIG.read_u16(location, 0x02).ok()?;
    let prog_if = CONFIG.read_u8(location, 0x09).ok()?;
    let subclass = CONFIG.read_u8(location, 0x0A).ok()?;
    let class_code = CONFIG.read_u8(location, 0x0B).ok()?;
    let header_type = CONFIG.read_u8(location, 0x0E).ok()?;

    Some(PciFunction {
        bus: location.bus(),
        slot: location.slot(),
        function: location.function(),
        vendor_id,
        device_id,
        class_code,
        subclass,
        prog_if,
        header_type,
    })
}

#[embassy_executor::task]
pub async fn pci_enumerate_task() {
    crate::debugcon_write_str("pci: enumerate\n");

    let mut xhci_loc: Option<DeviceLocation> = None;

    for bus in 0u8..=255 {
        for slot in 0u8..32 {
            let Some(loc0) = DeviceLocation::new(bus, slot, 0) else {
                continue;
            };
            let Some(func0) = read_function(loc0) else {
                continue;
            };

            log_func(&func0);
            if xhci_loc.is_none() && is_xhci(&func0) {
                xhci_loc = Some(loc0);
            }

            let functions = if (func0.header_type & 0x80) != 0 { 8 } else { 1 };
            for function in 1..functions {
                let Some(loc) = DeviceLocation::new(bus, slot, function) else {
                    continue;
                };
                if let Some(func) = read_function(loc) {
                    log_func(&func);
                    if xhci_loc.is_none() && is_xhci(&func) {
                        xhci_loc = Some(loc);
                    }
                }
            }
        }
    }

    crate::debugcon_write_str("pci: done\n");
}

fn log_func(func: &PciFunction) {
    crate::debugcon_write_str("pci ");
    write_hex_u8(func.bus);
    crate::debugcon_write_byte(b':');
    write_hex_u8(func.slot);
    crate::debugcon_write_byte(b'.');
    write_hex_u8(func.function);

    crate::debugcon_write_str(" vid=");
    write_hex_u16(func.vendor_id);
    crate::debugcon_write_str(" did=");
    write_hex_u16(func.device_id);
    crate::debugcon_write_str(" class=");
    write_hex_u8(func.class_code);
    crate::debugcon_write_byte(b':');
    write_hex_u8(func.subclass);
    crate::debugcon_write_byte(b':');
    write_hex_u8(func.prog_if);
    crate::debugcon_write_byte(b'\n');
}

fn is_xhci(func: &PciFunction) -> bool {
    func.class_code == 0x0C && func.subclass == 0x03 && func.prog_if == 0x30
}

#[inline(always)]
unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    asm!("in eax, dx", in("dx") port, out("eax") val, options(nomem, nostack, preserves_flags));
    val
}

#[inline(always)]
fn write_hex_u8(v: u8) {
    write_hex_nibble(v >> 4);
    write_hex_nibble(v & 0x0F);
}

#[inline(always)]
fn write_hex_u16(v: u16) {
    write_hex_u8((v >> 8) as u8);
    write_hex_u8(v as u8);
}

#[inline(always)]
fn write_hex_nibble(v: u8) {
    let v = v & 0x0F;
    let c = if v < 10 { b'0' + v } else { b'A' + (v - 10) };
    crate::debugcon_write_byte(c);
}
