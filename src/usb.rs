use crate::{debugconf, dma, osal, xhci};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::{Duration as EmbassyDuration, Timer};

#[repr(C, align(16))]
#[derive(Copy, Clone, Default)]
struct Trb {
    d0: u32,
    d1: u32,
    d2: u32,
    d3: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone, Default)]
struct ErstEntry {
    seg_base_lo: u32,
    seg_base_hi: u32,
    seg_size: u32,
    rsvd: u32,
}

struct TrbRing {
    phys: u64,
    trbs: *mut Trb,
    len: usize,
    enqueue: usize,
    cycle: bool,
}

struct EventRing {
    phys: u64,
    trbs: *mut Trb,
    count: usize,
    dequeue: usize,
    cycle: bool,
}

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
    pub unsafe fn new(info: xhci::ControllerInfo) -> Self {
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

    let max_slots = (ctx.hcsparams1 & 0xFF) as usize;
    let (dcbaa_phys, dcbaa_virt) = match dma::alloc((max_slots + 1) * 8, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc dcbaa\n");
            return;
        }
    };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    const CMD_RING_TRBS: usize = 64;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc cmd ring\n");
            return;
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let mut cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 64;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc event ring\n");
            return;
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let mut event_ring = unsafe {
        EventRing::new(
            evt_phys,
            evt_virt_raw as *mut Trb,
            EVENT_RING_TRBS,
        )
    };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ERST\n");
            return;
        }
    };
    unsafe {
        write_bytes(erst_virt, 0, size_of::<ErstEntry>());
        let entry = &mut *(erst_virt as *mut ErstEntry);
        entry.seg_base_lo = lo(evt_phys);
        entry.seg_base_hi = hi(evt_phys);
        entry.seg_size = EVENT_RING_TRBS as u32;
    }

    // Fresh start: simple poll of port status; more setup will follow.
    for port in 0..ctx.port_count {
        let status = unsafe { ctx.portsc(port as usize) };
        let (connected, enabled, speed) = decode_port_status(status);
        debugconf!(
            "usb: port {:02} status=0x{:08X} connected={} enabled={} speed={}\n",
            port + 1,
            status,
            connected,
            enabled,
            speed
        );
    }

    unsafe {
        write_reg64(ctx.op_base, 0x30, dcbaa_phys);
        write_volatile(ctx.op_base.add(0x38 / 4), 1);

        const IMAN: usize = 0x00 / 4;
        const ERSTSZ: usize = 0x08 / 4;
        const ERSTBA: usize = 0x10 / 4;
        const ERDP: usize = 0x18 / 4;
        let intr0 = ctx.runtime.add(0x20 / 4);
        write_volatile(intr0.add(ERSTSZ), 1);
        write_volatile(intr0.add(ERSTBA), lo(erst_phys));
        write_volatile(intr0.add(ERSTBA + 1), hi(erst_phys));
        event_ring.update_erdp(intr0);
        const IMAN_IE: u32 = 1 << 1;
        write_volatile(intr0.add(IMAN), IMAN_IE);

        write_reg64(ctx.op_base, 0x18, cmd_ring.crcr_value());

        const USBCMD: usize = 0x00 / 4;
        const USBSTS: usize = 0x04 / 4;
        const USBCMD_RS: u32 = 1 << 0;
        const USBCMD_INTE: u32 = 1 << 2;
        const USBSTS_HCH: u32 = 1 << 0;
        write_volatile(ctx.op_base.add(USBCMD), USBCMD_RS | USBCMD_INTE);
        let mut spin: u32 = 1_000_000;
        while spin > 0 {
            let sts = read_volatile(ctx.op_base.add(USBSTS));
            if (sts & USBSTS_HCH) == 0 {
                break;
            }
            spin -= 1;
        }
    }

    // Issue ENABLE_SLOT to get a slot id.
    let enable_slot = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(9),
    };
    if !cmd_ring.push(enable_slot) {
        debugconf!("usb: cmd ring full before enable-slot\n");
        return;
    }
    unsafe {
        write_volatile(ctx.doorbell.add(0), 0);
    }

    let mut slot_id = 0u32;
    let mut polls = 0;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            if evt_type == 33 && completion == 1 {
                slot_id = (evt.d3 >> 24) & 0xFF;
                debugconf!("usb: enable-slot ok slot={}\n", slot_id);
                break;
            }
            debugconf!(
                "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                evt_type,
                completion,
                evt.d0,
                evt.d1,
                evt.d2,
                evt.d3
            );
        }

        polls += 1;
        if polls > 400 {
            debugconf!("usb: timeout waiting for enable-slot completion\n");
            break;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    if slot_id == 0 {
        return;
    }

    // Prepare contexts for port 5 (index 4) only.
    let target_port: u8 = 5;
    let port_idx = (target_port - 1) as usize;
    let port_status = unsafe { ctx.portsc(port_idx) };
    let (_, _, _speed_str) = decode_port_status(port_status);
    let speed_code = (port_status >> 10) & 0xF;
    let max_packet = match speed_code {
        2 => 8,   // low speed
        1 => 8,   // full speed
        3 => 64,  // high speed
        4 => 512, // super speed
        _ => 8,
    } as u16;

    let (dev_ctx_phys, dev_ctx_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc device context\n");
            return;
        }
    };
    unsafe { write_bytes(dev_ctx_virt, 0, 4096) };

    let (input_ctx_phys, input_ctx_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc input context\n");
            return;
        }
    };
    unsafe { write_bytes(input_ctx_virt, 0, 4096) };

    // Control endpoint transfer ring.
    const EP0_TRBS: usize = 32;
    let (ep0_phys, ep0_virt_raw) = match dma::alloc(EP0_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ep0 ring\n");
            return;
        }
    };
    unsafe { write_bytes(ep0_virt_raw, 0, EP0_TRBS * size_of::<Trb>()) };
    let mut ep0_ring = unsafe { TrbRing::new(ep0_phys, ep0_virt_raw as *mut Trb, EP0_TRBS) };

    // Populate DCBAA entry for slot.
    unsafe {
        let dcbaa = dcbaa_virt as *mut u64;
        *dcbaa.add(slot_id as usize) = dev_ctx_phys;
    }

    // Build input context: add flags, slot context, ep0 context.
    unsafe {
        let add_flags_ptr = input_ctx_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), 0x3); // add slot + ep0

        let slot_ctx = input_ctx_virt.add(0x20) as *mut u32;
        let ep0_ctx = input_ctx_virt.add(0x40) as *mut u32;

        // Slot context
        let route_speed_ctx_entries = (speed_code << 20) | (1 << 27);
        write_volatile(slot_ctx.add(0), route_speed_ctx_entries);
        let root_port = (target_port as u32) << 16;
        write_volatile(slot_ctx.add(1), root_port);

        // EP0 context
        const EP_TYPE_CONTROL: u32 = 4;
        let ep_type_field = EP_TYPE_CONTROL << 16;
        write_volatile(ep0_ctx.add(0), ep_type_field);
        let max_packet_field = max_packet as u32;
        write_volatile(ep0_ctx.add(1), max_packet_field);
        let dq = ep0_ring.dequeue_ptr();
        write_volatile(ep0_ctx.add(2), lo(dq));
        write_volatile(ep0_ctx.add(3), hi(dq));
        write_volatile(ep0_ctx.add(4), 8); // average TRB length = 8 for control
    }

    // ADDRESS_DEVICE command referencing the input context.
    let addr_dev = Trb {
        d0: lo(input_ctx_phys),
        d1: hi(input_ctx_phys),
        d2: 0,
        d3: trb_type(11) | (slot_id << 24),
    };
    if !cmd_ring.push(addr_dev) {
        debugconf!("usb: cmd ring full before address-device\n");
        return;
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    polls = 0;
    let mut addressed = false;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            if evt_type == 33 && completion == 1 && evt_slot == slot_id {
                addressed = true;
                debugconf!("usb: address-device ok slot={}\n", slot_id);
                break;
            }
            debugconf!(
                "usb: unexpected event type={} cc={} slot={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                evt_type,
                completion,
                evt_slot,
                evt.d0,
                evt.d1,
                evt.d2,
                evt.d3
            );
        }
        polls += 1;
        if polls > 400 {
            debugconf!("usb: timeout waiting for address-device\n");
            break;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    if !addressed {
        return;
    }

    // Buffer for device descriptor.
    let (desc_phys, desc_virt) = match dma::alloc(64, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc desc buffer\n");
            return;
        }
    };
    unsafe { write_bytes(desc_virt, 0, 64) };

    // Control transfer: GET_DESCRIPTOR(Device, 0, len=18)
    let setup = Trb {
        d0: 0x0680 | (0x0100 << 16), // bmRequestType=0x80, bRequest=6, wValue=0x0100 (device descriptor)
        d1: 18 << 16,                // wIndex=0, wLength=18
        d2: 8 | (2 << 16),           // length=8, TRT=IN
        d3: trb_type(2) | (1 << 6),  // setup stage with IDT
    };

    let data = Trb {
        d0: lo(desc_phys),
        d1: hi(desc_phys),
        d2: 18,
        d3: trb_type(3) | (1 << 16) | (1 << 5), // data stage IN, IOC
    };

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5), // status stage, IOC
    };

    if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
        debugconf!("usb: ep0 ring overflow for setup\n");
        return;
    }

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    polls = 0;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            if evt_type == 32 {
                debugconf!(
                    "usb: transfer event cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                    completion,
                    evt.d0,
                    evt.d1,
                    evt.d2,
                    evt.d3
                );
                break;
            }
            debugconf!(
                "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                evt_type,
                completion,
                evt.d0,
                evt.d1,
                evt.d2,
                evt.d3
            );
        }
        polls += 1;
        if polls > 800 {
            debugconf!("usb: timeout waiting for transfer event\n");
            break;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    let first = unsafe { *desc_virt };
    debugconf!("usb: device descriptor first byte=0x{:02X}\n", first);

    // Fetch configuration descriptor (first 9 bytes to learn total length).
    let (cfg_phys, cfg_virt) = match dma::alloc(256, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc cfg buffer\n");
            return;
        }
    };
    unsafe { write_bytes(cfg_virt, 0, 256) };

    async fn get_cfg(
        ctx: &XhciContext,
        event_ring: &mut EventRing,
        ep0_ring: &mut TrbRing,
        slot_id: u32,
        cfg_phys: u64,
        length: u16,
    ) -> Result<(), ()> {
        let setup = Trb {
            d0: 0x0680 | (0x0200 << 16), // GET_DESCRIPTOR, config
            d1: (length as u32) << 16,
            d2: 8 | (2 << 16),
            d3: trb_type(2) | (1 << 6),
        };
        let data = Trb {
            d0: lo(cfg_phys),
            d1: hi(cfg_phys),
            d2: length as u32,
            d3: trb_type(3) | (1 << 16) | (1 << 5),
        };
        let status = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(4) | (1 << 5),
        };
        if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
            debugconf!("usb: ep0 ring overflow for config\n");
            return Err(());
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

        let mut polls_cfg = 0;
        loop {
            if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                let completion = (evt.d2 >> 24) & 0xFF;
                if evt_type == 32 {
                    debugconf!(
                        "usb: cfg transfer cc={} len={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        completion,
                        length,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    if completion == 1 {
                        return Ok(());
                    }
                    return Err(());
                }
            }
            polls_cfg += 1;
            if polls_cfg > 800 {
                debugconf!("usb: timeout waiting for cfg transfer len={}\n", length);
                return Err(());
            }
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
    }

    let mut cfg_total_len: u16 = 0;
    if get_cfg(&ctx, &mut event_ring, &mut ep0_ring, slot_id, cfg_phys, 9).await.is_ok() {
        cfg_total_len = unsafe { *(cfg_virt.add(2) as *const u16) };
        debugconf!("usb: config total length={}\n", cfg_total_len);
        let req_len = cfg_total_len.min(256) as u16;
        if req_len > 9 {
            let _ = get_cfg(&ctx, &mut event_ring, &mut ep0_ring, slot_id, cfg_phys, req_len).await;
        }
    }

    // Parse config to find first HID boot interrupt IN endpoint.
    let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_total_len as usize) };
    let ep_info = parse_hid_boot_ep(cfg_slice);
    let Some(ep) = ep_info else {
        debugconf!("usb: no HID boot interrupt IN endpoint found\n");
        return;
    };
    debugconf!(
        "usb: hid ep addr=0x{:02X} maxpkt={} interval={} iface={} cfg={}\n",
        ep.address,
        ep.max_packet,
        ep.interval,
        ep.interface,
        ep.configuration
    );

    // SET_CONFIGURATION (standard request) to chosen configuration.
    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((ep.configuration as u32) << 16),
        d1: 0,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        debugconf!("usb: ep0 ring overflow for set_configuration\n");
        return;
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let mut polls_cfg = 0;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            if evt_type == 32 {
                debugconf!("usb: set-configuration cc={}\n", completion);
                if completion != 1 {
                    return;
                }
                break;
            }
        }
        polls_cfg += 1;
        if polls_cfg > 400 {
            debugconf!("usb: timeout waiting for set-configuration\n");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    // Configure interrupt IN endpoint for the found HID interface.
    let (ep_ring_phys, ep_ring_virt) = match dma::alloc(32 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ep ring\n");
            return;
        }
    };
    unsafe { write_bytes(ep_ring_virt, 0, 32 * size_of::<Trb>()) };
    let mut ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, 32) };

    let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc input ctx for cfg-ep\n");
            return;
        }
    };
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    // Add flags: slot + endpoint.
    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        let ep_id = endpoint_target(ep.address);
        write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_id));

        let slot_ctx = input_cfg_virt.add(0x20) as *mut u32;
        let ep_ctx = input_cfg_virt.add(0x20 * ep_id) as *mut u32;

        let route_speed_ctx_entries = (speed_code << 20) | (ep_id as u32 + 1) << 27;
        write_volatile(slot_ctx.add(0), route_speed_ctx_entries);
        let root_port = (target_port as u32) << 16;
        write_volatile(slot_ctx.add(1), root_port);

        const EP_TYPE_INT_IN: u32 = 5 << 16;
        let mps = ep.max_packet as u32 & 0x7FF;
        let interval = ep.interval as u32;
        write_volatile(ep_ctx.add(0), EP_TYPE_INT_IN | (3 << 1)); // CErr=3
        write_volatile(ep_ctx.add(1), mps | (interval << 16));
        let dq = ep_ring.dequeue_ptr();
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));
        write_volatile(ep_ctx.add(4), ep.max_packet as u32);
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    if !cmd_ring.push(cfg_ep_cmd) {
        debugconf!("usb: cmd ring full before configure-endpoint\n");
        return;
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    polls_cfg = 0;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            if evt_type == 33 {
                debugconf!("usb: configure-endpoint cc={} slot={}\n", completion, slot_id);
                if completion != 1 {
                    return;
                }
                break;
            }
        }
        polls_cfg += 1;
        if polls_cfg > 400 {
            debugconf!("usb: timeout waiting for configure-endpoint\n");
            return;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    // Arm one interrupt IN transfer to read a boot report.
    let (rep_phys, rep_virt) = match dma::alloc(16, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc report buffer\n");
            return;
        }
    };
    unsafe { write_bytes(rep_virt, 0, 16) };

    let normal = Trb {
        d0: lo(rep_phys),
        d1: hi(rep_phys),
        d2: ep.max_packet as u32,
        d3: trb_type(1) | (1 << 5),
    };
    if !ep_ring.push(normal) {
        debugconf!("usb: ep ring full before interrupt IN\n");
        return;
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), endpoint_target(ep.address) as u32) };

    polls_cfg = 0;
    loop {
        if let Some(evt) = unsafe { event_ring.pop(ctx.runtime.add(0x20 / 4)) } {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            let completion = (evt.d2 >> 24) & 0xFF;
            if evt_type == 32 {
                debugconf!("usb: interrupt IN cc={} len={}\n", completion, ep.max_packet);
                let data = unsafe { core::slice::from_raw_parts(rep_virt, ep.max_packet as usize) };
                debugconf!("usb: report bytes: {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X} {:02X}\n",
                    data[0],data[1],data[2],data[3],data[4],data[5],data[6],data[7]);
                break;
            }
        }
        polls_cfg += 1;
        if polls_cfg > 800 {
            debugconf!("usb: timeout waiting for interrupt IN\n");
            break;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    // Placeholder async wait to keep task alive while we flesh out rings/commands.
    loop {
        Timer::after(EmbassyDuration::from_millis(500)).await;
    }
}

fn decode_port_status(status: u32) -> (bool, bool, &'static str) {
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
    unsafe fn new(phys: u64, trbs: *mut Trb, len: usize) -> Self {
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
            d3: trb_type(TRB_TYPE_LINK) | (1 << 1), // toggle cycle on wrap
        };
        link.d3 |= 1; // ring starts with cycle bit set
        write_volatile(link_ptr, link);
    }

    fn push(&mut self, mut trb: Trb) -> bool {
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

    fn crcr_value(&self) -> u64 {
        self.phys | if self.cycle { 1 } else { 0 }
    }

    fn dequeue_ptr(&self) -> u64 {
        // TR Dequeue Pointer format: bit0 = DCS; bits 3:1 reserved; pointer bits 63:4.
        (self.phys & !0xF) | 1
    }
}

impl EventRing {
    /// # Safety
    /// Caller must ensure `trbs` points to a DMA region with `count` TRBs.
    unsafe fn new(phys: u64, trbs: *mut Trb, count: usize) -> Self {
        EventRing {
            phys,
            trbs,
            count,
            dequeue: 0,
            cycle: true,
        }
    }

    unsafe fn update_erdp(&self, intr0: *mut u32) {
        const ERDP: usize = 0x18 / 4;
        let ptr = self.phys + (self.dequeue as u64 * size_of::<Trb>() as u64);
        write_volatile(intr0.add(ERDP + 1), hi(ptr));
        write_volatile(intr0.add(ERDP), lo(ptr) | (1 << 3));
    }

    unsafe fn pop(&mut self, intr0: *mut u32) -> Option<Trb> {
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
}

const fn lo(val: u64) -> u32 {
    (val & 0xFFFF_FFFF) as u32
}

const fn hi(val: u64) -> u32 {
    (val >> 32) as u32
}

const fn trb_type(ty: u32) -> u32 {
    ty << 10
}

unsafe fn write_reg64(base: *mut u32, byte_offset: usize, value: u64) {
    let ptr = base.add(byte_offset / 4);
    write_volatile(ptr, lo(value));
    write_volatile(ptr.add(1), hi(value));
}
