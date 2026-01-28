use core::arch::asm;

use heapless::Vec;
use spin::{Mutex, Once};

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

const ECAM_MAX_REGIONS: usize = 8;
const ECAM_BUS_WINDOW_SIZE: usize = 1 << 20; // 1MiB per bus

#[derive(Copy, Clone, Debug)]
struct EcamRegion {
    segment: u16,
    bus_start: u8,
    bus_end: u8,
    phys_base: u64,
}

struct EcamState {
    regions: Vec<EcamRegion, ECAM_MAX_REGIONS>,
    bus_cache_seg0: [Option<usize>; 256],
}

static ECAM: Once<Option<Mutex<EcamState>>> = Once::new();

fn init_ecam_once() {
    ECAM.call_once(|| {
        let Some(tables) = crate::efi::acpi::ensure_tables() else {
            return None;
        };

        let Some(mcfg) = tables.find_table::<acpi::sdt::mcfg::Mcfg>() else {
            crate::log!("pci: MCFG missing; using legacy cfg\n");
            return None;
        };

        let mut regions: Vec<EcamRegion, ECAM_MAX_REGIONS> = Vec::new();
        for entry in mcfg.entries() {
            let region = EcamRegion {
                segment: entry.pci_segment_group,
                bus_start: entry.bus_number_start,
                bus_end: entry.bus_number_end,
                phys_base: entry.base_address,
            };
            if regions.push(region).is_err() {
                crate::log!("pci: MCFG has too many regions; truncating\n");
                break;
            }
        }

        let mut seg0_count = 0usize;
        for r in regions.iter() {
            if r.segment == 0 {
                seg0_count += 1;
            }
        }
        crate::log!(
            "pci: MCFG present (regions={}, seg0={})\n",
            regions.len(),
            seg0_count
        );
        for r in regions.iter() {
            if r.segment == 0 {
                crate::log!(
                    "pci: ECAM seg0 base=0x{:X} bus={}-{}\n",
                    r.phys_base,
                    r.bus_start,
                    r.bus_end
                );
            }
        }

        Some(Mutex::new(EcamState {
            regions,
            bus_cache_seg0: [None; 256],
        }))
    });
}

fn ecam_region_seg0_for_bus(state: &EcamState, bus: u8) -> Option<EcamRegion> {
    for r in state.regions.iter() {
        if r.segment != 0 {
            continue;
        }
        if bus >= r.bus_start && bus <= r.bus_end {
            return Some(*r);
        }
    }
    None
}

fn ecam_bus_base_seg0(bus: u8) -> Option<usize> {
    init_ecam_once();
    let ecam = ECAM.get().and_then(|x| x.as_ref())?;

    let mut lock = ecam.lock();
    if let Some(virt) = lock.bus_cache_seg0[bus as usize] {
        return Some(virt);
    }

    let region = ecam_region_seg0_for_bus(&lock, bus)?;
    let bus_delta = (bus - region.bus_start) as u64;
    let phys_bus_base = region.phys_base + (bus_delta << 20);

    let virt = crate::pci::mmio::map_mmio_region_exact(phys_bus_base, ECAM_BUS_WINDOW_SIZE).ok()?;
    let virt_addr = virt.as_ptr() as usize;
    lock.bus_cache_seg0[bus as usize] = Some(virt_addr);
    Some(virt_addr)
}

fn ecam_read_u32(bus: u8, slot: u8, function: u8, aligned_off: u16) -> Option<u32> {
    if slot >= 32 || function >= 8 {
        return None;
    }
    if (aligned_off & 0x3) != 0 || aligned_off >= 0x1000 {
        return None;
    }

    let bus_base = ecam_bus_base_seg0(bus)?;
    let offset = ((slot as usize) << 15) | ((function as usize) << 12) | (aligned_off as usize);
    let ptr = (bus_base + offset) as *const u32;
    Some(unsafe { core::ptr::read_volatile(ptr) })
}

fn ecam_write_u32(bus: u8, slot: u8, function: u8, aligned_off: u16, value: u32) -> Option<()> {
    if slot >= 32 || function >= 8 {
        return None;
    }
    if (aligned_off & 0x3) != 0 || aligned_off >= 0x1000 {
        return None;
    }

    let bus_base = ecam_bus_base_seg0(bus)?;
    let offset = ((slot as usize) << 15) | ((function as usize) << 12) | (aligned_off as usize);
    let ptr = (bus_base + offset) as *mut u32;
    unsafe { core::ptr::write_volatile(ptr, value) };
    Some(())
}

fn enumerate_impl(log: bool) {
    if log {
        crate::log!("pci: enumerate\n");
    }

    DEVICES.lock().clear();

    for bus in 0u8..=255 {
        for slot in 0u8..32 {
            let vendor0 = config_read_u16(bus, slot, 0, 0x00);
            if vendor0 == 0xFFFF {
                continue;
            }

            let device0 = config_read_u16(bus, slot, 0, 0x02);
            let class0 = config_read_u8(bus, slot, 0, 0x0B);
            let subclass0 = config_read_u8(bus, slot, 0, 0x0A);
            let prog_if0 = config_read_u8(bus, slot, 0, 0x09);
            let header0 = config_read_u8(bus, slot, 0, 0x0E);
            record_device(bus, slot, 0, vendor0, device0, class0, subclass0, prog_if0);

            let functions = if (header0 & 0x80) != 0 { 8 } else { 1 };
            for function in 1..functions {
                let vendor = config_read_u16(bus, slot, function, 0x00);
                if vendor == 0xFFFF {
                    continue;
                }
                let device = config_read_u16(bus, slot, function, 0x02);
                let class = config_read_u8(bus, slot, function, 0x0B);
                let subclass = config_read_u8(bus, slot, function, 0x0A);
                let prog_if = config_read_u8(bus, slot, function, 0x09);
                record_device(
                    bus, slot, function, vendor, device, class, subclass, prog_if,
                );
            }
        }
    }

    if log {
        crate::log!("pci: done\n");
    }
}

pub fn enumerate_once() {
    enumerate_impl(true)
}

pub fn enumerate_silent() {
    enumerate_impl(false)
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
        crate::log!("pci device list full (>{})\n", MAX_PCI_DEVICES);
    }
}

pub fn log_devices_once() {
    with_devices(|list| {
        for dev in list {
            crate::log!(
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
    let bar_lo = config_read_u32(bus, slot, function, 0x10);
    if (bar_lo & 0x1) != 0 {
        return (bar_lo, None);
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    if is_64 {
        let bar_hi = config_read_u32(bus, slot, function, 0x14);
        (bar_lo, Some(bar_hi))
    } else {
        (bar_lo, None)
    }
}

pub fn enable_mem_and_bus_master(bus: u8, slot: u8, function: u8) {
    let mut cmd = config_read_u16(bus, slot, function, 0x04);
    cmd |= PCI_COMMAND_MEM_SPACE | PCI_COMMAND_BUS_MASTER;
    config_write_u16(bus, slot, function, 0x04, cmd);
}

fn normalize_offset(offset: u16) -> u8 {
    if offset > 0xFF {
        panic!("Extended PCI config offset 0x{:X} unsupported", offset);
    }
    offset as u8
}

pub fn config_read_u8(bus: u8, slot: u8, function: u8, offset: u16) -> u8 {
    if offset < 0x1000 {
        let aligned_off = (offset & !0x3) as u16;
        if let Some(aligned) = ecam_read_u32(bus, slot, function, aligned_off) {
            let shift = ((offset & 0x03) as u32) * 8;
            return ((aligned >> shift) & 0xFF) as u8;
        }
    }
    read_u8(bus, slot, function, normalize_offset(offset))
}

pub fn config_read_u16(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    if offset < 0x1000 {
        let aligned_off = (offset & !0x3) as u16;
        if let Some(aligned) = ecam_read_u32(bus, slot, function, aligned_off) {
            let shift = ((offset & 0x03) as u32) * 8;
            return ((aligned >> shift) & 0xFFFF) as u16;
        }
    }
    read_u16(bus, slot, function, normalize_offset(offset))
}

pub fn config_read_u32(bus: u8, slot: u8, function: u8, offset: u16) -> u32 {
    if (offset & 0x3) != 0 {
        panic!("Unaligned PCI config dword read at offset 0x{:X}", offset);
    }

    if offset < 0x1000 {
        if let Some(value) = ecam_read_u32(bus, slot, function, offset) {
            return value;
        }
    }

    let off = normalize_offset(offset);
    read_u32(bus, slot, function, off)
}

pub fn config_write_u8(bus: u8, slot: u8, function: u8, offset: u16, value: u8) {
    if offset < 0x1000 {
        let aligned_off = (offset & !0x3) as u16;
        if let Some(current) = ecam_read_u32(bus, slot, function, aligned_off) {
            let shift = ((offset & 0x03) as u32) * 8;
            let mask = !(0xFFu32 << shift);
            let new_val = (current & mask) | ((value as u32) << shift);
            if ecam_write_u32(bus, slot, function, aligned_off, new_val).is_some() {
                return;
            }
        }
    }
    write_u8(bus, slot, function, normalize_offset(offset), value);
}

pub fn config_write_u16(bus: u8, slot: u8, function: u8, offset: u16, value: u16) {
    if offset < 0x1000 {
        let aligned_off = (offset & !0x3) as u16;
        if let Some(current) = ecam_read_u32(bus, slot, function, aligned_off) {
            let shift = ((offset & 0x03) as u32) * 8;
            let mask = !(0xFFFFu32 << shift);
            let new_val = (current & mask) | ((value as u32) << shift);
            if ecam_write_u32(bus, slot, function, aligned_off, new_val).is_some() {
                return;
            }
        }
    }
    write_u16(bus, slot, function, normalize_offset(offset), value);
}

pub fn config_write_u32(bus: u8, slot: u8, function: u8, offset: u16, value: u32) {
    if (offset & 0x3) != 0 {
        panic!("Unaligned PCI config dword write at offset 0x{:X}", offset);
    }

    if offset < 0x1000 {
        if ecam_write_u32(bus, slot, function, offset, value).is_some() {
            return;
        }
    }

    let off = normalize_offset(offset);
    write_u32(bus, slot, function, off, value);
}
pub fn bar0_size_bytes(bus: u8, slot: u8, function: u8) -> Option<u64> {
    let orig_lo = config_read_u32(bus, slot, function, 0x10);
    if (orig_lo & 0x1) != 0 {
        return None;
    }

    let is_64 = ((orig_lo >> 1) & 0x3) == 0x2;
    let orig_hi = if is_64 {
        config_read_u32(bus, slot, function, 0x14)
    } else {
        0
    };

    config_write_u32(bus, slot, function, 0x10, 0xFFFF_FFF0);
    if is_64 {
        config_write_u32(bus, slot, function, 0x14, 0xFFFF_FFFF);
    }

    let mask_lo = config_read_u32(bus, slot, function, 0x10);
    let mask_hi = if is_64 {
        config_read_u32(bus, slot, function, 0x14)
    } else {
        0
    };

    config_write_u32(bus, slot, function, 0x10, orig_lo);
    if is_64 {
        config_write_u32(bus, slot, function, 0x14, orig_hi);
    }

    let mask = if is_64 {
        ((mask_hi as u64) << 32) | mask_lo as u64
    } else {
        mask_lo as u64
    };

    if is_64 {
        let size_mask = mask & !0xFu64;
        if size_mask == 0 {
            return None;
        }
        Some((!size_mask).wrapping_add(1))
    } else {
        // For 32-bit BARs, do the inversion in 32-bit space.
        let size_mask = (mask_lo & !0xFu32);
        if size_mask == 0 {
            return None;
        }
        Some(((!size_mask).wrapping_add(1)) as u64)
    }
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
