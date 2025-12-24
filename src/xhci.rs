use core::mem::size_of;
use core::ptr::{null_mut, read_volatile, write_volatile, NonNull};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
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

            let map_len = if size == 0 {
                0x10_000usize
            } else {
                size as usize
            };
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
                let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
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

                bootstrap_ports(cap as usize, caplength as usize, hcsparams1);
            }

            break;
        }
    });

    if !did_any {
        crate::debugconf!("xhci: not found\n");
    }
}

fn bootstrap_ports(cap_base: usize, cap_length: usize, hcsparams1: u32) {
    const PORT_BLOCK_OFFSET: usize = 0x400;
    const PORT_STRIDE: usize = 0x10;
    const PORTSC_CCS: u32 = 1 << 0;
    const PORTSC_PED: u32 = 1 << 1;
    const PORTSC_PR: u32 = 1 << 4;
    const PORTSC_PP: u32 = 1 << 9;

    let port_count = ((hcsparams1 >> 24) & 0xFF) as usize;
    if port_count == 0 {
        crate::debugconf!("xhci: no ports to bootstrap\n");
        return;
    }

    let op_base = cap_base.saturating_add(cap_length);
    let port_base = op_base.saturating_add(PORT_BLOCK_OFFSET);

    crate::debugconf!("xhci: bootstrapping {} port(s)\n", port_count);

    for port_idx in 0..port_count {
        let port_ptr = (port_base + port_idx * PORT_STRIDE) as *mut u32;
        let mut status = unsafe { read_volatile(port_ptr) };
        if (status & PORTSC_PP) == 0 {
            unsafe { write_volatile(port_ptr, status | PORTSC_PP) };
            status |= PORTSC_PP;
        }
        if (status & PORTSC_CCS) == 0 {
            continue;
        }
        if (status & PORTSC_PED) != 0 {
            continue;
        }

        unsafe { write_volatile(port_ptr, status | PORTSC_PR) };
        let mut spin: u32 = 1_000_000;
        while spin > 0 {
            let poll = unsafe { read_volatile(port_ptr) };
            let reset_cleared = (poll & PORTSC_PR) == 0;
            let enabled = (poll & PORTSC_PED) != 0;
            if enabled || reset_cleared {
                break;
            }
            spin -= 1;
        }
        let final_status = unsafe { read_volatile(port_ptr) };
        crate::debugconf!(
            "xhci: port {:02} bootstrap done status=0x{:08X}\n",
            port_idx + 1,
            final_status
        );
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

pub struct EventRing {
    pub phys: u64,
    pub trbs: *mut Trb,
    pub count: usize,
    dequeue: usize,
    cycle: bool,
}

unsafe impl Send for EventRing {}

const EVENT_BUFFER_CAP: usize = 128;

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

static EVENT_BUFFER: Mutex<EventBuffer> = Mutex::new(EventBuffer::new());

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

static EVENT_RING_STATE: Mutex<EventRingState> = Mutex::new(EventRingState::new());

#[derive(Copy, Clone, Debug)]
pub struct XhciContext {
    pub caplength: u8,
    pub hci_version: u16,
    pub hcsparams1: u32,
    pub hccparams1: u32,
    pub op_base: *mut u32,
    pub doorbell: *mut u32,
    pub runtime: *mut u32,
    pub port_count: u8,
}

impl XhciContext {
    /// # Safety
    /// Caller must ensure `info.mmio_base` is a valid mapped MMIO pointer.
    pub unsafe fn new(info: ControllerInfo) -> Self {
        let cap = info.mmio_base.as_ptr();
        let caplength = read_volatile(cap.add(0x00) as *const u8);
        let hci_version = read_volatile(cap.add(0x02) as *const u16);
        let hcsparams1 = read_volatile(cap.add(0x04) as *const u32);
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
            hccparams1,
            op_base,
            doorbell,
            runtime,
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
    let ep_num = (ep_addr & 0x0F) as u32;
    let dir_in = (ep_addr & 0x80) != 0;
    if ep_num == 0 {
        1
    } else {
        ep_num * 2 + if dir_in { 1 } else { 0 }
    }
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

    pub fn push(&mut self, mut trb: Trb) -> bool {
        if self.len < 2 {
            return false;
        }

        let usable = self.len - 1;
        if self.enqueue >= usable {
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }

        trb.d3 = (trb.d3 & !1) | (self.cycle as u32);
        unsafe { write_volatile(self.trbs.add(self.enqueue), trb) };
        self.enqueue += 1;
        if self.enqueue >= usable {
            self.enqueue = 0;
            self.cycle = !self.cycle;
        }
        true
    }

    pub fn crcr_value(&self) -> u64 {
        self.phys | if self.cycle { 1 } else { 0 }
    }

    pub fn dequeue_ptr(&self) -> u64 {
        (self.phys & !0xF) | 1
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

#[embassy_executor::task]
pub async fn controller_poll_task(info: ControllerInfo) {
    crate::debugconf!(
        "xhci: controller poll task running bus={:02X} slot={:02X} fn={}\n",
        info.bus,
        info.slot,
        info.function
    );

    loop {
        let evt_opt = {
            let mut state = EVENT_RING_STATE.lock();
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
            enqueue_event(evt);
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
}

fn enqueue_event(evt: Trb) {
    let mut buf = EVENT_BUFFER.lock();
    buf.push(evt);
}

fn try_take_matching_event<F>(predicate: &mut F) -> Option<Trb>
where
    F: FnMut(&Trb) -> bool,
{
    let mut buf = EVENT_BUFFER.lock();
    buf.take_matching(predicate)
}

pub fn install_event_ring(ring: EventRing, intr0: *mut u32) {
    {
        let mut state = EVENT_RING_STATE.lock();
        state.ring = Some(ring);
        state.intr0 = intr0;
    }
    EVENT_BUFFER.lock().clear();
}

pub async fn wait_for_event<F>(
    mut predicate: F,
    timeout_iters: usize,
    delay: EmbassyDuration,
) -> Option<Trb>
where
    F: FnMut(&Trb) -> bool,
{
    let mut polls = 0usize;
    loop {
        if let Some(evt) = try_take_matching_event(&mut predicate) {
            return Some(evt);
        }
        polls += 1;
        if polls > timeout_iters {
            return None;
        }
        Timer::after(delay).await;
    }
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

pub unsafe fn write_reg64(base: *mut u32, byte_offset: usize, value: u64) {
    let ptr = base.add(byte_offset / 4);
    write_volatile(ptr, lo(value));
    write_volatile(ptr.add(1), hi(value));
}
