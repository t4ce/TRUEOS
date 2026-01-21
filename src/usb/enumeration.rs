use super::hub::{HubWork, LOG_PORTS_MAX};
use super::xhci;
use super::xhci::{
    decode_port_status, ep_avg_trb_len_bits, ep_cerr_bits, ep_max_packet_bits, ep_state_bits,
    ep_type_bits, hi, lo, trb_type, Trb, TrbRing, XhciContext, EP_STATE_DISABLED,
    EP_STATE_RUNNING, EP_TYPE_CONTROL,
};
use super::{
    attach, control, hub, uac, UsbControllerState, NOT_CLAIMED_COUNT, NOT_CLAIMED_KEY,
};
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
macro_rules! usbv {
    ($($tt:tt)*) => {{
        if super::USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}
use embassy_time::{Duration as EmbassyDuration, Timer};

static LOG_ROOT_SLOT_CTX_ONCE: AtomicBool = AtomicBool::new(false);

async fn submit_cmd_and_wait(
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
            usbv!("usb: {}: cmd ring full\n", what);
            return Err(());
        }
    };
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    let evt = xhci::wait_for_event(
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
        usbv!("usb: {}: timeout waiting for command completion\n", what);
    })?;

    Ok(evt)
}

async fn update_ep0_max_packet(
    ctx: &XhciContext,
    cmd_ring: &mut TrbRing,
    input_ctx_phys: u64,
    input_ctx_virt: *mut u8,
    dev_ctx_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    slot_id: u32,
    new_mps: u16,
) -> Result<(), ()> {
    // Reprogram EP0 Max Packet Size using Evaluate Context, copying current slot/ep0
    // state from the output device context and only overriding the MPS field.
    unsafe {
        write_bytes(input_ctx_virt, 0, 4096);

        let add_flags_ptr = input_ctx_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), 0x3); // slot + ep0

        let slot_src = dev_ctx_virt as *const u32;
        let slot_dst = input_ctx_virt.add(ctx_stride_bytes) as *mut u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_dst.add(i), read_volatile(slot_src.add(i)));
        }

        let ep0_src = dev_ctx_virt.add(ctx_stride_bytes) as *const u32;
        let ep0_dst = input_ctx_virt.add(ctx_stride_bytes * 2) as *mut u32;
        for i in 0..ctx_stride_words {
            write_volatile(ep0_dst.add(i), read_volatile(ep0_src.add(i)));
        }

        let mut dw1 = read_volatile(ep0_dst.add(1));
        dw1 &= !0xFFFF_0000; // clear Max Packet Size field
        dw1 |= ((new_mps as u32) & 0x7FF) << 16;
        write_volatile(ep0_dst.add(1), dw1);
    }

    let eval_evt = submit_cmd_and_wait(
        ctx,
        cmd_ring,
        Trb {
            d0: lo(input_ctx_phys),
            d1: hi(input_ctx_phys),
            d2: 0,
            d3: trb_type(13) | (slot_id << 24),
        },
        Some(slot_id),
        "eval-ctx-ep0",
        800,
        EmbassyDuration::from_millis(5),
    )
    .await?;

    if control::trb_cc(&eval_evt) != 1 {
        usbv!(
            "usb: eval-ctx-ep0 cc={} slot={}\n",
            control::trb_cc(&eval_evt),
            slot_id
        );
        return Err(());
    }

    Ok(())
}

pub(crate) async fn enumerate_port(
    state: &mut UsbControllerState,
    target_port: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
    let ctx = state.ctx;
    let port_idx = (target_port - 1) as usize;
    const PORTSC_PED: u32 = 1 << 1;
    const PORTSC_PR: u32 = 1 << 4;

    usbv!(
        "usb: enum port {} begin portsc=0x{:08X}\n",
        target_port,
        unsafe { ctx.portsc(port_idx) }
    );

    unsafe {
        ctx.ensure_port_powered(port_idx);
        xhci::clear_port_change_bits(&ctx, target_port);
        ctx.reset_port(port_idx);
    }

    let mut port_status: u32;
    let mut reset_polls = 0u32;
    loop {
        port_status = unsafe { ctx.portsc(port_idx) };
        let pr_clear = (port_status & PORTSC_PR) == 0;
        let ped_set = (port_status & PORTSC_PED) != 0;
        if pr_clear && ped_set {
            break;
        }
        reset_polls += 1;
        if reset_polls > 1000 {
            usbv!(
                "usb: port {} reset timed out status=0x{:08X}\n",
                target_port,
                port_status
            );
            return;
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    usbv!(
        "usb: enum port {} reset-ok portsc=0x{:08X}\n",
        target_port,
        port_status
    );

    // Clear any change bits raised during reset (e.g., PRC/CSC) before continuing.
    unsafe {
        xhci::clear_port_change_bits(&ctx, target_port);
    }

    let Some(slot_id) = enable_slot(state, target_port).await else {
        return;
    };

    // From here on: always disable the slot on failure.
    let speed_code = (port_status >> 10) & 0xF;

    enumerate_with_params(
        state,
        target_port,
        slot_id,
        target_port,
        0,
        0,
        speed_code,
        Some(port_status),
        None,
        None,
        0,
        hub_queue,
    )
    .await;
}

fn speed_code_to_str(speed_code: u32) -> &'static str {
    match speed_code {
        1 => "full",
        2 => "low",
        3 => "high",
        4 => "super",
        5 => "super+",
        _ => "unk",
    }
}

fn is_full_or_low_speed(speed_code: u32) -> bool {
    speed_code == 1 || speed_code == 2
}

fn is_superspeed(speed_code: u32) -> bool {
    speed_code == 4 || speed_code == 5
}

fn slot_ctx_dw0(route_string: u32, speed_code: u32) -> u32 {
    // xHCI Slot Context DW0
    // - Route String: hub topology routing (20-bit route string). For devices
    //   directly on a root port, this is 0.
    // - Speed: 4-bit speed ID
    // - Context Entries: 1 means Slot+EP0 are valid in the input context.
    let route_bits = route_string & 0xFFFFF;
    let speed_bits = (speed_code & 0xF) << 20;
    let ctx_entries_bits = (1u32 & 0x1F) << 27;
    route_bits | speed_bits | ctx_entries_bits
}

fn slot_ctx_dw1(root_port: u8, target_port: u8) -> u32 {
    // xHCI Slot Context DW1: Root Hub Port Number in bits 23:16.
    let root_port_num = if root_port == 0 {
        target_port as u32
    } else {
        root_port as u32
    };
    (root_port_num & 0xFF) << 16
}

fn slot_ctx_dw2_tt(tt_hub_slot: u32, tt_port: u8, tt_think_time: u8) -> u32 {
    // xHCI Slot Context DW2 for FS/LS devices behind a HS hub.
    // - TT Hub Slot ID: bits 7:0
    // - TT Port Number: bits 15:8
    // - TT Think Time: bits 17:16
    (tt_hub_slot & 0xFF)
        | (((tt_port as u32) & 0xFF) << 8)
        | (((tt_think_time as u32) & 0x3) << 16)
}

fn hub_child_settle_delay_ms(speed_code: u32) -> u64 {
    // Minimal, speed-scaled settle time after hub port reset/enable.
    // Keep this small; correctness should come from context fields, not sleeps.
    match speed_code {
        1 | 2 => 50,
        3 => 10,
        _ => 20,
    }
}

pub(crate) async fn enumerate_with_params(
    state: &mut UsbControllerState,
    target_port: u8,
    slot_id: u32,
    root_port: u8,
    route_string: u32,
    depth: u8,
    speed_code: u32,
    portsc: Option<u32>,
    tree_parent: Option<(u32, u8)>,
    tt_info: Option<(u32, u8)>,
    tt_think_time: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
    if let Some((hub_slot, hub_port)) = tree_parent {
        crate::log!(
            "usb: hub child enum begin hub_slot={} port={} slot={} route=0x{:X} depth={} speed={}({})\n",
            hub_slot,
            hub_port,
            slot_id,
            route_string,
            depth,
            speed_code_to_str(speed_code),
            speed_code,
        );
        let (tt_slot, tt_port) = tt_info.unwrap_or((0, 0));
        crate::log!(
            "usb: hub child enum ctx root_port={} tt_slot={} tt_port={} tt_think={}\n",
            root_port,
            tt_slot,
            tt_port,
            tt_think_time,
        );

        if !is_full_or_low_speed(speed_code) && tt_info.is_some() {
            crate::log!(
                "usb: hub child note: tt_info present for non-FS/LS speed={}({})\n",
                speed_code_to_str(speed_code),
                speed_code
            );
        }
        if is_full_or_low_speed(speed_code) && tt_info.is_none() && tt_think_time != 0 {
            crate::log!(
                "usb: hub child warn: FS/LS missing tt_info for HS-TT path (speed={}({}) tt_think={})\n",
                speed_code_to_str(speed_code),
                speed_code,
                tt_think_time
            );
        }
    }
    let ctx = state.ctx;
    let dcbaa_virt = state.dcbaa_virt;
    let ctx_stride_bytes = state.ctx_stride_bytes;
    let ctx_stride_words = state.ctx_stride_words;

    let max_packet = match speed_code {
        2 | 1 => 8,
        3 => 64,
        4 => 512,
        _ => 8,
    } as u16;
    let mut ep0_mps = max_packet;

    let (dev_ctx_phys, dev_ctx_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(dev_ctx_virt, 0, 4096) };

    let (input_ctx_phys, input_ctx_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            dma::dealloc(dev_ctx_virt, 4096);
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(input_ctx_virt, 0, 4096) };

    const EP0_TRBS: usize = 32;
    let ep0_bytes = EP0_TRBS * size_of::<Trb>();
    let (ep0_phys, ep0_virt_raw) = match dma::alloc(ep0_bytes, 64) {
        Some(pair) => pair,
        None => {
            dma::dealloc(input_ctx_virt, 4096);
            dma::dealloc(dev_ctx_virt, 4096);
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(ep0_virt_raw, 0, ep0_bytes) };
    let mut ep0_ring = unsafe { TrbRing::new(ep0_phys, ep0_virt_raw as *mut Trb, EP0_TRBS) };

    unsafe {
        let dcbaa = dcbaa_virt as *mut u64;
        *dcbaa.add(slot_id as usize) = dev_ctx_phys;
    }

    let mut slot_dw0: u32 = 0;
    let mut slot_dw1: u32 = 0;
    let mut slot_dw2: u32 = 0;
    let mut ep0_dw0: u32 = 0;
    let mut ep0_dw1: u32 = 0;

    unsafe {
        let add_flags_ptr = input_ctx_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), 0x3);

        let slot_ctx = input_ctx_virt.add(ctx_stride_bytes) as *mut u32;
        let ep0_ctx = input_ctx_virt.add(ctx_stride_bytes * 2) as *mut u32;

        let dw0 = slot_ctx_dw0(route_string, speed_code);
        let dw1 = slot_ctx_dw1(root_port, target_port);

        // Slot Context DW2 is only meaningful for TT scheduling (FS/LS behind a HS hub).
        // For other hub topologies (e.g., a FS hub), DW2 must remain 0.
        let mut dw2 = 0u32;
        if is_full_or_low_speed(speed_code) {
            if let Some((tt_hub_slot, tt_port)) = tt_info {
                dw2 = slot_ctx_dw2_tt(tt_hub_slot, tt_port, tt_think_time);
            }
        }

        write_volatile(slot_ctx.add(0), dw0);
        write_volatile(slot_ctx.add(1), dw1);
        write_volatile(slot_ctx.add(2), dw2);

        slot_dw0 = read_volatile(slot_ctx.add(0));
        slot_dw1 = read_volatile(slot_ctx.add(1));
        slot_dw2 = read_volatile(slot_ctx.add(2));

        write_volatile(ep0_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_cfg = ep_cerr_bits(3);
        ep_cfg |= ep_type_bits(EP_TYPE_CONTROL);
        ep_cfg |= ep_max_packet_bits(max_packet as u32);
        write_volatile(ep0_ctx.add(1), ep_cfg);
        let dq = ep0_ring.dequeue_ptr();
        write_volatile(ep0_ctx.add(2), lo(dq));
        write_volatile(ep0_ctx.add(3), hi(dq));
        let avg_trb_len = core::cmp::max(8u32, max_packet as u32);
        write_volatile(ep0_ctx.add(4), ep_avg_trb_len_bits(avg_trb_len));

        ep0_dw0 = read_volatile(ep0_ctx.add(0));
        ep0_dw1 = read_volatile(ep0_ctx.add(1));
    }

    if tree_parent.is_some() {
        Timer::after(EmbassyDuration::from_millis(hub_child_settle_delay_ms(speed_code))).await;
    }

    // High-signal experiment: keep Address Device semantics identical for root
    // and hub-child devices by not using BSR.
    let use_bsr = false;
    let addr_evt = match submit_cmd_and_wait(
        &ctx,
        &mut state.cmd_ring,
        Trb {
            d0: lo(input_ctx_phys),
            d1: hi(input_ctx_phys),
            d2: 0,
            d3: trb_type(11) | if use_bsr { 1 << 9 } else { 0 } | (slot_id << 24),
        },
        Some(slot_id),
        if use_bsr { "address-device-bsr" } else { "address-device" },
        2000,
        EmbassyDuration::from_millis(5),
    )
    .await
    {
        Ok(evt) => evt,
        Err(()) => {
            usbv!(
                "usb: enum port {} address-device failed slot={}\n",
                target_port,
                slot_id
            );
            if let Some((hub_slot, hub_port)) = tree_parent {
                crate::log!(
                    "usb: hub child address-device timeout hub_slot={} port={} slot={}\n",
                    hub_slot,
                    hub_port,
                    slot_id
                );
            }
            disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
            return;
        }
    };

    if control::trb_cc(&addr_evt) != 1 {
        usbv!(
            "usb: enum port {} address-device cc={} slot={}\n",
            target_port,
            control::trb_cc(&addr_evt),
            slot_id
        );
        if let Some((hub_slot, hub_port)) = tree_parent {
            crate::log!(
                "usb: hub child address-device failed hub_slot={} port={} slot={} cc={} evt=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                hub_slot,
                hub_port,
                slot_id,
                control::trb_cc(&addr_evt),
                addr_evt.d0,
                addr_evt.d1,
                addr_evt.d2,
                addr_evt.d3,
            );
            crate::log!(
                "usb: hub child slot ctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} ep0 dw0=0x{:08X} dw1=0x{:08X}\n",
                slot_dw0,
                slot_dw1,
                slot_dw2,
                ep0_dw0,
                ep0_dw1,
            );
        }
        disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
        return;
    }

    if use_bsr {
        if control::control_out(
            &ctx,
            &mut ep0_ring,
            slot_id,
            control::setup_set_address(slot_id as u8),
            None,
            0,
            "set-address",
            800,
        )
        .await
        .is_err()
        {
            if let Some((hub_slot, hub_port)) = tree_parent {
                crate::log!(
                    "usb: hub child set-address failed hub_slot={} port={} slot={}\n",
                    hub_slot,
                    hub_port,
                    slot_id
                );
            }
            disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
            return;
        }
    }

    if let Some((hub_slot, hub_port)) = tree_parent {
        crate::log!(
            "usb: hub child address-ok hub_slot={} port={} slot={}\n",
            hub_slot,
            hub_port,
            slot_id
        );
    } else if !LOG_ROOT_SLOT_CTX_ONCE.swap(true, Ordering::AcqRel) {
        crate::log!(
            "usb: root address-ok slot={} slotctx dw0=0x{:08X} dw1=0x{:08X} dw2=0x{:08X} ep0 dw0=0x{:08X} dw1=0x{:08X}\n",
            slot_id,
            slot_dw0,
            slot_dw1,
            slot_dw2,
            ep0_dw0,
            ep0_dw1,
        );
    }

    usbv!(
        "usb: enum port {} address-ok slot={}\n",
        target_port,
        slot_id
    );

    let (desc_phys, desc_virt) = match dma::alloc(64, 64) {
        Some(pair) => pair,
        None => {
            disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
            return;
        }
    };
    unsafe { write_bytes(desc_virt, 0, 64) };

    // First grab the 8-byte header to learn bMaxPacketSize0, then, if needed,
    // reprogram EP0 MPS via Evaluate Context before pulling the full descriptor.
    let mut dev_mps0_hdr: u8 = ep0_mps as u8;
    if let Ok((_cc, hdr_xfer)) = control::control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        control::setup_get_descriptor(1, 0, 8),
        desc_phys,
        8,
        "get-devdesc-8",
        800,
    )
    .await
    {
        let hdr_len = (hdr_xfer as usize).min(8);
        let hdr = unsafe { core::slice::from_raw_parts(desc_virt, hdr_len) };
        if hdr_len >= 8 {
            dev_mps0_hdr = hdr[7];
        }
    }

    let desired_mps0 = match dev_mps0_hdr {
        0 => ep0_mps,
        8 | 16 | 32 | 64 => dev_mps0_hdr as u16,
        9 => 512, // SuperSpeed encodes 512-byte EP0 as 9
        _ => ep0_mps,
    };

    if desired_mps0 != ep0_mps {
        if update_ep0_max_packet(
            &ctx,
            &mut state.cmd_ring,
            input_ctx_phys,
            input_ctx_virt,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            slot_id,
            desired_mps0,
        )
        .await
        .is_err()
        {
            disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
            return;
        }
        ep0_mps = desired_mps0;
        usbv!(
            "usb: enum port {} ep0 mps updated to {} slot={}\n",
            target_port,
            ep0_mps,
            slot_id
        );
    }

    unsafe { write_bytes(desc_virt, 0, 64) };

    if control::control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        control::setup_get_descriptor(1, 0, 18),
        desc_phys,
        18,
        "get-devdesc",
        800,
    )
    .await
    .is_err()
    {
        usbv!(
            "usb: enum port {} get-devdesc failed slot={}\n",
            target_port,
            slot_id
        );
        disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
        return;
    }

    usbv!(
        "usb: enum port {} devdesc-ok slot={}\n",
        target_port,
        slot_id
    );

    let (dev_vid, dev_pid, dev_cls, dev_sub, dev_prot, dev_mps0, dev_i_serial, dev_num_cfg) = unsafe {
        let dd = core::slice::from_raw_parts(desc_virt, 18);
        let vid = u16::from_le_bytes([dd[8], dd[9]]);
        let pid = u16::from_le_bytes([dd[10], dd[11]]);
        (vid, pid, dd[4], dd[5], dd[6], dd[7], dd[16], dd[17])
    };

    let dev_serial = control::fetch_serial_string(&ctx, &mut ep0_ring, slot_id, dev_i_serial).await;

    let (cfg_phys, cfg_virt) = match dma::alloc(256, 64) {
        Some(pair) => pair,
        None => {
            disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
            return;
        }
    };
    unsafe { write_bytes(cfg_virt, 0, 256) };

    let mut cfg_total_len: u16 = 0;
    if let Ok((_cc, hdr_xfer)) = control::control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        control::setup_get_descriptor(2, 0, 9),
        cfg_phys,
        9,
        "get-cfg-hdr",
        800,
    )
    .await
    {
        let hdr_len = (hdr_xfer as usize).min(9);
        let hdr = unsafe { core::slice::from_raw_parts(cfg_virt, hdr_len) };
        if hdr_len >= 4 {
            cfg_total_len = u16::from_le_bytes([hdr[2], hdr[3]]);
        }

        let req_len = cfg_total_len.min(256) as u16;
        if req_len > 9 {
            let _ = control::control_in(
                &ctx,
                &mut ep0_ring,
                slot_id,
                control::setup_get_descriptor(2, 0, req_len),
                cfg_phys,
                req_len,
                "get-cfg-full",
                800,
            )
            .await;
        }
    }

    usbv!(
        "usb: enum port {} cfgdesc len={} slot={}\n",
        target_port,
        cfg_total_len,
        slot_id
    );

    let cfg_slice_len = cfg_total_len.min(256) as usize;
    let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_slice_len) };

    let speed_str = speed_code_to_str(speed_code);
    let has_uac_out = uac::has_as_out_endpoint(cfg_slice);
    crate::log!(
        "usb: enum port {} device vid=0x{:04X} pid=0x{:04X} slot={} speed={} cfg_len={} uac_out={}\n",
        target_port,
        dev_vid,
        dev_pid,
        slot_id,
        speed_str,
        cfg_slice_len,
        has_uac_out
    );

    if let Some((hub_slot_id, hub_port)) = tree_parent {
        hub::record_hub_child(
            &state.ctx,
            hub_slot_id,
            hub_port,
            slot_id,
            dev_vid,
            dev_pid,
            dev_cls,
            dev_sub,
            dev_prot,
        );
    } else {
        hub::record_root_device(
            &state.ctx,
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            dev_cls,
            dev_sub,
            dev_prot,
        );
    }

    let mut first_if: Option<(u8, u8, u8, u8)> = None;
    {
        let mut idx = 0usize;
        while idx + 2 <= cfg_slice.len() {
            let len = cfg_slice[idx] as usize;
            if len == 0 || idx + len > cfg_slice.len() {
                break;
            }
            let ty = cfg_slice[idx + 1];
            if ty == 4 && len >= 9 {
                let if_num = cfg_slice[idx + 2];
                let if_cls = cfg_slice[idx + 5];
                let if_sub = cfg_slice[idx + 6];
                let if_prot = cfg_slice[idx + 7];
                first_if = Some((if_num, if_cls, if_sub, if_prot));
                break;
            }
            idx += len;
        }
    }

    if attach::try_attach_device(
        state,
        &ctx,
        &mut ep0_ring,
        slot_id,
        dev_vid,
        dev_pid,
        dev_cls,
        dev_sub,
        dev_prot,
        dev_serial,
        cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
        root_port,
        route_string,
        depth,
        hub_queue,
    )
    .await
    .is_some()
    {
        return;
    }

    // Not claimed: rate-limited log + free the slot so we don't leak up to MaxSlots.
    let portsc = portsc.unwrap_or(0);
    let (ccs, ped, speed) = if portsc != 0 {
        decode_port_status(portsc)
    } else {
        (false, false, speed_code_to_str(speed_code))
    };
    let pls = (portsc >> 5) & 0xF;

    let controller_id = state.info.controller_id;
    let port_log_idx = (target_port as usize)
        .saturating_sub(1)
        .min(LOG_PORTS_MAX - 1);
    let key = ((dev_vid as u32) << 16) | (dev_pid as u32);
    let prev = NOT_CLAIMED_KEY[controller_id][port_log_idx].load(Ordering::Relaxed);
    if prev != key {
        NOT_CLAIMED_KEY[controller_id][port_log_idx].store(key, Ordering::Relaxed);
        NOT_CLAIMED_COUNT[controller_id][port_log_idx].store(0, Ordering::Relaxed);
    }
    let count = NOT_CLAIMED_COUNT[controller_id][port_log_idx].fetch_add(1, Ordering::Relaxed) + 1;
    let should_log = count == 1 || (count % 50 == 0);

    if should_log {
        if let Some((if_num, if_cls, if_sub, if_prot)) = first_if {
            crate::log!(
                "usb: device on port {} not claimed vid=0x{:04X} pid=0x{:04X} devcls={:02X}/{:02X}/{:02X} mps0={} cfgs={} if{}={:02X}/{:02X}/{:02X} portsc=0x{:08X} ccs={} ped={} speed={} pls=0x{:X} (attempt {})\n",
                target_port,
                dev_vid,
                dev_pid,
                dev_cls,
                dev_sub,
                dev_prot,
                dev_mps0,
                dev_num_cfg,
                if_num,
                if_cls,
                if_sub,
                if_prot,
                portsc,
                ccs,
                ped,
                speed,
                pls,
                count
            );
        } else {
            crate::log!(
                "usb: device on port {} not claimed vid=0x{:04X} pid=0x{:04X} devcls={:02X}/{:02X}/{:02X} mps0={} cfgs={} if=none portsc=0x{:08X} ccs={} ped={} speed={} pls=0x{:X} (attempt {})\n",
                target_port,
                dev_vid,
                dev_pid,
                dev_cls,
                dev_sub,
                dev_prot,
                dev_mps0,
                dev_num_cfg,
                portsc,
                ccs,
                ped,
                speed,
                pls,
                count
            );
        }
    }

    disable_slot_and_free(state, slot_id, dev_ctx_virt, input_ctx_virt, ep0_virt_raw, ep0_bytes).await;
    usbv!(
        "usb: enum port {} disable-slot (not claimed) slot={}\n",
        target_port,
        slot_id
    );
}

pub(crate) async fn enable_slot(state: &mut UsbControllerState, target_port: u8) -> Option<u32> {
    let ctx = state.ctx;
    let enable_evt = match submit_cmd_and_wait(
        &ctx,
        &mut state.cmd_ring,
        Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(9),
        },
        None,
        "enable-slot",
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    {
        Ok(evt) => evt,
        Err(()) => {
            usbv!("usb: enum port {} enable-slot failed\n", target_port);
            return None;
        }
    };

    if control::trb_cc(&enable_evt) != 1 {
        usbv!("usb: enable-slot failed cc={}\n", control::trb_cc(&enable_evt));
        return None;
    }

    let slot_id = (enable_evt.d3 >> 24) & 0xFF;
    if slot_id == 0 {
        return None;
    }

    usbv!(
        "usb: enum port {} enable-slot-ok slot={}\n",
        target_port,
        slot_id
    );

    Some(slot_id)
}

pub(crate) async fn disable_slot(state: &mut UsbControllerState, slot_id: u32) -> Result<(), ()> {
    if slot_id == 0 {
        return Err(());
    }

    let disable = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(10) | (slot_id << 24),
    };
    xhci::submit_cmd_and_wait(
        &state.ctx,
        &mut state.cmd_ring,
        disable,
        Some(slot_id),
        "disable-slot",
        400,
        EmbassyDuration::from_millis(5),
    )
    .await?;

    unsafe {
        let dcbaa = state.dcbaa_virt as *mut u64;
        let idx = slot_id as usize;
        let max_slots = core::cmp::max(1, (state.ctx.hcsparams1 & 0xFF) as usize + 1);
        if idx < max_slots {
            write_volatile(dcbaa.add(idx), 0);
        }
    }
    Ok(())
}

async fn disable_slot_and_free(
    state: &mut UsbControllerState,
    slot_id: u32,
    dev_ctx_virt: *mut u8,
    input_ctx_virt: *mut u8,
    ep0_virt_raw: *mut u8,
    ep0_bytes: usize,
) {
    dma::dealloc(ep0_virt_raw, ep0_bytes);
    dma::dealloc(input_ctx_virt, 4096);
    dma::dealloc(dev_ctx_virt, 4096);
    let _ = disable_slot(state, slot_id).await;
}