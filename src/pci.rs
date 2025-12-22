use core::arch::asm;

use heapless::Vec;
use spin::Mutex;

const CFG_ADDR: u16 = 0xCF8;
const CFG_DATA: u16 = 0xCFC;
const CFG_ENABLE: u32 = 0x8000_0000;

const MAX_PCI_DEVICES: usize = 256; // adjust if you need more than 256 entries

#[derive(Copy, Clone, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor: u16,
    pub device: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

static DEVICES: Mutex<Vec<PciDevice, MAX_PCI_DEVICES>> = Mutex::new(Vec::new());

pub fn enumerate_once() {
    crate::debugcon_write_str("pci: enumerate\n");

    // start fresh each time we enumerate
    DEVICES.lock().clear();

    for bus in 0u8..=255 {
        for slot in 0u8..32 {
            let vendor0 = read_u16(bus, slot, 0, 0x00);
            if vendor0 == 0xFFFF {
                continue;
            }

            let device0 = read_u16(bus, slot, 0, 0x02);
            let class0 = read_u8(bus, slot, 0, 0x0B);
            let subclass0 = read_u8(bus, slot, 0, 0x0A);
            let prog_if0 = read_u8(bus, slot, 0, 0x09);
            let header0 = read_u8(bus, slot, 0, 0x0E);
            record_device(bus, slot, 0, vendor0, device0, class0, subclass0, prog_if0);

            let functions = if (header0 & 0x80) != 0 { 8 } else { 1 };
            for function in 1..functions {
                let vendor = read_u16(bus, slot, function, 0x00);
                if vendor == 0xFFFF {
                    continue;
                }
                let device = read_u16(bus, slot, function, 0x02);
                let class = read_u8(bus, slot, function, 0x0B);
                let subclass = read_u8(bus, slot, function, 0x0A);
                let prog_if = read_u8(bus, slot, function, 0x09);
                record_device(bus, slot, function, vendor, device, class, subclass, prog_if);
            }
        }
    }

    crate::debugcon_write_str("pci: done\n");
}

fn cfg_address(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    CFG_ENABLE
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

fn read_u16(bus: u8, slot: u8, function: u8, offset: u8) -> u16 {
    let aligned = read_u32(bus, slot, function, offset & !0x03);
    let shift = ((offset & 0x03) as u32) * 8;
    ((aligned >> shift) & 0xFFFF) as u16
}

fn read_u8(bus: u8, slot: u8, function: u8, offset: u8) -> u8 {
    let aligned = read_u32(bus, slot, function, offset & !0x03);
    let shift = ((offset & 0x03) as u32) * 8;
    ((aligned >> shift) & 0xFF) as u8
}

fn write_u16(bus: u8, slot: u8, function: u8, offset: u8, value: u16) {
    let aligned_off = offset & !0x03;
    let shift = ((offset & 0x03) as u32) * 8;
    let mask = !(0xFFFFu32 << shift);

    let current = read_u32(bus, slot, function, aligned_off);
    let new_val = (current & mask) | ((value as u32) << shift);
    write_u32(bus, slot, function, aligned_off, new_val);
}

fn read_u32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    debug_assert_eq!(offset & 0x03, 0);
    let addr = cfg_address(bus, slot, function, offset);
    unsafe {
        outl(CFG_ADDR, addr);
        inl(CFG_DATA)
    }
}

fn write_u32(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    debug_assert_eq!(offset & 0x03, 0);
    let addr = cfg_address(bus, slot, function, offset);
    unsafe {
        outl(CFG_ADDR, addr);
        outl(CFG_DATA, value);
    }
}

fn record_device(bus: u8, slot: u8, function: u8, vendor: u16, device: u16, class: u8, subclass: u8, prog_if: u8) {
    push_device(PciDevice { bus, slot, function, vendor, device, class, subclass, prog_if });
}

fn push_device(dev: PciDevice) {
    let mut lock = DEVICES.lock();
    if lock.push(dev).is_err() {
        crate::debugconf!("pci device list full (>{})\n", MAX_PCI_DEVICES);
    }
}

/// Log all detected devices once, after enumeration has populated the list.
pub fn log_devices_once() {
    with_devices(|list| {
        for dev in list {
            crate::debugconf!(
                "pci {:02X}:{:02X}.{} vid={:04X} did={:04X} class={:02X}:{:02X}:{:02X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                dev.vendor,
                dev.device,
                dev.class,
                dev.subclass,
                dev.prog_if
            );
        }
    });
}

/// Iterate over detected devices without exposing the mutex. Useful for later registration.
pub fn with_devices<R, F: FnOnce(&[PciDevice]) -> R>(f: F) -> R {
    let lock = DEVICES.lock();
    f(lock.as_slice())
}

pub fn first_xhci() -> Option<(PciDevice, u64)> {
    with_devices(|list| {
        list.iter().find_map(|dev| {
            if dev.class == 0x0C && dev.subclass == 0x03 && dev.prog_if == 0x30 {
                read_bar0(dev.bus, dev.slot, dev.function).map(|bar| (*dev, bar))
            } else {
                None
            }
        })
    })
}

/// Enable MMIO decoding and bus mastering so the device will respond on BARs.
pub fn enable_mem_and_bus_master(bus: u8, slot: u8, function: u8) {
    let mut cmd = read_u16(bus, slot, function, 0x04);
    cmd |= 0x0006; // bit1 MEM space, bit2 bus master
    write_u16(bus, slot, function, 0x04, cmd);
}

fn read_bar0(bus: u8, slot: u8, function: u8) -> Option<u64> {
    let bar_lo = read_u32(bus, slot, function, 0x10);
    if (bar_lo & 0x1) != 0 {
        // I/O BAR not supported here
        return None;
    }
    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
    if is_64 {
        let bar_hi = read_u32(bus, slot, function, 0x14);
        base |= (bar_hi as u64) << 32;
    }
    Some(base)
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

