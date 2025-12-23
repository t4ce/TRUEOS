use core::ptr::{read_volatile, write_volatile, NonNull};
use spin::Mutex;

#[derive(Copy, Clone, Debug)]
pub struct ControllerInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar_phys: u64,
    pub bar_size: u64,
    pub mmio_base: NonNull<u8>,
    pub supports_64bit: bool,
}

unsafe impl Send for ControllerInfo {}
unsafe impl Sync for ControllerInfo {}

static FIRST_CONTROLLER: Mutex<Option<ControllerInfo>> = Mutex::new(None);

fn set_first_controller(info: ControllerInfo) {
    let mut guard = FIRST_CONTROLLER.lock();
    if guard.is_none() {
        *guard = Some(info);
    }
}

pub fn controller_info() -> Option<ControllerInfo> {
    FIRST_CONTROLLER.lock().clone()
}

pub fn init_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::debugconf!("xhci: no HHDM\n");
        return;
    }

    let mut did_any = false;
    crate::pci::with_devices(|list| {
        for dev in list {
            if dev.class != 0x0C || dev.subclass != 0x03 || dev.prog_if != 0x30 {
                continue;
            }

            did_any = true;
            crate::pci::enable_mem_and_bus_master(dev.bus, dev.slot, dev.function);

            let (bar_lo, bar_hi) = crate::pci::read_bar0_raw(dev.bus, dev.slot, dev.function);
            if (bar_lo & 0x1) != 0 {
                crate::debugconf!("xhci: IO BAR not supported\n");
                break;
            }

            let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
            let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
            if is_64 {
                base |= (bar_hi.unwrap_or(0) as u64) << 32;
            }

            let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
            crate::debugconf!(
                "xhci: {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                base,
                size
            );

            let map_len = if size == 0 { 0x10_000usize } else { size as usize };
            let mmio = match crate::mmio::map_mmio_region(base, map_len) {
                Ok(ptr) => ptr,
                Err(err) => {
                    crate::debugconf!("xhci: failed to map MMIO: {:?}\n", err);
                    break;
                }
            };

            unsafe {
                let cap = mmio.as_ptr();
                let caplength = read_volatile(cap.add(0x00) as *const u8) as u64;
                let hci_version = read_volatile(cap.add(0x02) as *const u16);
                let hccparams1 = read_volatile(cap.add(0x10) as *const u32);
                let supports_64bit = (hccparams1 & 0x1) != 0;
                let op = cap.add(caplength as usize) as *mut u32;

                crate::debugconf!(
                    "xhci: caplen=0x{:X} ver=0x{:04X} op=0x{:X} ac64={}\n",
                    caplength,
                    hci_version,
                    op as usize,
                    supports_64bit
                );

                set_first_controller(ControllerInfo {
                    bus: dev.bus,
                    slot: dev.slot,
                    function: dev.function,
                    bar_phys: base,
                    bar_size: size as u64,
                    mmio_base: mmio,
                    supports_64bit,
                });

                const USBCMD: usize = 0x00 / 4;
                const USBSTS: usize = 0x04 / 4;

                const USBCMD_RS: u32 = 1 << 0;
                const USBCMD_HCRST: u32 = 1 << 1;

                const USBSTS_HCH: u32 = 1 << 0;
                const USBSTS_CNR: u32 = 1 << 11;

                let mut cmd = read_volatile(op.add(USBCMD));
                let mut sts = read_volatile(op.add(USBSTS));

                if (cmd & USBCMD_RS) != 0 {
                    cmd &= !USBCMD_RS;
                    write_volatile(op.add(USBCMD), cmd);
                }

                let mut spin: u64 = 5_000_000;
                while (sts & USBSTS_HCH) == 0 && spin != 0 {
                    sts = read_volatile(op.add(USBSTS));
                    spin -= 1;
                }
                if (sts & USBSTS_HCH) == 0 {
                    crate::debugconf!("xhci: halt timeout sts=0x{:X}\n", sts);
                    break;
                }

                cmd = read_volatile(op.add(USBCMD));
                write_volatile(op.add(USBCMD), cmd | USBCMD_HCRST);

                spin = 10_000_000;
                while (read_volatile(op.add(USBCMD)) & USBCMD_HCRST) != 0 && spin != 0 {
                    spin -= 1;
                }
                if (read_volatile(op.add(USBCMD)) & USBCMD_HCRST) != 0 {
                    crate::debugconf!("xhci: reset bit stuck\n");
                    break;
                }

                spin = 10_000_000;
                sts = read_volatile(op.add(USBSTS));
                while (sts & USBSTS_CNR) != 0 && spin != 0 {
                    sts = read_volatile(op.add(USBSTS));
                    spin -= 1;
                }

                if (sts & USBSTS_CNR) != 0 {
                    crate::debugconf!("xhci: CNR stuck sts=0x{:X}\n", sts);
                    break;
                }

                crate::debugconf!("xhci: reset ok sts=0x{:X}\n", sts);
            }

            break;
        }
    });

    if !did_any {
        crate::debugconf!("xhci: not found\n");
    }
}
