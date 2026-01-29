use crate::pci::mmio;
use core::mem::size_of;
use core::ptr::{null_mut, read_volatile, write_volatile, NonNull};
use core::sync::atomic::{AtomicBool, AtomicU32, fence, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

/// Firmware-provided information about the physical xHC (controller hardware).
#[derive(Copy, Clone, Debug)]
pub struct XhcInfo {
    pub bus: u8,
    pub slot: u8,
    pub function: u8,
    pub bar_phys: u64,
    pub bar_size: u64,
    pub mmio_base: NonNull<u8>,
    pub supports_64bit: bool,
    pub controller_id: usize,
}

/// Attempt xHCI legacy BIOS/OS ownership handoff.
///
/// On some real machines the firmware keeps the controller "owned" unless the OS
/// sets OS Owned Semaphore in the xHCI Legacy Support extended capability.
unsafe fn bios_handoff_if_present(cap: *mut u8, hccparams1: u32) {
    // xECP: offset (in DWORDs) from capability base.
    let mut ecp_dwords = ((hccparams1 >> 16) & 0xFFFF) as usize;
    if ecp_dwords == 0 {
        return;
    }

    for _ in 0..64 {
        let off_bytes = ecp_dwords * 4;
        let hdr = read_volatile(cap.add(off_bytes) as *const u32);
        let cap_id = (hdr & 0xFF) as u8;
        let next = ((hdr >> 8) & 0xFF) as usize;

        if cap_id == 1 {
            // Legacy Support capability.
            let legsup = cap.add(off_bytes) as *mut u32;
            let legctlsts = cap.add(off_bytes + 4) as *mut u32;

            const BIOS_OWNED: u32 = 1 << 16;
            const OS_OWNED: u32 = 1 << 24;

            // Request OS ownership.
            let mut v = read_volatile(legsup);
            if (v & OS_OWNED) == 0 {
                v |= OS_OWNED;
                write_volatile(legsup, v);
                fence(Ordering::SeqCst);
            }

            // Wait for BIOS to drop ownership.
            let mut spin: u32 = 5_000_000;
            while spin != 0 {
                let cur = read_volatile(legsup);
                if (cur & BIOS_OWNED) == 0 {
                    break;
                }
                spin -= 1;
            }

            // Disable SMIs and clear pending SMI status bits.
            // Lower 16 bits are SMI enables; upper 16 bits are RW1C status.
            let ctl = read_volatile(legctlsts);
            write_volatile(legctlsts, ctl & 0xFFFF_0000);
            fence(Ordering::SeqCst);
            return;
        }

        if next == 0 {
            break;
        }
        ecp_dwords += next;
    }
}

unsafe impl Send for XhcInfo {}
unsafe impl Sync for XhcInfo {}

pub const MAX_XHCI_CONTROLLERS: usize = 8;

static FIRST_CONTROLLER: Mutex<Option<XhcInfo>> = Mutex::new(None);
static CONTROLLERS: Mutex<Vec<XhcInfo, MAX_XHCI_CONTROLLERS>> = Mutex::new(Vec::new());
static LOG_PORTS_ON_INIT: AtomicBool = AtomicBool::new(true);

// Per-controller cache of the last enumerated VID:PID per *root* port.
// Packed as (vid << 16) | pid, with 0 meaning "unknown".
const MAX_PORTS_TRACKED: usize = 256;
static PORT_VIDPID: [[AtomicU32; MAX_PORTS_TRACKED]; MAX_XHCI_CONTROLLERS] =
    [const { [const { AtomicU32::new(0) }; MAX_PORTS_TRACKED] }; MAX_XHCI_CONTROLLERS];

pub fn set_port_vidpid(controller_id: usize, port_id: u8, vid: u16, pid: u16) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }
    let idx = port_id as usize;
    if idx == 0 || idx >= MAX_PORTS_TRACKED {
        return;
    }
    let packed = ((vid as u32) << 16) | (pid as u32);
    PORT_VIDPID[controller_id][idx].store(packed, Ordering::Release);
}

pub fn get_port_vidpid(controller_id: usize, port_id: u8) -> Option<(u16, u16)> {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return None;
    }
    let idx = port_id as usize;
    if idx == 0 || idx >= MAX_PORTS_TRACKED {
        return None;
    }
    let packed = PORT_VIDPID[controller_id][idx].load(Ordering::Acquire);
    if packed == 0 {
        return None;
    }
    Some(((packed >> 16) as u16, (packed & 0xFFFF) as u16))
}

pub fn log_cached_port_id(controller_id: usize, port_id: u8) {
    if let Some((vid, pid)) = get_port_vidpid(controller_id, port_id) {
        crate::log!(
            "xhci: port {} id={:04X}:{:04X} (cached after descriptor read)\n",
            port_id,
            vid,
            pid
        );
    }
}

pub fn set_log_ports_on_init(enable: bool) {
    LOG_PORTS_ON_INIT.store(enable, Ordering::Release);
}

fn register_xhc(mut info: XhcInfo) {
    let mut list = CONTROLLERS.lock();
    let id = list.len();
    if id >= MAX_XHCI_CONTROLLERS {
        crate::log!(
            "xhci: controller list full; dropping {:02X}:{:02X}.{}\n",
            info.bus,
            info.slot,
            info.function
        );
        return;
    }

    info.controller_id = id;

    let mut first = FIRST_CONTROLLER.lock();
    if first.is_none() {
        *first = Some(info);
    }

    let _ = list.push(info);
}

/// Returns cached information about the first detected xHC hardware block.
pub fn xhc_info() -> Option<XhcInfo> {
    FIRST_CONTROLLER.lock().clone()
}

/// Returns cached information about all detected xHC controllers.
pub fn xhc_list() -> Vec<XhcInfo, MAX_XHCI_CONTROLLERS> {
    CONTROLLERS.lock().clone()
}

pub fn init_once() {
    if crate::limine::hhdm_offset().is_none() {
        crate::log!("xhci: no HHDM\n");
        return;
    }

    FIRST_CONTROLLER.lock().take();
    CONTROLLERS.lock().clear();

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
                crate::log!("xhci: IO BAR not supported\n");
                continue;
            }

            let is_64 = ((bar_lo >> 1) & 0x3) == 0x2;
            let mut base = (bar_lo & 0xFFFF_FFF0) as u64;
            if is_64 {
                base |= (bar_hi.unwrap_or(0) as u64) << 32;
            }

            let size = crate::pci::bar0_size_bytes(dev.bus, dev.slot, dev.function).unwrap_or(0);
            crate::log!(
                "xhci: {:02X}:{:02X}.{} bar0=0x{:X} size=0x{:X}\n",
                dev.bus,
                dev.slot,
                dev.function,
                base,
                size
            );

            // xHCI MMIO spaces can be large, but the register blocks we touch live in the
            // beginning. Mapping a bounded window avoids insane mappings if BAR sizing is odd.
            let mut map_len = if size == 0 {
                0x10_000usize
            } else {
                size as usize
            };
            if map_len < 0x10_000 {
                map_len = 0x10_000;
            }
            if map_len > 0x1_00000 {
                map_len = 0x1_00000;
            }
            let mmio = match mmio::map_mmio_region(base, map_len) {
                Ok(ptr) => ptr,
                Err(err) => {
                    crate::log!("xhci: failed to map MMIO: {:?}\n", err);
                    continue;
                }
            };

            unsafe {
                let cap = mmio.as_ptr();
                let caplength = read_volatile(cap.add(0x00) as *const u8) as u64;
                let hci_version = read_volatile(cap.add(0x02) as *const u16);
                let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
                let hccparams1 = read_volatile(cap.add(0x10) as *const u32);
                let supports_64bit = (hccparams1 & 0x1) != 0;
                let op = cap.add(caplength as usize) as *mut u32;

                crate::log!(
                    "xhci: caplen=0x{:X} ver=0x{:04X} op=0x{:X} ac64={}\n",
                    caplength,
                    hci_version,
                    op as usize,
                    supports_64bit
                );

                // Real hardware may require BIOS/OS ownership handoff.
                bios_handoff_if_present(cap, hccparams1);

                let info = XhcInfo {
                    bus: dev.bus,
                    slot: dev.slot,
                    function: dev.function,
                    bar_phys: base,
                    bar_size: size as u64,
                    mmio_base: mmio,
                    supports_64bit,
                    controller_id: 0,
                };

                register_xhc(info);

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
                    crate::log!("xhci: halt timeout sts=0x{:X}\n", sts);
                    continue;
                }

                cmd = read_volatile(op.add(USBCMD));
                write_volatile(op.add(USBCMD), cmd | USBCMD_HCRST);

                spin = 10_000_000;
                while (read_volatile(op.add(USBCMD)) & USBCMD_HCRST) != 0 && spin != 0 {
                    spin -= 1;
                }
                if (read_volatile(op.add(USBCMD)) & USBCMD_HCRST) != 0 {
                    crate::log!("xhci: reset bit stuck\n");
                    continue;
                }

                spin = 10_000_000;
                sts = read_volatile(op.add(USBSTS));
                while (sts & USBSTS_CNR) != 0 && spin != 0 {
                    sts = read_volatile(op.add(USBSTS));
                    spin -= 1;
                }

                if (sts & USBSTS_CNR) != 0 {
                    crate::log!("xhci: CNR stuck sts=0x{:X}\n", sts);
                    continue;
                }

                crate::log!("xhci: reset ok sts=0x{:X}\n", sts);

                if LOG_PORTS_ON_INIT.load(Ordering::Acquire) {
                    let ctx = XhciContext::new(info);
                    log_ports_table(&ctx);
                }
            }

        }
    });

    if !did_any {
        crate::log!("xhci: not found\n");
    }
}

#[repr(C, align(16))]
#[derive(Copy, Clone, Default)]
pub struct Trb {
    pub d0: u32,
    pub d1: u32,
    pub d2: u32,
    pub d3: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Default)]
pub struct ErstEntry {
    pub seg_base_lo: u32,
    pub seg_base_hi: u32,
    pub seg_size: u32,
    pub rsvd: u32,
}

pub struct TrbRing {
    pub phys: u64,
    pub trbs: *mut Trb,
    pub len: usize,
    enqueue: usize,
    cycle: bool,
}

#[derive(Copy, Clone, Debug)]
pub struct TrbRingState {
    pub phys: u64,
    pub trbs: *mut Trb,
    pub len: usize,
    pub enqueue: usize,
    pub cycle: bool,
}

// Safe because access is synchronized and the ring memory is DMA-mapped and stable.
unsafe impl Send for TrbRingState {}
unsafe impl Sync for TrbRingState {}

unsafe impl Send for TrbRing {}
unsafe impl Sync for TrbRing {}

pub struct EventRing {
    pub phys: u64,
    pub trbs: *mut Trb,
    pub count: usize,
    dequeue: usize,
    cycle: bool,
}

unsafe impl Send for EventRing {}

const EVENT_BUFFER_CAP: usize = 512;

struct EventBuffer {
    entries: Vec<Trb, EVENT_BUFFER_CAP>,
}

impl EventBuffer {
    const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn push(&mut self, evt: Trb) {
        if self.entries.push(evt).is_err() {
            let _ = self.entries.remove(0);
            let _ = self.entries.push(evt);
        }
    }

    fn take_matching<F>(&mut self, predicate: &mut F) -> Option<Trb>
    where
        F: FnMut(&Trb) -> bool,
    {
        for idx in 0..self.entries.len() {
            if predicate(&self.entries[idx]) {
                return Some(self.entries.remove(idx));
            }
        }
        None
    }

    fn clear(&mut self) {
        self.entries.clear();
    }
}

static EVENT_BUFFERS: [Mutex<EventBuffer>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(EventBuffer::new()) }; MAX_XHCI_CONTROLLERS];

struct EventRingState {
    ring: Option<EventRing>,
    intr0: *mut u32,
}

unsafe impl Send for EventRingState {}

impl EventRingState {
    const fn new() -> Self {
        Self {
            ring: None,
            intr0: null_mut(),
        }
    }

    fn has_ring(&self) -> bool {
        self.ring.is_some() && !self.intr0.is_null()
    }
}

static EVENT_RING_STATES: [Mutex<EventRingState>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(EventRingState::new()) }; MAX_XHCI_CONTROLLERS];

/// Software-side state for the xHCI (driver) view of the controller.
#[derive(Copy, Clone, Debug)]
pub struct XhciContext {
    pub caplength: u8,
    pub hci_version: u16,
    pub hcsparams1: u32,
    pub hcsparams2: u32,
    pub hccparams1: u32,
    pub op_base: *mut u32,
    pub doorbell: *mut u32,
    pub runtime: *mut u32,
    pub port_count: u8,
    pub controller_id: usize,
}

impl XhciContext {
    /// # Safety
    /// Caller must ensure `info.mmio_base` is a valid mapped MMIO pointer.
    pub unsafe fn new(info: XhcInfo) -> Self {
        let cap = info.mmio_base.as_ptr();
        let caplength = read_volatile(cap.add(0x00) as *const u8);
        let hci_version = read_volatile(cap.add(0x02) as *const u16);
        let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
        let hcsparams2 = read_volatile(cap.add(0x08) as *const u32);
        let hccparams1 = read_volatile(cap.add(0x10) as *const u32);
        let dboff = read_volatile(cap.add(0x14) as *const u32) & !0x1F;
        let rtsoff = read_volatile(cap.add(0x18) as *const u32) & !0x1F;
        let op_base = cap.add(caplength as usize) as *mut u32;
        let doorbell = cap.add(dboff as usize) as *mut u32;
        let runtime = cap.add(rtsoff as usize) as *mut u32;
        let port_count = ((hcsparams1 >> 24) & 0xFF) as u8;

        XhciContext {
            caplength,
            hci_version,
            hcsparams1,
            hcsparams2,
            hccparams1,
            op_base,
            doorbell,
            runtime,
            port_count,
            controller_id: info.controller_id,
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
        const PORTSC_PED: u32 = 1 << 1;
        const PORTSC_LWS: u32 = 1 << 16;
        let port_base = (self.op_base as usize).saturating_add(PORT_BLOCK_OFFSET);
        let port_ptr = (port_base + port_idx * PORT_STRIDE) as *mut u32;

        // PORTSC contains RW1C and RW1S bits. Never mirror RW1S bits as 1
        // (except the one we intentionally trigger), otherwise we may retrigger
        // actions or enter undefined controller behavior on real hardware.
        let cur = read_volatile(port_ptr);
        let mut writeback = cur;
        // Never mirror RW1S bits back as 1.
        writeback &= !(PORTSC_PR | PORTSC_LWS);
        // PED is RW1C on xHCI; writing 1 would disable the port.
        writeback &= !PORTSC_PED;
        // Trigger a port reset.
        writeback |= PORTSC_PR;
        write_volatile(port_ptr, writeback);
    }

    pub unsafe fn ensure_port_powered(&self, port_idx: usize) {
        const PORT_BLOCK_OFFSET: usize = 0x400;
        const PORT_STRIDE: usize = 0x10;
        const PORTSC_PP: u32 = 1 << 9;
        const PORTSC_PED: u32 = 1 << 1;
        const PORTSC_LWS: u32 = 1 << 16;
        let port_base = (self.op_base as usize).saturating_add(PORT_BLOCK_OFFSET);
        let port_ptr = (port_base + port_idx * PORT_STRIDE) as *mut u32;

        let cur = read_volatile(port_ptr);
        if (cur & PORTSC_PP) != 0 {
            return;
        }

        let mut writeback = cur;
        // Avoid writing RW1S bits as 1 accidentally.
        // (PR is RW1S too; don't retrigger it while powering.)
        writeback &= !(1 << 4);
        writeback &= !PORTSC_LWS;
        // PED is RW1C; never write it as 1.
        writeback &= !PORTSC_PED;
        // Power on.
        writeback |= PORTSC_PP;
        write_volatile(port_ptr, writeback);
    }

    pub fn max_scratchpad_buffers(&self) -> u32 {
        // xHCI HCSPARAMS2: Max Scratchpad Buffers
        // - Max Scratchpad Buffers Lo: bits [20:16]
        // - Max Scratchpad Buffers Hi: bits [26:21]
        // Count = (Hi << 5) | Lo
        let low = (self.hcsparams2 >> 16) & 0x1F;
        let high = (self.hcsparams2 >> 21) & 0x1F;
        (high << 5) | low
    }

    /// Returns the PAGESIZE register bitmask (bit n => 2^(12+n) supported).
    pub unsafe fn page_size_mask(&self) -> u32 {
        const PAGESIZE: usize = 0x08 / 4;
        read_volatile(self.op_base.add(PAGESIZE))
    }
}

pub fn endpoint_target(ep_addr: u8) -> u32 {
    let ep_num = (ep_addr & 0x0F) as u32;
    let dir_in = (ep_addr & 0x80) != 0;
    if ep_num == 0 {
        1
    } else {
        ep_num * 2 + if dir_in { 1 } else { 0 }
    }
}

pub fn context_index(ep_addr: u8) -> u32 {
    endpoint_target(ep_addr) + 1
}

// Endpoint context bit helpers (xHCI Rev 1.2, section 6.2.3.6).
pub const EP_STATE_DISABLED: u32 = 0;
pub const EP_STATE_RUNNING: u32 = 1;

pub const EP_TYPE_ISOCH_OUT: u32 = 1;
pub const EP_TYPE_BULK_OUT: u32 = 2;
pub const EP_TYPE_INT_OUT: u32 = 3;
pub const EP_TYPE_CONTROL: u32 = 4;
pub const EP_TYPE_ISOCH_IN: u32 = 5;
pub const EP_TYPE_BULK_IN: u32 = 6;
pub const EP_TYPE_INT_IN: u32 = 7;

#[inline(always)]
pub const fn ep_state_bits(state: u32) -> u32 {
    state & 0x7
}

#[inline(always)]
pub const fn ep_mult_bits(mult: u32) -> u32 {
    (mult & 0x3) << 8
}

#[inline(always)]
pub const fn ep_interval_bits(interval: u32) -> u32 {
    (interval & 0xFF) << 16
}

#[inline(always)]
pub const fn ep_type_bits(ep_type: u32) -> u32 {
    (ep_type & 0x7) << 3
}

#[inline(always)]
pub const fn ep_cerr_bits(count: u32) -> u32 {
    (count & 0x3) << 1
}

#[inline(always)]
pub const fn ep_max_burst_bits(burst: u32) -> u32 {
    (burst & 0xFF) << 8
}

#[inline(always)]
pub const fn ep_max_packet_bits(bytes: u32) -> u32 {
    (bytes & 0xFFFF) << 16
}

#[inline(always)]
pub const fn ep_avg_trb_len_bits(len: u32) -> u32 {
    len & 0xFFFF
}

#[inline(always)]
pub const fn ep_max_esit_payload_lo_bits(payload: u32) -> u32 {
    (payload & 0xFFFF) << 16
}

#[inline(always)]
pub const fn ep_max_esit_payload_hi_bits(payload: u32) -> u32 {
    ((payload >> 16) & 0xFF) << 24
}

pub fn decode_port_status(status: u32) -> (bool, bool, &'static str) {
    const PORTSC_CCS: u32 = 1 << 0;
    const PORTSC_PED: u32 = 1 << 1;
    const PORTSC_SPEED_SHIFT: u32 = 10;
    const PORTSC_SPEED_MASK: u32 = 0xF << PORTSC_SPEED_SHIFT;

    let connected = (status & PORTSC_CCS) != 0;
    let enabled = (status & PORTSC_PED) != 0;
    let speed_code = (status & PORTSC_SPEED_MASK) >> PORTSC_SPEED_SHIFT;

    let speed = match speed_code {
        0 => "none",
        1 => "full",
        2 => "low",
        3 => "high",
        4 => "super",
        5 => "super+",
        _ => "unknown",
    };

    (connected, enabled, speed)
}

impl TrbRing {
    /// # Safety
    /// Caller must ensure `trbs` points to a DMA-mapped, zeroed region of `len` TRBs.
    pub unsafe fn new(phys: u64, trbs: *mut Trb, len: usize) -> Self {
        let ring = TrbRing {
            phys,
            trbs,
            len,
            enqueue: 0,
            cycle: true,
        };
        ring.init_link_trb();
        ring
    }

    pub fn snapshot(&self) -> TrbRingState {
        TrbRingState {
            phys: self.phys,
            trbs: self.trbs,
            len: self.len,
            enqueue: self.enqueue,
            cycle: self.cycle,
        }
    }

    /// # Safety
    /// Caller must ensure the TRB ring memory is valid and still DMA-mapped.
    pub unsafe fn from_state(state: TrbRingState) -> Self {
        TrbRing {
            phys: state.phys,
            trbs: state.trbs,
            len: state.len,
            enqueue: state.enqueue,
            cycle: state.cycle,
        }
    }

    unsafe fn init_link_trb(&self) {
        const TRB_TYPE_LINK: u32 = 6;
        if self.len < 2 {
            return;
        }
        let link_idx = self.len - 1;
        let link_ptr = self.trbs.add(link_idx);
        let mut link = Trb {
            d0: lo(self.phys),
            d1: hi(self.phys),
            d2: 0,
            d3: trb_type(TRB_TYPE_LINK) | (1 << 1),
        };
        link.d3 |= 1;
        write_volatile(link_ptr, link);
    }

    #[inline(always)]
    unsafe fn set_link_cycle_bit(&self, cycle: bool) {
        if self.len < 2 {
            return;
        }
        let link_idx = self.len - 1;
        let link_ptr = self.trbs.add(link_idx);
        let mut link = read_volatile(link_ptr);
        link.d3 = (link.d3 & !1) | (cycle as u32);
        write_volatile(link_ptr, link);
    }

    pub fn push(&mut self, mut trb: Trb) -> bool {
        if self.len < 2 {
            return false;
        }

        let usable = self.len - 1;
        if self.enqueue >= usable {
            // Shouldn't happen (we never enqueue into the Link TRB slot), but recover.
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }

        trb.d3 = (trb.d3 & !1) | (self.cycle as u32);
        unsafe { write_volatile(self.trbs.add(self.enqueue), trb) };
        self.enqueue += 1;
        if self.enqueue >= usable {
            // We just filled the last usable TRB. Ensure the Link TRB's cycle bit
            // matches the current cycle so the controller can follow it, then
            // toggle for the next pass.
            unsafe { self.set_link_cycle_bit(self.cycle) };
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }
        true
    }

    pub fn push_with_phys(&mut self, mut trb: Trb) -> Option<u64> {
        if self.len < 2 {
            return None;
        }

        let usable = self.len - 1;
        if self.enqueue >= usable {
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }

        let idx = self.enqueue;
        trb.d3 = (trb.d3 & !1) | (self.cycle as u32);
        unsafe { write_volatile(self.trbs.add(idx), trb) };
        self.enqueue += 1;
        if self.enqueue >= usable {
            unsafe { self.set_link_cycle_bit(self.cycle) };
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }

        Some(self.phys + (idx as u64) * size_of::<Trb>() as u64)
    }

    pub fn crcr_value(&self) -> u64 {
        self.phys | if self.cycle { 1 } else { 0 }
    }

    pub fn dequeue_ptr(&self) -> u64 {
        (self.phys & !0xF) | 1
    }

    pub fn state_snapshot(&self) -> (usize, bool) {
        (self.enqueue, self.cycle)
    }
}

impl EventRing {
    /// # Safety
    /// Caller must ensure `trbs` points to a DMA region with `count` TRBs.
    pub unsafe fn new(phys: u64, trbs: *mut Trb, count: usize) -> Self {
        EventRing {
            phys,
            trbs,
            count,
            dequeue: 0,
            cycle: true,
        }
    }

    pub unsafe fn update_erdp(&self, intr0: *mut u32) {
        const ERDP: usize = 0x18 / 4;
        let ptr = self.phys + (self.dequeue as u64 * size_of::<Trb>() as u64);
        write_volatile(intr0.add(ERDP + 1), hi(ptr));
        write_volatile(intr0.add(ERDP), lo(ptr) | (1 << 3));
    }

    pub unsafe fn pop(&mut self, intr0: *mut u32) -> Option<Trb> {
        if self.count == 0 {
            return None;
        }

        let trb = read_volatile(self.trbs.add(self.dequeue));
        let trb_cycle = (trb.d3 & 1) != 0;
        if trb_cycle != self.cycle {
            return None;
        }

        // Lowest-level trace can overwhelm the system (especially on real hardware
        // when Port Status Change Events storm). Keep it off by default.
        const TRACE_EVENT_TRBS: bool = false;
        if TRACE_EVENT_TRBS {
            crate::log!(
                "xhci: evt dequeue={} cycle={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                self.dequeue,
                self.cycle as u8,
                trb.d0,
                trb.d1,
                trb.d2,
                trb.d3
            );
        }

        self.dequeue += 1;
        if self.dequeue >= self.count {
            self.dequeue = 0;
            self.cycle = !self.cycle;
        }

        self.update_erdp(intr0);
        Some(trb)
    }

    pub fn last_trb_phys(&self) -> u64 {
        if self.count == 0 {
            return self.phys;
        }
        let idx = if self.dequeue == 0 {
            self.count - 1
        } else {
            self.dequeue - 1
        };
        self.phys + (idx as u64) * size_of::<Trb>() as u64
    }

    pub async fn wait_for_trb<F>(
        &mut self,
        ctx: &XhciContext,
        mut predicate: F,
        timeout_iters: usize,
        delay: EmbassyDuration,
    ) -> Option<Trb>
    where
        F: FnMut(Trb) -> Option<Trb>,
    {
        let intr0 = unsafe { ctx.runtime.add(0x20 / 4) };
        let mut polls = 0usize;
        loop {
            if let Some(evt) = unsafe { self.pop(intr0) } {
                if let Some(done) = predicate(evt) {
                    return Some(done);
                }
            }
            polls += 1;
            if polls > timeout_iters {
                return None;
            }
            Timer::after(delay).await;
        }
    }
}

#[inline(always)]
pub unsafe fn clear_port_change_bits(ctx: &XhciContext, port_id: u8) {
    if port_id == 0 {
        return;
    }
    // Port ID is 1-based.
    let port_idx = (port_id as usize).saturating_sub(1);

    // RW1C change bits in PORTSC. Writing 1 clears, writing 0 leaves unchanged.
    const PORTSC_CSC: u32 = 1 << 17;
    const PORTSC_PEC: u32 = 1 << 18;
    const PORTSC_WRC: u32 = 1 << 19;
    const PORTSC_OCC: u32 = 1 << 20;
    const PORTSC_PRC: u32 = 1 << 21;
    const PORTSC_PLC: u32 = 1 << 22;
    const PORTSC_CEC: u32 = 1 << 23;
    const RW1C_MASK: u32 =
        PORTSC_CSC | PORTSC_PEC | PORTSC_WRC | PORTSC_OCC | PORTSC_PRC | PORTSC_PLC | PORTSC_CEC;

    // RW1S bits: writing 1 would *trigger* an action. Don't mirror these back.
    const PORTSC_PR: u32 = 1 << 4; // Port Reset (RW1S)
    const PORTSC_LWS: u32 = 1 << 16; // Link Write Strobe (RW1S)
                                     // PED is RW1C: writing 1 would disable the port.
    const PORTSC_PED: u32 = 1 << 1;

    const PORT_BLOCK_OFFSET: usize = 0x400;
    const PORT_STRIDE: usize = 0x10;
    let port_base = (ctx.op_base as usize).saturating_add(PORT_BLOCK_OFFSET);
    let port_ptr = (port_base + port_idx * PORT_STRIDE) as *mut u32;

    // Preserve current state bits; only clear the change bits that are set.
    let cur = read_volatile(port_ptr);
    let clear = cur & RW1C_MASK;
    if clear == 0 {
        return;
    }
    let mut writeback = cur;
    // Never write back RW1S bits as 1 (could retrigger reset/link state changes).
    writeback &= !(PORTSC_PR | PORTSC_LWS);
    // Never write PED as 1 (RW1C).
    writeback &= !PORTSC_PED;
    // Ensure we only write 1s for change bits we want to clear.
    writeback &= !RW1C_MASK;
    writeback |= clear;
    write_volatile(port_ptr, writeback);
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn poll_task(info: XhcInfo) {
    crate::log!(
        "xhci: controller poll task running bus={:02X} slot={:02X} fn={}\n",
        info.bus,
        info.slot,
        info.function
    );

    let ctx = unsafe { XhciContext::new(info) };
    let controller_id = ctx.controller_id;

    loop {
        let evt_opt = {
            let mut state = EVENT_RING_STATES[controller_id].lock();
            let intr0 = state.intr0;
            if let Some(ring) = state.ring.as_mut() {
                if !intr0.is_null() {
                    unsafe { ring.pop(intr0) }
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(evt) = evt_opt {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type == 34 {
                // Port Status Change Event: must be acked by clearing PORTSC change bits,
                // otherwise the controller can keep generating the same event forever.
                let port_id = (evt.d0 >> 24) as u8;

                // (debug log removed)
                unsafe { clear_port_change_bits(&ctx, port_id) };
                // Drop it; higher layers currently rescan via PORTSC anyway.
                continue;
            }

            enqueue_event(controller_id, evt);
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
}

fn enqueue_event(controller_id: usize, evt: Trb) {
    let mut buf = EVENT_BUFFERS[controller_id].lock();
    buf.push(evt);
}

fn pump_one_event(ctx: &XhciContext) -> bool {
    let controller_id = ctx.controller_id;
    let evt_opt = {
        let mut state = EVENT_RING_STATES[controller_id].lock();
        let intr0 = state.intr0;
        if let Some(ring) = state.ring.as_mut() {
            if !intr0.is_null() {
                unsafe { ring.pop(intr0) }
            } else {
                None
            }
        } else {
            None
        }
    };

    let Some(evt) = evt_opt else {
        return false;
    };

    let evt_type = (evt.d3 >> 10) & 0x3F;
    if evt_type == 34 {
        // Port Status Change Event: must be acked by clearing PORTSC change bits,
        // otherwise the controller can keep generating the same event forever.
        let port_id = (evt.d0 >> 24) as u8;
        unsafe { clear_port_change_bits(ctx, port_id) };
        return true;
    }

    enqueue_event(controller_id, evt);
    true
}

fn try_take_matching_event<F>(controller_id: usize, predicate: &mut F) -> Option<Trb>
where
    F: FnMut(&Trb) -> bool,
{
    let mut buf = EVENT_BUFFERS[controller_id].lock();
    buf.take_matching(predicate)
}

pub fn install_event_ring(ctx: &XhciContext, ring: EventRing, intr0: *mut u32) {
    {
        let mut state = EVENT_RING_STATES[ctx.controller_id].lock();
        state.ring = Some(ring);
        state.intr0 = intr0;
    }
    EVENT_BUFFERS[ctx.controller_id].lock().clear();
}

pub async fn wait_for_event<F>(
    ctx: &XhciContext,
    mut predicate: F,
    timeout_iters: usize,
    delay: EmbassyDuration,
) -> Option<Trb>
where
    F: FnMut(&Trb) -> bool,
{
    let mut polls = 0usize;
    loop {
        if let Some(evt) = try_take_matching_event(ctx.controller_id, &mut predicate) {
            return Some(evt);
        }
        polls += 1;
        if polls > timeout_iters {
            return None;
        }
        Timer::after(delay).await;
    }
}

pub fn wait_for_event_spin<F>(ctx: &XhciContext, mut predicate: F, spin_iters: usize) -> Option<Trb>
where
    F: FnMut(&Trb) -> bool,
{
    let mut polls = 0usize;
    loop {
        if let Some(evt) = try_take_matching_event(ctx.controller_id, &mut predicate) {
            return Some(evt);
        }

        // If we're in a synchronous/blocking context, the async controller poll task may not be
        // getting scheduled. Actively drain the event ring so transfers can still complete.
        let _ = pump_one_event(ctx);

        polls += 1;
        if polls > spin_iters {
            return None;
        }
        core::hint::spin_loop();
    }
}

pub async fn submit_cmd_and_wait(
    ctx: &XhciContext,
    cmd_ring: &mut TrbRing,
    cmd: Trb,
    slot_filter: Option<u32>,
    what: &'static str,
    timeout_iters: usize,
    delay: EmbassyDuration,
) -> Result<Trb, ()> {
    let cmd_phys = match cmd_ring.push_with_phys(cmd) {
        Some(phys) => phys,
        None => {
            crate::log!("xhci: {}: cmd ring full\n", what);
            return Err(());
        }
    };
    unsafe { core::ptr::write_volatile(ctx.doorbell.add(0), 0) };

    let evt = wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 33 {
                return false;
            }
            let evt_cmd_ptr = ((evt.d1 as u64) << 32) | (evt.d0 as u64);
            if (evt_cmd_ptr & !0xF) != (cmd_phys & !0xF) {
                return false;
            }
            if let Some(slot) = slot_filter {
                let evt_slot = (evt.d3 >> 24) & 0xFF;
                evt_slot == slot
            } else {
                true
            }
        },
        timeout_iters,
        delay,
    )
    .await
    .ok_or(())
    .map_err(|_| {
        crate::log!("xhci: {}: timeout waiting for command completion\n", what);
    })?;

    let completion = (evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        let evt_slot = (evt.d3 >> 24) & 0xFF;
        crate::log!(
            "xhci: {} failed cc={} slot={} evt=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
            what,
            completion,
            evt_slot,
            evt.d0,
            evt.d1,
            evt.d2,
            evt.d3,
        );
        return Err(());
    }

    Ok(evt)
}

pub const fn lo(val: u64) -> u32 {
    (val & 0xFFFF_FFFF) as u32
}

pub const fn hi(val: u64) -> u32 {
    (val >> 32) as u32
}

pub const fn trb_type(ty: u32) -> u32 {
    ty << 10
}

pub fn log_ports_table(ctx: &XhciContext) {
    crate::log!(
        "xhci: ports={} (PORTSC: ccs ped pr pp pls speed csc pec prc plc cec)\n",
        ctx.port_count
    );
    crate::log!("xhci: #  PORTSC       ccs ped pr pp pls spd   csc pec prc plc cec\n");

    for port in 0..ctx.port_count {
        let portsc = unsafe { ctx.portsc(port as usize) };

        let ccs = (portsc & (1 << 0)) != 0;
        let ped = (portsc & (1 << 1)) != 0;
        let pr = (portsc & (1 << 4)) != 0;
        let pls = ((portsc >> 5) & 0xF) as u8;
        let pp = (portsc & (1 << 9)) != 0;
        let speed_code = ((portsc >> 10) & 0xF) as u8;
        let speed = match speed_code {
            0 => "none",
            1 => "full",
            2 => "low",
            3 => "high",
            4 => "super",
            5 => "super+",
            _ => "unk",
        };

        let csc = (portsc & (1 << 17)) != 0;
        let pec = (portsc & (1 << 18)) != 0;
        let prc = (portsc & (1 << 21)) != 0;
        let plc = (portsc & (1 << 22)) != 0;
        let cec = (portsc & (1 << 23)) != 0;

        crate::log!(
            "xhci: {:>2}  0x{:08X}  {:>3} {:>3} {:>2} {:>2} 0x{:X}  {:>5}  {:>3} {:>3} {:>3} {:>3} {:>3}\n",
            port + 1,
            portsc,
            ccs as u8,
            ped as u8,
            pr as u8,
            pp as u8,
            pls,
            speed,
            csc as u8,
            pec as u8,
            prc as u8,
            plc as u8,
            cec as u8,
        );

        if ccs {
            if let Some((vid, pid)) = get_port_vidpid(ctx.controller_id, (port + 1) as u8) {
                crate::log!("xhci:      port {} id={:04X}:{:04X}\n", port + 1, vid, pid);
            }
        }
    }
}

pub unsafe fn write_reg64(base: *mut u32, byte_offset: usize, value: u64) {
    let ptr = base.add(byte_offset / 4);
    write_volatile(ptr, lo(value));
    write_volatile(ptr.add(1), hi(value));
}
