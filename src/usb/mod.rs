pub mod hid;
pub mod pen;
pub mod print;

use crate::{debugconf, dma, osal, xhci};
use crate::xhci::{
    decode_port_status,
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

use self::hid::BootAttachParams;

#[embassy_executor::task]
pub async fn usb_scout(info: xhci::ControllerInfo) {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
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

    let mut selected_port: Option<(u8, u32)> = None;
    for port in 0..ctx.port_count {
        let status = unsafe { ctx.portsc(port as usize) };
        let (connected, _, _) = decode_port_status(status);
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
    let speed_code = (port_status >> 10) & 0xF;
    let max_packet = match speed_code {
        2 => 8,
        1 => 8,
        3 => 64,
        4 => 512,
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

    unsafe {
        let dcbaa = dcbaa_virt as *mut u64;
        *dcbaa.add(slot_id as usize) = dev_ctx_phys;
    }

    unsafe {
        let add_flags_ptr = input_ctx_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), 0x3);

        let slot_ctx = input_ctx_virt.add(ctx_stride_bytes) as *mut u32;
        let ep0_ctx = input_ctx_virt.add(ctx_stride_bytes * 2) as *mut u32;

        let route_speed_ctx_entries = (speed_code << 20) | (1 << 27);
        write_volatile(slot_ctx.add(0), route_speed_ctx_entries);
        let root_port = (target_port as u32) << 16;
        write_volatile(slot_ctx.add(1), root_port);

        const EP_TYPE_CONTROL: u32 = 4;
        let ep_type_field = EP_TYPE_CONTROL << 16;
        write_volatile(ep0_ctx.add(0), ep_type_field);
        let max_packet_field = max_packet as u32;
        write_volatile(ep0_ctx.add(1), max_packet_field);
        let dq = ep0_ring.dequeue_ptr();
        write_volatile(ep0_ctx.add(2), lo(dq));
        write_volatile(ep0_ctx.add(3), hi(dq));
        write_volatile(ep0_ctx.add(4), 8);
    }

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

    let (desc_phys, desc_virt) = match dma::alloc(64, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc desc buffer\n");
            return;
        }
    };
    unsafe { write_bytes(desc_virt, 0, 64) };

    let setup = Trb {
        d0: 0x0680 | (0x0100 << 16),
        d1: 18 << 16,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    };

    let data = Trb {
        d0: lo(desc_phys),
        d1: hi(desc_phys),
        d2: 18,
        d3: trb_type(3) | (1 << 16) | (1 << 5),
    };

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5),
    };

    if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
        debugconf!("usb: ep0 ring overflow for setup\n");
        return;
    }

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let Some(_desc_evt) = xhci::wait_for_event(
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
            d0: 0x0680 | (0x0200 << 16),
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
        if completion == 1 {
            Ok(())
        } else {
            Err(())
        }
    }

    let mut cfg_total_len: u16 = 0;
    if get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, 9).await.is_ok() {
        cfg_total_len = unsafe { *(cfg_virt.add(2) as *const u16) };
        let req_len = cfg_total_len.min(256) as u16;
        if req_len > 9 {
            let _ = get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, req_len).await;
        }
    }

    let cfg_slice_len = cfg_total_len.min(256) as usize;
    let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_slice_len) };

    {
        let mut idx = 0usize;
        while idx + 2 <= cfg_slice.len() {
            let len = cfg_slice[idx] as usize;
            if len == 0 || idx + len > cfg_slice.len() {
                break;
            }
            let ty = cfg_slice[idx + 1];
            debugconf!("usb: cfg desc idx={} len={} ty=0x{:02X}\n", idx, len, ty);
            idx += len;
        }
    }

    if print::try_handle(cfg_slice, target_port) {
        return;
    }

    if pen::try_handle(cfg_slice, target_port) {
        return;
    }

    let attach_params = BootAttachParams {
        ctx: &ctx,
        cmd_ring: &mut cmd_ring,
        ep0_ring: &mut ep0_ring,
        slot_id,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    };

    if hid::attach_boot_device(attach_params).await.is_err() {
        return;
    }
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
            }
            break;
        }
    }
}
