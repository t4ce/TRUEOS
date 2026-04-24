#[cfg(target_arch = "x86_64")]
use core::arch::asm;
#[cfg(target_arch = "x86_64")]
use x86_64::instructions::interrupts;

use heapless::Vec;
use spin::{Mutex, Once};

// PCI config access direction:
// - Legacy CF8/CFC is a single global address+data window, so accesses must be serialized.
// - Keep that serialization coarse for now so sub-dword read-modify-write helpers stay correct.
// - On x86_64 we also mask local CPU interrupts while holding the legacy lock to avoid
//   deadlocking against an interrupt handler re-entering CF8/CFC on the same core.
// - Future cleanup: preserve this conservative legacy fallback, but let ECAM-backed config
//   accesses stay parallel so independent PCI probe / hotplug / driver bring-up can fan out
//   without reintroducing CF8/CFC races.

const CFG_ADDR: u16 = 0xCF8;
const CFG_DATA: u16 = 0xCFC;
const CFG_ENABLE: u32 = 0x8000_0000;

const MAX_PCI_DEVICES: usize = 256;

const PCI_COMMAND_IO_SPACE: u16 = 1 << 0;
const PCI_COMMAND_MEM_SPACE: u16 = 1 << 1;
const PCI_COMMAND_BUS_MASTER: u16 = 1 << 2;
const PCI_STATUS_CAP_LIST: u16 = 1 << 4;

const PCI_CAP_PTR: u16 = 0x34;
const PCI_CAP_ID_PCI_EXPRESS: u8 = 0x10;
const PCI_EXP_DEVCAP: u16 = 0x04;
const PCI_EXP_DEVCTL: u16 = 0x08;
const PCI_EXP_DEVCAP_FLR: u32 = 1 << 28;
const PCI_EXP_DEVCTL_BCR_FLR: u16 = 1 << 15;

// PCI-to-PCI Bridge class/subclass
const PCI_CLASS_BRIDGE: u8 = 0x06;
const PCI_SUBCLASS_PCI_TO_PCI: u8 = 0x04;

// Bridge window registers (Type 1 header)
const PCI_BRIDGE_BUS_NUMBERS: u16 = 0x18;
const PCI_BRIDGE_MEM_BASE_LIMIT: u16 = 0x20;

const MAX_BRIDGE_ALLOCS: usize = 32;

static LEGACY_CFG_LOCK: Mutex<()> = Mutex::new(());

#[derive(Copy, Clone, Debug)]
struct BridgeAlloc {
    bus: u8,
    slot: u8,
    function: u8,
    base: u64,
    next_top: u64,
    limit_excl: u64,
}

static BRIDGE_ALLOCS: Mutex<Vec<BridgeAlloc, MAX_BRIDGE_ALLOCS>> = Mutex::new(Vec::new());

pub mod class {
    pub const UNCLASSIFIED: u8 = 0x00;
    pub const MASS_STORAGE: u8 = 0x01;
    pub const NETWORK: u8 = 0x02;
    pub const DISPLAY: u8 = 0x03;
    pub const MULTIMEDIA: u8 = 0x04;
    pub const MEMORY: u8 = 0x05;
    pub const BRIDGE: u8 = 0x06;
    pub const SERIAL_BUS: u8 = 0x0C;
}

#[derive(Copy, Clone, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub vendor: u16,
    pub device: u16,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
}

impl PciDevice {
    /// Return the decoded BAR address for `index`.
    ///
    /// For MMIO BARs this masks off the low attribute bits.
    /// For 64-bit BARs this combines the low/high dwords.
    pub fn bar_address(&self, index: usize) -> Option<u64> {
        if index >= 6 {
            return None;
        }

        let (bar_lo, bar_hi) = read_bar_raw(self.bus, self.slot, self.function, index as u8);
        if bar_lo == 0 {
            return None;
        }

        if (bar_lo & 0x1) != 0 {
            return Some((bar_lo & !0x3) as u64);
        }

        let addr_lo = (bar_lo & !0xFu32) as u64;
        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        if is_64 {
            Some(((bar_hi.unwrap_or(0) as u64) << 32) | addr_lo)
        } else {
            Some(addr_lo)
        }
    }

    /// Returns true when BAR `index` is present and memory-mapped.
    pub fn bar_is_memory(&self, index: usize) -> bool {
        if index >= 6 {
            return false;
        }

        let (bar_lo, _) = read_bar_raw(self.bus, self.slot, self.function, index as u8);
        bar_lo != 0 && (bar_lo & 0x1) == 0
    }

    /// Returns true when BAR `index` is present and I/O-port based.
    pub fn bar_is_io(&self, index: usize) -> bool {
        if index >= 6 {
            return false;
        }

        let (bar_lo, _) = read_bar_raw(self.bus, self.slot, self.function, index as u8);
        bar_lo != 0 && (bar_lo & 0x1) != 0
    }
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
        if crate::logflag::BOOT_INFO_LOGS {
            crate::log!("pci: MCFG present (regions={}, seg0={})\n", regions.len(), seg0_count);
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

pub fn enumerate_impl() {
    let mut new_devices: Vec<PciDevice, MAX_PCI_DEVICES> = Vec::new();

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

            if new_devices
                .push(PciDevice {
                    bus,
                    slot,
                    function: 0,
                    vendor: vendor0,
                    device: device0,
                    vendor_id: vendor0,
                    device_id: device0,
                    class: class0,
                    subclass: subclass0,
                    prog_if: prog_if0,
                })
                .is_err()
            {
                crate::log!("pci device list full (>{})\n", MAX_PCI_DEVICES);
                break;
            }

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
                if new_devices
                    .push(PciDevice {
                        bus,
                        slot,
                        function,
                        vendor,
                        device,
                        vendor_id: vendor,
                        device_id: device,
                        class,
                        subclass,
                        prog_if,
                    })
                    .is_err()
                {
                    crate::log!("pci device list full (>{})\n", MAX_PCI_DEVICES);
                    break;
                }
            }
        }
    }

    *DEVICES.lock() = new_devices;
}

fn cfg_address(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    CFG_ENABLE
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((function as u32) << 8)
        | ((offset as u32) & 0xFC)
}

#[inline(always)]
fn with_legacy_cfg_lock<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "x86_64")]
    {
        interrupts::without_interrupts(|| {
            let _guard = LEGACY_CFG_LOCK.lock();
            f()
        })
    }

    #[cfg(not(target_arch = "x86_64"))]
    {
        let _guard = LEGACY_CFG_LOCK.lock();
        f()
    }
}

#[cfg(not(target_arch = "x86_64"))]
#[cold]
fn legacy_cfg_unsupported() -> ! {
    panic!(
        "pci: legacy CF8/CFC config access is only supported on x86_64; non-x86 needs ECAM/platform PCI glue"
    )
}

fn read_u16(bus: u8, slot: u8, function: u8, offset: u8) -> u16 {
    with_legacy_cfg_lock(|| {
        let aligned = read_u32_unlocked(bus, slot, function, offset & !0x03);
        let shift = ((offset & 0x03) as u32) * 8;
        ((aligned >> shift) & 0xFFFF) as u16
    })
}

fn read_u8(bus: u8, slot: u8, function: u8, offset: u8) -> u8 {
    with_legacy_cfg_lock(|| {
        let aligned = read_u32_unlocked(bus, slot, function, offset & !0x03);
        let shift = ((offset & 0x03) as u32) * 8;
        ((aligned >> shift) & 0xFF) as u8
    })
}

fn write_u16(bus: u8, slot: u8, function: u8, offset: u8, value: u16) {
    with_legacy_cfg_lock(|| {
        let aligned_off = offset & !0x03;
        let shift = ((offset & 0x03) as u32) * 8;
        let mask = !(0xFFFFu32 << shift);

        let current = read_u32_unlocked(bus, slot, function, aligned_off);
        let new_val = (current & mask) | ((value as u32) << shift);
        write_u32_unlocked(bus, slot, function, aligned_off, new_val);
    })
}

fn write_u8(bus: u8, slot: u8, function: u8, offset: u8, value: u8) {
    with_legacy_cfg_lock(|| {
        let aligned_off = offset & !0x03;
        let shift = ((offset & 0x03) as u32) * 8;
        let mask = !(0xFFu32 << shift);

        let current = read_u32_unlocked(bus, slot, function, aligned_off);
        let new_val = (current & mask) | ((value as u32) << shift);
        write_u32_unlocked(bus, slot, function, aligned_off, new_val);
    })
}

fn read_u32(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    debug_assert_eq!(offset & 0x03, 0);
    with_legacy_cfg_lock(|| read_u32_unlocked(bus, slot, function, offset))
}

fn read_u32_unlocked(bus: u8, slot: u8, function: u8, offset: u8) -> u32 {
    debug_assert_eq!(offset & 0x03, 0);
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (bus, slot, function, offset);
        legacy_cfg_unsupported();
    }

    #[cfg(target_arch = "x86_64")]
    let addr = cfg_address(bus, slot, function, offset);
    #[cfg(target_arch = "x86_64")]
    unsafe {
        outl(CFG_ADDR, addr);
        inl(CFG_DATA)
    }
}

fn write_u32(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    debug_assert_eq!(offset & 0x03, 0);
    with_legacy_cfg_lock(|| write_u32_unlocked(bus, slot, function, offset, value))
}

fn write_u32_unlocked(bus: u8, slot: u8, function: u8, offset: u8, value: u32) {
    debug_assert_eq!(offset & 0x03, 0);
    #[cfg(not(target_arch = "x86_64"))]
    {
        let _ = (bus, slot, function, offset, value);
        legacy_cfg_unsupported();
    }

    #[cfg(target_arch = "x86_64")]
    let addr = cfg_address(bus, slot, function, offset);
    #[cfg(target_arch = "x86_64")]
    unsafe {
        outl(CFG_ADDR, addr);
        outl(CFG_DATA, value);
    }
}

pub fn with_devices<R, F: FnOnce(&[PciDevice]) -> R>(f: F) -> R {
    let lock = DEVICES.lock();
    f(lock.as_slice())
}

pub fn find_by_class(class: u8) -> alloc::vec::Vec<PciDevice> {
    let mut out = alloc::vec::Vec::new();
    with_devices(|list| {
        for dev in list {
            if dev.class == class {
                out.push(*dev);
            }
        }
    });
    out
}

pub fn read_bar_raw(bus: u8, slot: u8, function: u8, index: u8) -> (u32, Option<u32>) {
    if index >= 6 {
        return (0, None);
    }

    let off = 0x10u16 + (index as u16) * 4;
    let bar_lo = config_read_u32(bus, slot, function, off);
    if (bar_lo & 0x1) != 0 {
        return (bar_lo, None);
    }

    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    if is_64 {
        let bar_hi = config_read_u32(bus, slot, function, off + 4);
        (bar_lo, Some(bar_hi))
    } else {
        (bar_lo, None)
    }
}

pub fn read_bar0_raw(bus: u8, slot: u8, function: u8) -> (u32, Option<u32>) {
    read_bar_raw(bus, slot, function, 0)
}

pub fn enable_mem_and_bus_master(bus: u8, slot: u8, function: u8) {
    let mut cmd = config_read_u16(bus, slot, function, 0x04);
    cmd |= PCI_COMMAND_MEM_SPACE | PCI_COMMAND_BUS_MASTER;
    config_write_u16(bus, slot, function, 0x04, cmd);
}

pub fn try_function_level_reset(bus: u8, slot: u8, function: u8) -> bool {
    let Some(pcie_cap) = find_capability_bdf(bus, slot, function, PCI_CAP_ID_PCI_EXPRESS) else {
        return false;
    };

    let devcap = config_read_u32(bus, slot, function, pcie_cap + PCI_EXP_DEVCAP);
    if (devcap & PCI_EXP_DEVCAP_FLR) == 0 {
        return false;
    }

    let mut devctl = config_read_u16(bus, slot, function, pcie_cap + PCI_EXP_DEVCTL);
    devctl |= PCI_EXP_DEVCTL_BCR_FLR;
    config_write_u16(bus, slot, function, pcie_cap + PCI_EXP_DEVCTL, devctl);

    // PCIe FLR requires software to wait 100ms before touching config space again.
    let _ = crate::wait::spin_until_timeout(100, || false);
    true
}

/// Walk the standard PCI capability list and return the capability offset.
pub fn find_capability(dev: &PciDevice, cap_id: u8) -> Option<u16> {
    find_capability_bdf(dev.bus, dev.slot, dev.function, cap_id)
}

fn find_capability_bdf(bus: u8, slot: u8, function: u8, cap_id: u8) -> Option<u16> {
    let status = config_read_u16(bus, slot, function, 0x06);
    if (status & PCI_STATUS_CAP_LIST) == 0 {
        return None;
    }

    let mut ptr = (config_read_u8(bus, slot, function, PCI_CAP_PTR) & !0x3) as u16;
    let mut guard = 0usize;
    while ptr >= 0x40 && ptr < 0x100 && guard < 48 {
        let id = config_read_u8(bus, slot, function, ptr);
        if id == cap_id {
            return Some(ptr);
        }
        ptr = (config_read_u8(bus, slot, function, ptr + 1) & !0x3) as u16;
        guard += 1;
    }
    None
}

fn normalize_offset(offset: u16) -> u8 {
    if offset > 0xFF {
        panic!("Extended PCI config offset 0x{:X} unsupported", offset);
    }
    offset as u8
}

pub fn config_read_u8(bus: u8, slot: u8, function: u8, offset: u16) -> u8 {
    if offset < 0x1000 {
        let aligned_off = offset & !0x3;
        if let Some(aligned) = ecam_read_u32(bus, slot, function, aligned_off) {
            let shift = ((offset & 0x03) as u32) * 8;
            return ((aligned >> shift) & 0xFF) as u8;
        }
    }
    read_u8(bus, slot, function, normalize_offset(offset))
}

pub fn config_read_u16(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    if offset < 0x1000 {
        let aligned_off = offset & !0x3;
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

    if offset < 0x1000
        && let Some(value) = ecam_read_u32(bus, slot, function, offset)
    {
        return value;
    }

    let off = normalize_offset(offset);
    read_u32(bus, slot, function, off)
}

pub fn config_write_u8(bus: u8, slot: u8, function: u8, offset: u16, value: u8) {
    if offset < 0x1000 {
        let aligned_off = offset & !0x3;
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
        let aligned_off = offset & !0x3;
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

    if offset < 0x1000 && ecam_write_u32(bus, slot, function, offset, value).is_some() {
        return;
    }

    let off = normalize_offset(offset);
    write_u32(bus, slot, function, off, value);
}

/// Read config space via legacy CF8/CFC mechanism only (never ECAM).
pub fn config_read_u16_legacy(bus: u8, slot: u8, function: u8, offset: u16) -> u16 {
    read_u16(bus, slot, function, normalize_offset(offset))
}

/// Read config space via legacy CF8/CFC mechanism only (never ECAM).
pub fn config_read_u32_legacy(bus: u8, slot: u8, function: u8, offset: u16) -> u32 {
    if (offset & 0x3) != 0 {
        panic!("Unaligned PCI legacy config dword read at offset 0x{:X}", offset);
    }
    read_u32(bus, slot, function, normalize_offset(offset))
}

/// Write config space via legacy CF8/CFC mechanism only (never ECAM).
pub fn config_write_u16_legacy(bus: u8, slot: u8, function: u8, offset: u16, value: u16) {
    write_u16(bus, slot, function, normalize_offset(offset), value);
}

/// Write config space via legacy CF8/CFC mechanism only (never ECAM).
pub fn config_write_u32_legacy(bus: u8, slot: u8, function: u8, offset: u16, value: u32) {
    if (offset & 0x3) != 0 {
        panic!("Unaligned PCI legacy config dword write at offset 0x{:X}", offset);
    }
    write_u32(bus, slot, function, normalize_offset(offset), value);
}

pub fn read_bar0_raw_legacy(bus: u8, slot: u8, function: u8) -> (u32, Option<u32>) {
    let bar_lo = config_read_u32_legacy(bus, slot, function, 0x10);
    if (bar_lo & 0x1) != 0 {
        return (bar_lo, None);
    }
    let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
    if is_64 {
        let bar_hi = config_read_u32_legacy(bus, slot, function, 0x14);
        (bar_lo, Some(bar_hi))
    } else {
        (bar_lo, None)
    }
}

pub fn enable_mem_and_bus_master_legacy(bus: u8, slot: u8, function: u8) {
    let mut cmd = config_read_u16_legacy(bus, slot, function, 0x04);
    cmd |= PCI_COMMAND_MEM_SPACE | PCI_COMMAND_BUS_MASTER;
    config_write_u16_legacy(bus, slot, function, 0x04, cmd);
}

pub fn bar_size_bytes(bus: u8, slot: u8, function: u8, index: u8) -> Option<u64> {
    if index >= 6 {
        return None;
    }

    let cmd = config_read_u16(bus, slot, function, 0x04);
    config_write_u16(
        bus,
        slot,
        function,
        0x04,
        cmd & !(PCI_COMMAND_IO_SPACE | PCI_COMMAND_MEM_SPACE),
    );

    let result = (|| {
        let off = 0x10u16 + (index as u16) * 4;
        let orig_lo = config_read_u32(bus, slot, function, off);
        if (orig_lo & 0x1) != 0 {
            return None;
        }

        let is_64 = ((orig_lo >> 1) & 0x3) == 0x2;
        if is_64 && index >= 5 {
            return None;
        }
        let orig_hi = if is_64 {
            config_read_u32(bus, slot, function, off + 4)
        } else {
            0
        };

        config_write_u32(bus, slot, function, off, 0xFFFF_FFF0);
        if is_64 {
            config_write_u32(bus, slot, function, off + 4, 0xFFFF_FFFF);
        }

        let mask_lo = config_read_u32(bus, slot, function, off);
        let mask_hi = if is_64 {
            config_read_u32(bus, slot, function, off + 4)
        } else {
            0
        };

        config_write_u32(bus, slot, function, off, orig_lo);
        if is_64 {
            config_write_u32(bus, slot, function, off + 4, orig_hi);
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
            let size_mask = mask_lo & !0xFu32;
            if size_mask == 0 {
                return None;
            }
            Some((!size_mask).wrapping_add(1) as u64)
        }
    })();

    config_write_u16(bus, slot, function, 0x04, cmd);

    result
}

pub fn bar0_size_bytes(bus: u8, slot: u8, function: u8) -> Option<u64> {
    bar_size_bytes(bus, slot, function, 0)
}

fn bridge_bus_numbers(bus: u8, slot: u8, function: u8) -> (u8, u8, u8) {
    let v = config_read_u32(bus, slot, function, PCI_BRIDGE_BUS_NUMBERS);
    let primary = (v & 0xFF) as u8;
    let secondary = ((v >> 8) & 0xFF) as u8;
    let subordinate = ((v >> 16) & 0xFF) as u8;
    (primary, secondary, subordinate)
}

fn bridge_mem_window(bus: u8, slot: u8, function: u8) -> Option<(u64, u64)> {
    let v = config_read_u32(bus, slot, function, PCI_BRIDGE_MEM_BASE_LIMIT);
    let base_reg = (v & 0xFFFF) as u16;
    let limit_reg = (v >> 16) as u16;

    // 1MiB granularity. Base is bits [15:4] << 16. Limit is bits [15:4] << 16 plus 0xFFFFF.
    let base = ((base_reg as u64) & 0xFFF0u64) << 16;
    let limit_incl = (((limit_reg as u64) & 0xFFF0u64) << 16) | 0xFFFFFu64;
    if limit_incl < base {
        return None;
    }
    Some((base, limit_incl.saturating_add(1)))
}

fn parent_bridge_for_bus(target_bus: u8) -> Option<(u8, u8, u8)> {
    let mut best: Option<((u8, u8, u8), u16)> = None;
    with_devices(|list| {
        for dev in list {
            if dev.class != PCI_CLASS_BRIDGE || dev.subclass != PCI_SUBCLASS_PCI_TO_PCI {
                continue;
            }
            let (_p, sec, sub) = bridge_bus_numbers(dev.bus, dev.slot, dev.function);
            if sec == 0 {
                continue;
            }

            let score = if sec == target_bus {
                0u16
            } else if sec <= target_bus && target_bus <= sub {
                1u16 + (target_bus as u16).saturating_sub(sec as u16)
            } else {
                continue;
            };

            match best {
                Some((_b, best_score)) if score >= best_score => {}
                _ => best = Some(((dev.bus, dev.slot, dev.function), score)),
            }
        }
    });
    best.map(|(bdf, _)| bdf)
}

fn alloc_from_bridge_window(
    bridge_bus: u8,
    bridge_slot: u8,
    bridge_function: u8,
    window_base: u64,
    window_limit_excl: u64,
    size: u64,
    align: u64,
) -> Option<u64> {
    if size == 0 {
        return None;
    }

    let mut lock = BRIDGE_ALLOCS.lock();
    let mut idx: Option<usize> = None;
    for (i, a) in lock.iter().enumerate() {
        if a.bus == bridge_bus && a.slot == bridge_slot && a.function == bridge_function {
            idx = Some(i);
            break;
        }
    }

    let (base, mut next_top, limit_excl) = if let Some(i) = idx {
        let a = lock[i];
        let base = a.base.min(window_base);
        let limit_excl = a.limit_excl.max(window_limit_excl);
        let next_top = a.next_top.min(limit_excl);
        (base, next_top, limit_excl)
    } else {
        (window_base, window_limit_excl, window_limit_excl)
    };

    // Allocate from the top of the window downward.
    // Heuristic: firmware typically allocates from base upward for devices present at boot.
    // Using a top-down allocator reduces the chance of colliding with existing BARs without
    // probing other devices.
    next_top = next_top.min(limit_excl);
    if next_top <= base {
        return None;
    }

    let align = align.max(1);
    let raw_start = next_top.checked_sub(size)?;
    let start = (raw_start / align) * align;
    let end = start.checked_add(size)?;
    if start < base || end > limit_excl {
        return None;
    }

    let new = BridgeAlloc {
        bus: bridge_bus,
        slot: bridge_slot,
        function: bridge_function,
        base,
        next_top: start,
        limit_excl,
    };

    if let Some(i) = idx {
        lock[i] = new;
    } else {
        let _ = lock.push(new);
    }

    Some(start)
}

/// Allocate an MMIO base for a newly discovered device on `target_bus`.
///
/// Strategy:
/// - Prefer allocating from the parent PCIe bridge's non-prefetchable Memory Window.
/// - Fall back to the kernel's fixed "known-safe" MMIO32 allocator.
///
/// This is intentionally simple and is currently intended for our own endpoint(s).
pub fn alloc_hotplug_mmio_base(target_bus: u8, size: u64, align: u64) -> Option<u64> {
    if let Some((b_bus, b_slot, b_fun)) = parent_bridge_for_bus(target_bus)
        && let Some((base, limit_excl)) = bridge_mem_window(b_bus, b_slot, b_fun)
        && let Some(addr) =
            alloc_from_bridge_window(b_bus, b_slot, b_fun, base, limit_excl, size, align)
    {
        return Some(addr);
    }

    // Fallback: fixed allocator (below 4GiB).
    crate::pci::bar_alloc::alloc_mmio32(size, align).map(|x| x as u64)
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn outl(port: u16, val: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
}

#[inline(always)]
#[cfg(target_arch = "x86_64")]
unsafe fn inl(port: u16) -> u32 {
    let val: u32;
    asm!("in eax, dx", in("dx") port, out("eax") val, options(nomem, nostack, preserves_flags));
    val
}
