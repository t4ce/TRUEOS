use crate::{debugconf, osal, xhci};
use core::ptr::{read_volatile, write_volatile};
use embassy_time::{Duration as EmbassyDuration, Timer};

#[derive(Copy, Clone, Debug)]
pub struct XhciContext {
    pub caplength: u8,
    pub hci_version: u16,
    pub hcsparams1: u32,
    pub hccparams1: u32,
    pub op_base: *mut u32,
    pub port_count: u8,
}

impl XhciContext {
    /// # Safety
    /// Caller must ensure `info.mmio_base` is a valid mapped MMIO pointer.
    pub unsafe fn new(info: xhci::ControllerInfo) -> Self {
        let cap = info.mmio_base.as_ptr();
        let caplength = read_volatile(cap.add(0x00) as *const u8);
        let hci_version = read_volatile(cap.add(0x02) as *const u16);
        let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
        let hccparams1 = read_volatile(cap.add(0x10) as *const u32);
        let op_base = cap.add(caplength as usize) as *mut u32;
        let port_count = ((hcsparams1 >> 24) & 0xFF) as u8;

        XhciContext {
            caplength,
            hci_version,
            hcsparams1,
            hccparams1,
            op_base,
            port_count,
        }
    }

    pub unsafe fn portsc(&self, port_idx: usize) -> u32 {
        const PORT_BLOCK_OFFSET: usize = 0x400;
        const PORT_STRIDE: usize = 0x10;
        let port_base = (self.op_base as usize).saturating_add(PORT_BLOCK_OFFSET);
        let port_ptr = (port_base + port_idx * PORT_STRIDE) as *const u32;
        read_volatile(port_ptr)
    }

    pub unsafe fn reset_port(&self, port_idx: usize) {
        const PORT_BLOCK_OFFSET: usize = 0x400;
        const PORT_STRIDE: usize = 0x10;
        const PORTSC_PR: u32 = 1 << 4;
        let port_base = (self.op_base as usize).saturating_add(PORT_BLOCK_OFFSET);
        let port_ptr = (port_base + port_idx * PORT_STRIDE) as *mut u32;
        let status = read_volatile(port_ptr);
        write_volatile(port_ptr, status | PORTSC_PR);
    }
}

#[embassy_executor::task]
pub async fn usb_init_task(info: xhci::ControllerInfo) {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    debugconf!(
        "usb: xhci caps len=0x{:X} ver=0x{:04X} ports={} ac64={}\n",
        ctx.caplength,
        ctx.hci_version,
        ctx.port_count,
        (ctx.hccparams1 & 0x1) != 0
    );

    // Fresh start: simple poll of port status; more setup will follow.
    for port in 0..ctx.port_count {
        let status = unsafe { ctx.portsc(port as usize) };
        debugconf!("usb: port {:02} status=0x{:08X}\n", port + 1, status);
    }

    // Placeholder async wait to keep task alive while we flesh out rings/commands.
    loop {
        Timer::after(EmbassyDuration::from_millis(500)).await;
    }
}
