use alloc::vec::Vec;

use crate::{disc::block, pci::mmio};

pub(crate) type NvmeDiagController = super::nvme_backend::NvmeDiagController;

macro_rules! nvme_verbose_log {
    ($($arg:tt)*) => {
        if crate::logflag::NVME_VERBOSE {
            crate::log!($($arg)*);
        }
    };
}

pub(crate) fn diag_snapshot() -> Vec<NvmeDiagController> {
    super::nvme_backend::diag_snapshot()
}

pub fn probe_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("nvme: no HHDM\n");
        return;
    }

    let mut did_any = false;
    let mut registered_any = false;

    let mut nvme_devices: Vec<crate::pci::PciDevice> = Vec::new();
    crate::pci::with_devices(|list| {
        for dev in list {
            if super::nvme_backend::is_nvme(dev) {
                did_any = true;
                nvme_devices.push(*dev);
            }
        }
    });

    for dev in nvme_devices {
        // Quirk: FLR can wedge bring-up on some targets/emulators and leave boot
        // stuck after the reset log. Keep probe progress deterministic by default.
        let do_flr = false;
        if do_flr {
            if crate::pci::try_function_level_reset(dev.bus, dev.slot, dev.function) {
                nvme_verbose_log!(
                    "nvme: {:02X}:{:02X}.{} function-level reset issued\n",
                    dev.bus,
                    dev.slot,
                    dev.function
                );
            }
        } else {
            nvme_verbose_log!(
                "nvme: {:02X}:{:02X}.{} skipping FLR (stability quirk)\n",
                dev.bus,
                dev.slot,
                dev.function
            );
        }
        nvme_verbose_log!(
            "nvme: {:02X}:{:02X}.{} probe step=bar-read\n",
            dev.bus,
            dev.slot,
            dev.function
        );

        let (mut bar_lo, mut bar_hi) =
            crate::pci::read_bar0_raw_legacy(dev.bus, dev.slot, dev.function);
        nvme_verbose_log!(
            "nvme: {:02X}:{:02X}.{} probe step=bar-read-done lo=0x{:08X} hi={:?}\n",
            dev.bus,
            dev.slot,
            dev.function,
            bar_lo,
            bar_hi
        );
        if (bar_lo & 0x1) != 0 {
            crate::log!(
                "nvme: {:02X}:{:02X}.{} BAR0 is IO space (unsupported)\n",
                dev.bus,
                dev.slot,
                dev.function
            );
            continue;
        }

        let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
        let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
        if is_64 {
            base |= (bar_hi.unwrap_or(0) as u64) << 32;
        }
        // Avoid BAR size probe writes during bring-up on fragile targets.
        // NVMe register space is small; 16KiB mapping is sufficient.
        let mut size = 0x4000u64;

        // Hotplug/firmware gap handling: some setups expose NVMe with BAR0 still unassigned.
        // Program a safe MMIO base so CAP/VS reads become meaningful.
        if base == 0 || base >= 0x40_0000_0000 {
            if size == 0 {
                size = 0x4000;
            }
            let align = size.max(0x1000);
            nvme_verbose_log!(
                "nvme: {:02X}:{:02X}.{} probe step=bar-alloc size=0x{:X} align=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                size,
                align
            );
            let Some(new_base) = crate::pci::alloc_hotplug_mmio_base(dev.bus, size, align) else {
                crate::log!(
                    "nvme: {:02X}:{:02X}.{} BAR0 unassigned and allocator failed (size=0x{:X} align=0x{:X})\n",
                    dev.bus,
                    dev.slot,
                    dev.function,
                    size,
                    align
                );
                continue;
            };

            let new_lo = ((new_base as u32) & !0xFu32) | (bar_lo & 0xFu32);
            nvme_verbose_log!(
                "nvme: {:02X}:{:02X}.{} probe step=bar-write new_base=0x{:X} new_lo=0x{:08X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                new_base,
                new_lo
            );
            crate::pci::config_write_u32_legacy(dev.bus, dev.slot, dev.function, 0x10, new_lo);
            if is_64 {
                crate::pci::config_write_u32_legacy(
                    dev.bus,
                    dev.slot,
                    dev.function,
                    0x14,
                    (new_base >> 32) as u32,
                );
            }

            nvme_verbose_log!(
                "nvme: {:02X}:{:02X}.{} probe step=bar-reread\n",
                dev.bus,
                dev.slot,
                dev.function
            );
            (bar_lo, bar_hi) = crate::pci::read_bar0_raw_legacy(dev.bus, dev.slot, dev.function);
            base = (bar_lo & 0xFFFF_FFF0) as u64;
            if is_64 {
                base |= (bar_hi.unwrap_or(0) as u64) << 32;
            }

            nvme_verbose_log!(
                "nvme: {:02X}:{:02X}.{} BAR0 assigned by OS base=0x{:X} size=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                base,
                size
            );
            crate::pci::enable_mem_and_bus_master_legacy(dev.bus, dev.slot, dev.function);
        }

        crate::pci::enable_mem_and_bus_master_legacy(dev.bus, dev.slot, dev.function);

        nvme_verbose_log!(
            "nvme: {:02X}:{:02X}.{} BAR0 raw lo=0x{:08X} hi={:?} base=0x{:X} size=0x{:X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            bar_lo,
            bar_hi,
            base,
            size
        );

        let mut map_len = if size == 0 {
            0x4000usize
        } else {
            size as usize
        };
        map_len = map_len.clamp(0x4000, 0x10000);

        nvme_verbose_log!(
            "nvme: {:02X}:{:02X}.{} probe step=mmio-map base=0x{:X} len=0x{:X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            base,
            map_len
        );

        let mmio_ptr = match mmio::map_mmio_region(base, map_len) {
            Ok(ptr) => ptr,
            Err(err) => {
                crate::log!("nvme: failed to map MMIO: {:?}\n", err);
                continue;
            }
        };

        let pci_addr = block::PciAddress::new(dev.bus, dev.slot, dev.function);
        if super::nvme_backend::register_mapped_controller(mmio_ptr, pci_addr) {
            registered_any = true;

            // For now, only claim/register the first controller.
            break;
        }
    }

    if !did_any {
        crate::log!("nvme: none found\n");
    } else if !registered_any {
        crate::log!("nvme: found controller(s) but none registered\n");
    }
}
