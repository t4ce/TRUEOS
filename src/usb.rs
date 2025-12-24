use crate::{debugconf, dma, hid::{self, HidRuntime}, osal, xhci};
use crate::xhci::{
    decode_port_status,
    endpoint_target,
    context_index,
    write_reg64,
    trb_type,
    lo,
    hi,
    EventRing,
    Trb,
    TrbRing,
    XhciContext,
    ErstEntry,
};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::{Duration as EmbassyDuration, Timer};


#[embassy_executor::task]
pub async fn usb_scout(info: xhci::ControllerInfo) {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
    debugconf!(
        "usb: xhci caps len=0x{:X} ver=0x{:04X} ports={} ac64={} csz_64={}\n",
        ctx.caplength,
        ctx.hci_version,
        ctx.port_count,
        (ctx.hccparams1 & 0x1) != 0,
        csz_64
    );
    // Size of each context entry depends on controller capability (CSZ bit).
    let ctx_stride_bytes: usize = if csz_64 { 0x40 } else { 0x20 };
    let ctx_stride_words: usize = ctx_stride_bytes / 4;

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

    // Fresh start: simple poll of port status; remember the first connected device.
    let mut selected_port: Option<(u8, u32)> = None;
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
        if connected && selected_port.is_none() {
            selected_port = Some(((port + 1) as u8, status));
        }
    }

    let (target_port, mut port_status) = match selected_port {
        Some(pair) => pair,
        None => {
            debugconf!("usb: no connected devices detected\n");
            return;
        }
    };

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
        xhci::install_event_ring(event_ring, intr0);
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

    let Some(enable_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type == 33 {
                true
            } else {
                let completion = (evt.d2 >> 24) & 0xFF;
                debugconf!(
                    "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                    evt_type,
                    completion,
                    evt.d0,
                    evt.d1,
                    evt.d2,
                    evt.d3
                );
                false
            }
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for enable-slot completion\n");
        return;
    };

    let completion = (enable_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        debugconf!(
            "usb: enable-slot failed cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
            completion,
            enable_evt.d0,
            enable_evt.d1,
            enable_evt.d2,
            enable_evt.d3
        );
        return;
    }

    let slot_id = (enable_evt.d3 >> 24) & 0xFF;
    if slot_id == 0 {
        debugconf!("usb: enable-slot returned slot 0\n");
        return;
    }
    debugconf!("usb: enable-slot ok slot={}\n", slot_id);

    // Prepare contexts for the selected port only.
    let port_idx = (target_port - 1) as usize;
    const PORTSC_PED: u32 = 1 << 1;
    const PORTSC_PR: u32 = 1 << 4;
    unsafe {
        ctx.reset_port(port_idx);
    }
    let mut reset_polls = 0;
    loop {
        port_status = unsafe { ctx.portsc(port_idx) };
        let pr_clear = (port_status & PORTSC_PR) == 0;
        let ped_set = (port_status & PORTSC_PED) != 0;
        if pr_clear && ped_set {
            break;
        }
        reset_polls += 1;
        if reset_polls > 400 {
            debugconf!(
                "usb: port {} reset timed out status=0x{:08X}\n",
                target_port,
                port_status
            );
            return;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }
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

        let slot_ctx = input_ctx_virt.add(ctx_stride_bytes) as *mut u32;
        let ep0_ctx = input_ctx_virt.add(ctx_stride_bytes * 2) as *mut u32;

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

        debugconf!(
            "usb: pre-address slot ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X}\n",
            read_volatile(slot_ctx.add(0)),
            read_volatile(slot_ctx.add(1)),
            read_volatile(slot_ctx.add(2)),
            read_volatile(slot_ctx.add(3))
        );
        debugconf!(
            "usb: pre-address ep0 ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} dw4=0x{:08X}\n",
            read_volatile(ep0_ctx.add(0)),
            read_volatile(ep0_ctx.add(1)),
            read_volatile(ep0_ctx.add(2)),
            read_volatile(ep0_ctx.add(3)),
            read_volatile(ep0_ctx.add(4))
        );
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

        let Some(addr_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type == 33 {
                true
            } else {
                let completion = (evt.d2 >> 24) & 0xFF;
                let evt_slot = (evt.d3 >> 24) & 0xFF;
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
                false
            }
        },
        400,
        EmbassyDuration::from_millis(500)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for address-device\n");
        return;
    };

    let completion = (addr_evt.d2 >> 24) & 0xFF;
    let evt_slot = (addr_evt.d3 >> 24) & 0xFF;
    if completion != 1 || evt_slot != slot_id {
        debugconf!(
            "usb: address-device unexpected completion cc={} slot={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
            completion,
            evt_slot,
            addr_evt.d0,
            addr_evt.d1,
            addr_evt.d2,
            addr_evt.d3
        );
        return;
    }
    debugconf!("usb: address-device ok slot={}\n", slot_id);

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

    let Some(desc_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type == 32 {
                true
            } else {
                let completion = (evt.d2 >> 24) & 0xFF;
                debugconf!(
                    "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                    evt_type,
                    completion,
                    evt.d0,
                    evt.d1,
                    evt.d2,
                    evt.d3
                );
                false
            }
        },
        800,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for transfer event\n");
        return;
    };
    debugconf!(
        "usb: transfer event cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
        (desc_evt.d2 >> 24) & 0xFF,
        desc_evt.d0,
        desc_evt.d1,
        desc_evt.d2,
        desc_evt.d3
    );

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

        let Some(evt) = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type == 32 {
                    true
                } else {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    debugconf!(
                        "usb: unexpected cfg event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        evt_type,
                        completion,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    false
                }
            },
            800,
            EmbassyDuration::from_millis(5)
        )
        .await
        else {
            debugconf!("usb: timeout waiting for cfg transfer len={}\n", length);
            return Err(());
        };

        let completion = (evt.d2 >> 24) & 0xFF;
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
            Ok(())
        } else {
            Err(())
        }
    }

    let mut cfg_total_len: u16 = 0;
    if get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, 9).await.is_ok() {
        cfg_total_len = unsafe { *(cfg_virt.add(2) as *const u16) };
        debugconf!("usb: config total length={}\n", cfg_total_len);
        let req_len = cfg_total_len.min(256) as u16;
        if req_len > 9 {
            let _ = get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, req_len).await;
        }
    }

    // Dump the fetched config descriptor set for debugging.
    {
        let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_total_len as usize) };
        let mut idx = 0usize;
        while idx + 2 <= cfg_slice.len() {
            let len = cfg_slice[idx] as usize;
            if len == 0 || idx + len > cfg_slice.len() {
                break;
            }
            let ty = cfg_slice[idx + 1];
            debugconf!("usb: cfg desc idx={} len={} ty=0x{:02X}\n", idx, len, ty);
            let mut off = 0usize;
            while off < len {
                let b = cfg_slice[idx + off];
                debugconf!("usb:   b[{}]={:02X}\n", off, b);
                off += 1;
            }
            idx += len;
        }
    }

    // Parse config to find first HID boot interrupt IN endpoint.
    let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_total_len as usize) };
    let ep_info = hid::parse_boot_endpoint(cfg_slice);
    let Some(ep) = ep_info else {
        debugconf!("usb: no HID boot interrupt IN endpoint found\n");
        return;
    };
    debugconf!(
        "usb: hid ep addr=0x{:02X} maxpkt={} interval={} iface={} cfg={} proto={}\n",
        ep.address,
        ep.max_packet,
        ep.interval,
        ep.interface,
        ep.configuration,
        ep.protocol
    );
    let hid_kind = hid::hid_kind_from_protocol(ep.protocol);

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
    let Some(set_cfg_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 32
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for set-configuration\n");
        return;
    };


    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    debugconf!("usb: set-configuration cc={}\n", completion);
    if completion != 1 {
        return;
    }

    let _ = hid::class_request_nodata(
        &ctx,
        &mut ep0_ring,
        slot_id,
        0x0B, // SET_PROTOCOL
        0,
        ep.interface as u16,
    )
    .await;

    let _ = hid::class_request_nodata(
        &ctx,
        &mut ep0_ring,
        slot_id,
        0x0A, // SET_IDLE
        0,
        ep.interface as u16,
    )
    .await;

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

    // Add flags + endpoint context rebuilt with experimental settings.
    let ep_target = endpoint_target(ep.address);
    let ep_ctx_index = context_index(ep.address);

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        // Add only slot + new endpoint (omit ep0 in add-flags).
        write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_ctx_index));

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

        // Copy current slot context from device context to preserve address/state.
        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        // Context Entries: try ep_ctx_index + 1 for this run.
        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | ((ep_ctx_index + 1) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        // Ensure root port value is preserved (dw1 bits 23:16).
        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        const EP_TYPE_INT_IN: u32 = 7;
        let mps = (ep.max_packet as u32) & 0x7FF;
        let interval = if speed_code == 3 {
            core::cmp::min(15u32, ep.interval.saturating_sub(1) as u32)
        } else {
            ep.interval as u32
        };

        // Build endpoint context with conservative payload and DCS=0 on TR dequeue.
        write_volatile(ep_ctx.add(0), interval << 16);
        write_volatile(ep_ctx.add(1), (mps << 16) | (EP_TYPE_INT_IN << 3) | (3 << 1));
        let dq = ep_ring.phys & !0xF; // DCS = 0 for this experiment
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));
        let avg_trb_len = 8u32;
        let max_esit_payload = 4u32;
        write_volatile(ep_ctx.add(4), (avg_trb_len << 16) | max_esit_payload);

        debugconf!(
            "usb: input add_flags=0x{:08X} drop_flags=0x{:08X}\n",
            read_volatile(add_flags_ptr.add(1)),
            read_volatile(add_flags_ptr)
        );
        debugconf!(
            "usb: input slot ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X}\n",
            read_volatile(slot_ctx.add(0)),
            read_volatile(slot_ctx.add(1)),
            read_volatile(slot_ctx.add(2)),
            read_volatile(slot_ctx.add(3))
        );
        debugconf!(
            "usb: input ep ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} dw4=0x{:08X}\n",
            read_volatile(ep_ctx.add(0)),
            read_volatile(ep_ctx.add(1)),
            read_volatile(ep_ctx.add(2)),
            read_volatile(ep_ctx.add(3)),
            read_volatile(ep_ctx.add(4)),
        );
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

    let Some(cfg_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 33
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for configure-endpoint\n");
        return;
    };

    let completion = (cfg_evt.d2 >> 24) & 0xFF;
    debugconf!(
        "usb: configure-endpoint cc={} slot={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
        completion,
        slot_id,
        cfg_evt.d0,
        cfg_evt.d1,
        cfg_evt.d2,
        cfg_evt.d3
    );
    unsafe {
        let slot_ctx = dev_ctx_virt as *const u32;
        let ep0_ctx = dev_ctx_virt.add(ctx_stride_bytes) as *const u32;
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = dev_ctx_virt.add(ep_ctx_off) as *const u32;
        debugconf!(
            "usb: slot ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X}\n",
            read_volatile(slot_ctx.add(0)),
            read_volatile(slot_ctx.add(1)),
            read_volatile(slot_ctx.add(2)),
            read_volatile(slot_ctx.add(3))
        );
        debugconf!(
            "usb: ep0 ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} dw4=0x{:08X}\n",
            read_volatile(ep0_ctx.add(0)),
            read_volatile(ep0_ctx.add(1)),
            read_volatile(ep0_ctx.add(2)),
            read_volatile(ep0_ctx.add(3)),
            read_volatile(ep0_ctx.add(4)),
        );
        debugconf!(
            "usb: ep ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} dw3=0x{:08X} dw4=0x{:08X}\n",
            read_volatile(ep_ctx.add(0)),
            read_volatile(ep_ctx.add(1)),
            read_volatile(ep_ctx.add(2)),
            read_volatile(ep_ctx.add(3)),
            read_volatile(ep_ctx.add(4)),
        );
    }
    if completion != 1 {
        return;
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

    hid::register_runtime(HidRuntime {
        ep,
        report_virt: rep_virt,
        hid_kind,
    });

    let normal = Trb {
        d0: lo(rep_phys),
        d1: hi(rep_phys),
        d2: 8,
        d3: trb_type(1) | (1 << 5),
    };
    if !ep_ring.push(normal) {
        debugconf!("usb: ep ring full before interrupt IN\n");
        return;
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

}

#[embassy_executor::task]
pub async fn usb_init_task(_info: xhci::ControllerInfo) {
    loop {
        let evt_opt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                evt_type == 32
            },
            1000,
            EmbassyDuration::from_millis(5)
        )
        .await;

        if let Some(evt) = evt_opt {
            if let Some(runtime) = hid::runtime() {
                let completion = (evt.d2 >> 24) & 0xFF;
                let data = unsafe {
                    core::slice::from_raw_parts(runtime.report_virt, runtime.ep.max_packet as usize)
                };
                hid::handle_report(&runtime, completion, data);
            } else {
                debugconf!("usb: interrupt IN but no registered HID runtime\n");
            }
            break;
        } else {
            debugconf!("usb: waiting for interrupt \n");
        }
    }
}

