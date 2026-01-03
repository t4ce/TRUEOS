use core::ptr::read_volatile;
use core::ptr::NonNull;

use crate::pci::mmio;

#[derive(Copy, Clone, Debug)]
pub struct NvmeControllerInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar0_phys: u64,
    pub bar0_size: u64,
    pub mmio_base: NonNull<u8>,
}

unsafe impl Send for NvmeControllerInfo {}
unsafe impl Sync for NvmeControllerInfo {}

static FIRST: spin::Mutex<Option<NvmeControllerInfo>> = spin::Mutex::new(None);

fn set_first(info: NvmeControllerInfo) {
    let mut guard = FIRST.lock();
    if guard.is_none() {
        *guard = Some(info);
    }
}

pub fn first_controller() -> Option<NvmeControllerInfo> {
    FIRST.lock().clone()
}

fn is_nvme(dev: &crate::pci::PciDevice) -> bool {
    dev.class == 0x01 && dev.subclass == 0x08 && dev.prog_if == 0x02
}

pub fn probe_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("nvme: no HHDM\n");
        return;
    }

    let mut did_any = false;
    crate::pci::with_devices(|list| {
        for dev in list {
            if !is_nvme(dev) {
                continue;
            }

            did_any = true;
            crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

            let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
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

            let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
            crate::log!(
                "nvme: {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                base,
                size
            );

            // NVMe register set is small; bound mapping to a reasonable window.
            let mut map_len = if size == 0 {
                0x4000usize
            } else {
                size as usize
            };
            if map_len < 0x4000 {
                map_len = 0x4000;
            }
            if map_len > 0x10000 {
                map_len = 0x10000;
            }

            let mmio_ptr = match mmio::map_mmio_region(base, map_len) {
                Ok(ptr) => ptr,
                Err(err) => {
                    crate::log!("nvme: failed to map MMIO: {:?}\n", err);
                    continue;
                }
            };

            unsafe {
                // NVMe register offsets (bytes)
                // CAP 0x00 (u64), VS 0x08 (u32), CC 0x14 (u32), CSTS 0x1C (u32)
                let regs = mmio_ptr.as_ptr();
                let cap = read_volatile(regs.add(0x00) as *const u64);
                let vs = read_volatile(regs.add(0x08) as *const u32);
                let cc = read_volatile(regs.add(0x14) as *const u32);
                let csts = read_volatile(regs.add(0x1C) as *const u32);

                crate::log!(
                    "nvme: CAP=0x{:016X} VS=0x{:08X} CC=0x{:08X} CSTS=0x{:08X}\n",
                    cap,
                    vs,
                    cc,
                    csts
                );

                set_first(NvmeControllerInfo {
                    bus: dev.bus,
                    slot: dev.slot,
                    function: dev.function,
                    bar0_phys: base,
                    bar0_size: size as u64,
                    mmio_base: mmio_ptr,
                });
            }

            // For now, only probe the first controller.
            break;
        }
    });

    if !did_any {
        crate::log!("nvme: none found\n");
    }
}
