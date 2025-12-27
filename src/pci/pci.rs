use core::arch::asm;

use heapless::Vec;
use spin::Mutex;

const CFG_ADDR: u16 = 0xCF8;
const CFG_DATA: u16 = 0xCFC;
const CFG_ENABLE: u32 = 0x8000_0000;

const MAX_PCI_DEVICES: usize = 256;

const PCI_COMMAND_MEM_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;

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
                record_device(
                    bus, slot, function, vendor, device, class, subclass, prog_if,
                );
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

fn write_u8(bus: u8, slot: u8, function: u8, offset: u8, value: u8) {
    let aligned_off = offset & !0x03;
    let shift = ((offset & 0x03) as u32) * 8;
    let mask = !(0xFFu32 << shift);

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

fn record_device(
    bus: u8,
    slot: u8,
    function: u8,
    vendor: u16,
    device: u16,
    class: u8,
    subclass: u8,
    prog_if: u8,
) {
    push_device(PciDevice {
        bus,
        slot,
        function,
        vendor,
        device,
        class,
        subclass,
        prog_if,
    });
}

fn push_device(dev: PciDevice) {
    let mut lock = DEVICES.lock();
    if lock.push(dev).is_err() {
        crate::debugconf!("pci device list full (>{})\n", MAX_PCI_DEVICES);
    }
}

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

pub fn with_devices<R, F: FnOnce(&[PciDevice]) -> R>(f: F) -> R {
    let lock = DEVICES.lock();
    f(lock.as_slice())
}

pub fn read_bar0_raw(bus: u8, slot: u8, function: u8) -> (u32, Option<u32>) {
    let bar_lo = read_u32(bus, slot, function, 0x10);
    if (bar_lo & 0x1) != 0 {
        return (bar_lo, None);
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    if is_64 {
        let bar_hi = read_u32(bus, slot, function, 0x14);
        (bar_lo, Some(bar_hi))
    } else {
        (bar_lo, None)
    }
}

pub fn enable_mem_and_bus_master(bus: u8, slot: u8, function: u8) {
    let mut cmd = read_u16(bus, slot, function, 0x04);
    cmd |= PCI_COMMAND_MEM_SPACE | PCI_COMMAND_BUS_MASTER;
    write_u16(bus, slot, function, 0x04, cmd);
}

fn normalize_offset(offset: u16) -> u8 {
    if offset > 0xFF {
        panic!("Extended PCI config offset 0x{:X} unsupported", offset);
    }
    offset as u8
}

pub fn config_read_u8(bus: u8, slot: u8, function: u8, offset: u16) -> u8 {
    read_u8(bus, slot, function, normalize_offset(offset))
}

pub fn config_read_u16(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    read_u16(bus, slot, function, normalize_offset(offset))
}

pub fn config_read_u32(bus: u8, slot: u8, function: u8, offset: u16) -> u32 {
    let off = normalize_offset(offset);
    if off & 0x3 != 0 {
        panic!("Unaligned PCI config dword read at offset 0x{:X}", offset);
    }
    read_u32(bus, slot, function, off)
}

pub fn config_write_u8(bus: u8, slot: u8, function: u8, offset: u16, value: u8) {
    write_u8(bus, slot, function, normalize_offset(offset), value);
}

pub fn config_write_u16(bus: u8, slot: u8, function: u8, offset: u16, value: u16) {
    write_u16(bus, slot, function, normalize_offset(offset), value);
}

pub fn config_write_u32(bus: u8, slot: u8, function: u8, offset: u16, value: u32) {
    let off = normalize_offset(offset);
    if off & 0x3 != 0 {
        panic!("Unaligned PCI config dword write at offset 0x{:X}", offset);
    }
    write_u32(bus, slot, function, off, value);
}
pub fn bar0_size_bytes(bus: u8, slot: u8, function: u8) -> Option<u64> {
    let orig_lo = read_u32(bus, slot, function, 0x10);
    if (orig_lo & 0x1) != 0 {
        return None;
    }

    let is_64 = ((orig_lo >> 1) & 0x3) == 0x2;
    let orig_hi = if is_64 {
        read_u32(bus, slot, function, 0x14)
    } else {
        0
    };

    write_u32(bus, slot, function, 0x10, 0xFFFF_FFF0);
    if is_64 {
        write_u32(bus, slot, function, 0x14, 0xFFFF_FFFF);
    }

    let mask_lo = read_u32(bus, slot, function, 0x10);
    let mask_hi = if is_64 {
        read_u32(bus, slot, function, 0x14)
    } else {
        0
    };

    write_u32(bus, slot, function, 0x10, orig_lo);
    if is_64 {
        write_u32(bus, slot, function, 0x14, orig_hi);
    }

    let mask = if is_64 {
        ((mask_hi as u64) << 32) | mask_lo as u64
    } else {
        mask_lo as u64
    };

    let size_mask = mask & !0xFu64;
    if size_mask == 0 {
        return None;
    }

    let size = (!size_mask).wrapping_add(1);
    Some(size)
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
