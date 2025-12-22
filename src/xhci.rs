use core::ptr::{addr_of, read_volatile};

use crate::debugconf;
use crate::limine::LIMINE_HHDM_REQUEST;

// Minimal xHCI capability probe with HHDM-based MMIO mapping.
pub struct Xhci {
    mmio_base: *mut u8,
    op_regs: *mut OpRegs,
    doorbell_base: *mut u32,
    runtime_base: *mut u8,
    slot_count: u8,
    port_count: u8,
}

impl Xhci {
    /// Map BAR0 through the higher-half direct map and read basic caps. Controller stays untouched.
    pub unsafe fn init(bar0_phys: usize) -> Option<Self> {
        if !PROBE_XHCI {
            debugconf!("xhci: probe disabled\n");
            return None;
        }

        let hhdm = hhdm_offset()?;
        let mmio_base = (bar0_phys + hhdm) as *mut u8;

        let cap = mmio_base as *mut CapRegs;
        // Access packed fields via addr_of! to avoid unaligned references.
        let caplength = read_volatile(addr_of!((*cap).caplength)) as usize;
        let hcs_params1 = read_volatile(addr_of!((*cap).hcs_params1));
        let hcc_params1 = read_volatile(addr_of!((*cap).hcc_params1));
        let dboff = read_volatile(addr_of!((*cap).dboff)) as usize;
        let rtsoff = read_volatile(addr_of!((*cap).rtsoff)) as usize;

        let slot_count = (hcs_params1 & 0xFF) as u8;
        let port_count = ((hcs_params1 >> 24) & 0xFF) as u8;

        let op_regs = mmio_base.add(caplength) as *mut OpRegs;
        let doorbell_base = mmio_base.add(dboff) as *mut u32;
        let runtime_base = mmio_base.add(rtsoff) as *mut u8;

        debugconf!(
            "xhci: caplen=0x{:02X} slots={} ports={} dboff=0x{:X} rtsoff=0x{:X} hcc=0x{:08X}\n",
            caplength,
            slot_count,
            port_count,
            dboff,
            rtsoff,
            hcc_params1
        );

        Some(Self {
            mmio_base,
            op_regs,
            doorbell_base,
            runtime_base,
            slot_count,
            port_count,
        })
    }

    pub fn slot_count(&self) -> u8 { self.slot_count }
    pub fn port_count(&self) -> u8 { self.port_count }
}

const PROBE_XHCI: bool = true;

#[repr(C, packed)]
struct CapRegs {
    caplength: u8,
    _reserved: u8,
    hci_version: u16,
    hcs_params1: u32,
    hcs_params2: u32,
    hcs_params3: u32,
    hcc_params1: u32,
    dboff: u32,
    rtsoff: u32,
    hcc_params2: u32,
}

#[repr(C)]
struct OpRegs {
    usbcmd: u32,
    usbsts: u32,
    pagesize: u32,
    _rsvd0: [u32; 2],
    dnctrl: u32,
    crcr: u64,
    _rsvd1: [u32; 4],
    dcbaap: u64,
    config: u32,
    // followed by port registers
}

fn hhdm_offset() -> Option<usize> {
    let resp_ptr = LIMINE_HHDM_REQUEST.response;
    if resp_ptr.is_null() {
        return None;
    }
    let resp = unsafe { &*resp_ptr };
    Some(resp.offset as usize)
}
