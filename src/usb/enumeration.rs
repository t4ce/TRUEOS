use super::xhci;
use super::xhci::{
    EP_STATE_DISABLED, EP_TYPE_CONTROL, Trb, TrbRing, XhciContext, decode_port_status,
    ep_avg_trb_len_bits, ep_cerr_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo,
    trb_type,
};
use super::{
    NOT_CLAIMED_COUNT, NOT_CLAIMED_KEY, UsbControllerState, attach, control, hid,
    hid_descripto as usbdesc, mass, uac,
};
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use heapless::String;
macro_rules! usbv {
    ($($tt:tt)*) => {{
        if super::USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}
use embassy_time::{Duration as EmbassyDuration, Timer};

static LOG_ROOT_SLOT_CTX_ONCE: AtomicBool = AtomicBool::new(false);
static LOG_ROOT_ADDRDEV_FAIL_ONCE: AtomicBool = AtomicBool::new(false);
const USB_EVENT_POLL_DELAY_MS: u64 = 1;
const CMD_TIMEOUT_SHORT_ITERS: usize = 400;
const CMD_TIMEOUT_DEFAULT_ITERS: usize = 800;
const CMD_TIMEOUT_ADDRESS_ITERS: usize = 2000;
const CTRL_TIMEOUT_DEFAULT_ITERS: usize = 800;

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

    let eval_evt = xhci::submit_cmd_and_wait_any_cc(
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
        CMD_TIMEOUT_DEFAULT_ITERS,
        EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
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

async fn fetch_devdesc_header_mps0(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    desc_phys: u64,
    desc_virt: *mut u8,
    fallback_mps0: u16,
) -> u8 {
    if let Ok((_, hdr_xfer)) = control::control_in(
        ctx,
        ep0_ring,
        slot_id,
        control::setup_get_descriptor(1, 0, 8),
        desc_phys,
        8,
        "get-devdesc-8",
        CTRL_TIMEOUT_DEFAULT_ITERS,
    )
    .await
    {
        let hdr_len = (hdr_xfer as usize).min(8);
        let hdr = unsafe { core::slice::from_raw_parts(desc_virt, hdr_len) };
        if hdr_len >= 8 {
            return hdr[7];
        }
    }
    fallback_mps0 as u8
}

async fn fetch_full_device_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    desc_phys: u64,
) -> Result<(), ()> {
    control::control_in(
        ctx,
        ep0_ring,
        slot_id,
        control::setup_get_descriptor(1, 0, 18),
        desc_phys,
        18,
        "get-devdesc",
        CTRL_TIMEOUT_DEFAULT_ITERS,
    )
    .await
    .map(|_| ())
}

async fn fetch_cfg_total_len(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    cfg_phys: u64,
    cfg_virt: *mut u8,
) -> u16 {
    let mut cfg_total_len: u16 = 0;
    if let Ok((_, hdr_xfer)) = control::control_in(
        ctx,
        ep0_ring,
        slot_id,
        control::setup_get_descriptor(2, 0, 9),
        cfg_phys,
        9,
        "get-cfg-hdr",
        CTRL_TIMEOUT_DEFAULT_ITERS,
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
                ctx,
                ep0_ring,
                slot_id,
                control::setup_get_descriptor(2, 0, req_len),
                cfg_phys,
                req_len,
                "get-cfg-full",
                CTRL_TIMEOUT_DEFAULT_ITERS,
            )
            .await;
        }
    }
    cfg_total_len
}

async fn fetch_device_strings_pair(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    i_mfr: u8,
    i_prod: u8,
) -> (String<64>, String<64>) {
    let mfr = control::fetch_string_ascii::<64>(ctx, ep0_ring, slot_id, i_mfr).await;
    let prod = control::fetch_string_ascii::<64>(ctx, ep0_ring, slot_id, i_prod).await;
    (mfr, prod)
}

pub(crate) async fn enumerate_port(state: &mut UsbControllerState, target_port: u8) {
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
            crate::log!(
                "usb: enum port {} reset timeout status=0x{:08X}\n",
                target_port,
                port_status
            );
            return;
        }
        Timer::after(EmbassyDuration::from_millis(2)).await;
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
        crate::log!("usb: enum port {} enable-slot failed\n", target_port);
        return;
    };

    // From here on: always disable the slot on failure.
    let speed_code = (port_status >> 10) & 0xF;

    enumerate_with_params(state, target_port, slot_id, speed_code, Some(port_status)).await;
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

fn slot_ctx_dw0(speed_code: u32) -> u32 {
    // xHCI Slot Context DW0
    // - Route String: topology routing (20-bit route string). Root-port devices use 0.
    // - Speed: 4-bit speed ID
    // - Context Entries: 1 means Slot+EP0 are valid in the input context.
    let route_bits = 0u32;
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

pub(crate) async fn enumerate_with_params(
    state: &mut UsbControllerState,
    target_port: u8,
    slot_id: u32,
    speed_code: u32,
    portsc: Option<u32>,
) {
    let root_port: u8 = target_port;
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

    const EP0_TRBS: usize = 64;
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

    let (slot_dw0, slot_dw1, slot_dw2, ep0_dw0, ep0_dw1) = unsafe {
        let add_flags_ptr = input_ctx_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), 0x3);

        let slot_ctx = input_ctx_virt.add(ctx_stride_bytes) as *mut u32;
        let ep0_ctx = input_ctx_virt.add(ctx_stride_bytes * 2) as *mut u32;

        let dw0 = slot_ctx_dw0(speed_code);
        let dw1 = slot_ctx_dw1(root_port, target_port);

        let dw2 = 0u32;

        write_volatile(slot_ctx.add(0), dw0);
        write_volatile(slot_ctx.add(1), dw1);
        write_volatile(slot_ctx.add(2), dw2);

        let slot_dw0 = read_volatile(slot_ctx.add(0));
        let slot_dw1 = read_volatile(slot_ctx.add(1));
        let slot_dw2 = read_volatile(slot_ctx.add(2));

        write_volatile(ep0_ctx.add(0), ep_state_bits(EP_STATE_DISABLED));
        let mut ep_cfg = ep_cerr_bits(3);
        ep_cfg |= ep_type_bits(EP_TYPE_CONTROL);
        ep_cfg |= ep_max_packet_bits(max_packet as u32);
        write_volatile(ep0_ctx.add(1), ep_cfg);
        let dq = ep0_ring.dequeue_ptr();
        write_volatile(ep0_ctx.add(2), lo(dq));
        write_volatile(ep0_ctx.add(3), hi(dq));
        // For EP0/control, keep average TRB length conservative at 8 (setup packet size).
        // Some controllers reject larger values here during Address Device with CC=5.
        write_volatile(ep0_ctx.add(4), ep_avg_trb_len_bits(8));

        let ep0_dw0 = read_volatile(ep0_ctx.add(0));
        let ep0_dw1 = read_volatile(ep0_ctx.add(1));
        (slot_dw0, slot_dw1, slot_dw2, ep0_dw0, ep0_dw1)
    };

    let addr_evt = match xhci::submit_cmd_and_wait_any_cc(
        &ctx,
        &mut state.cmd_ring,
        Trb {
            d0: lo(input_ctx_phys),
            d1: hi(input_ctx_phys),
            d2: 0,
            d3: trb_type(11) | (slot_id << 24),
        },
        Some(slot_id),
        "address-device",
        CMD_TIMEOUT_ADDRESS_ITERS,
        EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
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
            crate::log!(
                "usb: enum port {} address-device timeout slot={}\n",
                target_port,
                slot_id
            );
            disable_slot_and_free(
                state,
                slot_id,
                dev_ctx_virt,
                input_ctx_virt,
                ep0_virt_raw,
                ep0_bytes,
            )
            .await;
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
        crate::log!(
            "usb: enum port {} address-device failed cc={} slot={}\n",
            target_port,
            control::trb_cc(&addr_evt),
            slot_id
        );
        if !LOG_ROOT_ADDRDEV_FAIL_ONCE.swap(true, Ordering::AcqRel) {
            crate::log!(
                "usb: root addrdev fail evt=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}] slot_dw0=0x{:08X} slot_dw1=0x{:08X} slot_dw2=0x{:08X} ep0_dw0=0x{:08X} ep0_dw1=0x{:08X}\n",
                addr_evt.d0,
                addr_evt.d1,
                addr_evt.d2,
                addr_evt.d3,
                slot_dw0,
                slot_dw1,
                slot_dw2,
                ep0_dw0,
                ep0_dw1
            );
        }
        disable_slot_and_free(
            state,
            slot_id,
            dev_ctx_virt,
            input_ctx_virt,
            ep0_virt_raw,
            ep0_bytes,
        )
        .await;
        return;
    }

    if !LOG_ROOT_SLOT_CTX_ONCE.swap(true, Ordering::AcqRel) {
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
            crate::log!(
                "usb: enum port {} alloc devdesc buffer failed slot={}\n",
                target_port,
                slot_id
            );
            disable_slot_and_free(
                state,
                slot_id,
                dev_ctx_virt,
                input_ctx_virt,
                ep0_virt_raw,
                ep0_bytes,
            )
            .await;
            return;
        }
    };
    unsafe { write_bytes(desc_virt, 0, 64) };

    // First grab the 8-byte header to learn bMaxPacketSize0, then, if needed,
    // reprogram EP0 MPS via Evaluate Context before pulling the full descriptor.
    let dev_mps0_hdr =
        fetch_devdesc_header_mps0(&ctx, &mut ep0_ring, slot_id, desc_phys, desc_virt, ep0_mps)
            .await;

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
            disable_slot_and_free(
                state,
                slot_id,
                dev_ctx_virt,
                input_ctx_virt,
                ep0_virt_raw,
                ep0_bytes,
            )
            .await;
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

    if fetch_full_device_descriptor(&ctx, &mut ep0_ring, slot_id, desc_phys)
        .await
        .is_err()
    {
        usbv!(
            "usb: enum port {} get-devdesc failed slot={}\n",
            target_port,
            slot_id
        );
        crate::log!(
            "usb: enum port {} get-devdesc failed slot={}\n",
            target_port,
            slot_id
        );
        disable_slot_and_free(
            state,
            slot_id,
            dev_ctx_virt,
            input_ctx_virt,
            ep0_virt_raw,
            ep0_bytes,
        )
        .await;
        return;
    }

    usbv!(
        "usb: enum port {} devdesc-ok slot={}\n",
        target_port,
        slot_id
    );

    let (
        dev_vid,
        dev_pid,
        dev_cls,
        dev_sub,
        dev_prot,
        dev_mps0,
        dev_i_mfr,
        dev_i_prod,
        _dev_i_serial,
        dev_num_cfg,
    ) = unsafe {
        let dd = core::slice::from_raw_parts(desc_virt, 18);
        let vid = u16::from_le_bytes([dd[8], dd[9]]);
        let pid = u16::from_le_bytes([dd[10], dd[11]]);
        (
            vid, pid, dd[4], dd[5], dd[6], dd[7], dd[14], dd[15], dd[16], dd[17],
        )
    };

    let (cfg_phys, cfg_virt) = match dma::alloc(256, 64) {
        Some(pair) => pair,
        None => {
            disable_slot_and_free(
                state,
                slot_id,
                dev_ctx_virt,
                input_ctx_virt,
                ep0_virt_raw,
                ep0_bytes,
            )
            .await;
            return;
        }
    };
    unsafe { write_bytes(cfg_virt, 0, 256) };

    let cfg_total_len = fetch_cfg_total_len(&ctx, &mut ep0_ring, slot_id, cfg_phys, cfg_virt).await;

    usbv!(
        "usb: enum port {} cfgdesc len={} slot={}\n",
        target_port,
        cfg_total_len,
        slot_id
    );

    let cfg_slice_len = cfg_total_len.min(256) as usize;
    let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_slice_len) };

    usbdesc::log_all_descriptor_types(cfg_slice, target_port, slot_id);

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

    let controller_id = state.info.controller_id;

    // xHCI PORTSC doesn't include VID:PID; cache it for xHCI-side debug once we've read it.
    xhci::set_port_vidpid(ctx.controller_id, target_port, dev_vid, dev_pid);
    crate::log!(
        "xhci: port {} id={:04X}:{:04X}\n",
        target_port,
        dev_vid,
        dev_pid
    );

    super::record_slot_identity(
        controller_id,
        slot_id,
        dev_vid,
        dev_pid,
        dev_cls,
        dev_sub,
        dev_prot,
    );

    let mut first_if: Option<(u8, u8, u8, u8)> = None;
    let mut first_if_hid_report_len: Option<u16> = None;
    {
        let mut idx = 0usize;
        let mut current_alt: u8 = 0;
        let mut current_iface_is_first: bool = false;
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
                current_alt = cfg_slice[idx + 3];

                if first_if.is_none() {
                    first_if = Some((if_num, if_cls, if_sub, if_prot));
                    current_iface_is_first = true;
                } else {
                    current_iface_is_first = false;
                }
            } else if ty == 0x21 && len >= 9 {
                // HID descriptor: report descriptor length is at bytes 7..8.
                if current_alt == 0 && current_iface_is_first {
                    first_if_hid_report_len =
                        Some(u16::from_le_bytes([cfg_slice[idx + 7], cfg_slice[idx + 8]]));
                }
            }
            idx += len;
        }
    }

    if let Some(pair) = mass::parse_mass_interface(cfg_slice) {
        crate::log!(
            "usb: enum port {} mass-candidate (SCSI/BOT) iface={} cfg={} ep_in=0x{:02X} ep_out=0x{:02X} mps_in={} mps_out={}\n",
            target_port,
            pair.interface,
            pair.configuration,
            pair.ep_in,
            pair.ep_out,
            pair.max_packet_in,
            pair.max_packet_out,
        );
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
        cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
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

    let port_log_idx = (target_port as usize)
        .saturating_sub(1)
        .min(super::LOG_PORTS_MAX - 1);
    let key = ((dev_vid as u32) << 16) | (dev_pid as u32);
    let prev = NOT_CLAIMED_KEY[controller_id][port_log_idx].load(Ordering::Relaxed);
    if prev != key {
        NOT_CLAIMED_KEY[controller_id][port_log_idx].store(key, Ordering::Relaxed);
        NOT_CLAIMED_COUNT[controller_id][port_log_idx].store(0, Ordering::Relaxed);
    }
    let count = NOT_CLAIMED_COUNT[controller_id][port_log_idx].fetch_add(1, Ordering::Relaxed) + 1;
    let should_log = count == 1 || (count % 50 == 0);

    if should_log {
        if dev_i_mfr != 0 || dev_i_prod != 0 {
            let (dev_mfr, dev_prod) =
                fetch_device_strings_pair(&ctx, &mut ep0_ring, slot_id, dev_i_mfr, dev_i_prod)
                    .await;
            if !(dev_mfr.is_empty() && dev_prod.is_empty()) {
                crate::log!(
                    "usb: device strings on port {} mfr='{}' prod='{}'\n",
                    target_port,
                    dev_mfr.as_str(),
                    dev_prod.as_str()
                );
            }
        }

        if let Some((if_num, if_cls, if_sub, if_prot)) = first_if {
            crate::log!(
                "usb: device on port {} not claimed vid=0x{:04X} pid=0x{:04X} devcls={:02X}/{:02X}/{:02X} mps0={} cfgs={} if{}={:02X}/{:02X}/{:02X} hid_rep_len={:?} portsc=0x{:08X} ccs={} ped={} speed={} pls=0x{:X} (attempt {})\n",
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
                first_if_hid_report_len,
                portsc,
                ccs,
                ped,
                speed,
                pls,
                count
            );

            if count == 1
                && (super::USB_LOG_VERBOSE || (if_cls == 0x03 && first_if_hid_report_len.is_some()))
            {
                if if_cls == 0x03 {
                    if let Some(rep_len) = first_if_hid_report_len {
                        let rep_len = rep_len as usize;
                        match hid::fetch_report_descriptor(
                            &ctx,
                            &mut ep0_ring,
                            slot_id,
                            if_num,
                            rep_len,
                        )
                        .await
                        {
                            Ok(desc) => {
                                crate::log!(
                                    "usb: hid report-desc slot={} port={} iface={} len={} ok bytes={}\n",
                                    slot_id,
                                    target_port,
                                    if_num,
                                    rep_len,
                                    desc.len()
                                );
                                let show = core::cmp::min(desc.len(), 128);
                                let mut i = 0usize;
                                while i < show {
                                    let end = core::cmp::min(i + 16, show);
                                    crate::log!("usb:  rep{:03X}: {}\n", i, hex16(&desc[i..end]));
                                    i = end;
                                }
                            }
                            Err(err) => {
                                crate::log!(
                                    "usb: hid report-desc slot={} port={} iface={} len={} err={:?}\n",
                                    slot_id,
                                    target_port,
                                    if_num,
                                    rep_len,
                                    err
                                );
                            }
                        }
                    }
                }
            }
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

    // Keep the slot assigned for unclaimed devices.
    // This avoids periodic rescans burning through slots and spamming logs.
    let registry_port = if root_port != 0 {
        root_port
    } else {
        target_port
    };
    super::register_unclaimed_device(
        state.info.controller_id,
        slot_id,
        registry_port,
        super::DeviceResources {
            dev_ctx_virt: dev_ctx_virt as usize,
            input_ctx_virt: input_ctx_virt as usize,
            ep0_virt_raw: ep0_virt_raw as usize,
            ep0_bytes,
        },
    );
    usbv!(
        "usb: enum port {} keep-slot (unclaimed) slot={}\n",
        target_port,
        slot_id
    );
    return;
}

fn hex16(bytes: &[u8]) -> heapless::String<64> {
    let mut s: heapless::String<64> = heapless::String::new();
    let mut first = true;
    for &b in bytes.iter().take(16) {
        if !first {
            let _ = s.push(' ');
        }
        first = false;
        let _ = core::fmt::Write::write_fmt(&mut s, format_args!("{:02X}", b));
    }
    s
}

pub(crate) async fn enable_slot(state: &mut UsbControllerState, target_port: u8) -> Option<u32> {
    let ctx = state.ctx;
    let enable_evt = match xhci::submit_cmd_and_wait_any_cc(
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
        CMD_TIMEOUT_SHORT_ITERS,
        EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
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
        usbv!(
            "usb: enable-slot failed cc={}\n",
            control::trb_cc(&enable_evt)
        );
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
    let evt = xhci::submit_cmd_and_wait_any_cc(
        &state.ctx,
        &mut state.cmd_ring,
        disable,
        // Some controllers report disable-slot completion with slot_id=0.
        // Do not over-filter; match by command pointer only.
        None,
        "disable-slot",
        CMD_TIMEOUT_SHORT_ITERS,
        EmbassyDuration::from_millis(USB_EVENT_POLL_DELAY_MS),
    )
    .await?;

    let cc = control::trb_cc(&evt);
    if cc != 1 {
        crate::log!(
            "usb: disable-slot failed cc={} req_slot={} evt_slot={} evt=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
            cc,
            slot_id,
            (evt.d3 >> 24) & 0xFF,
            evt.d0,
            evt.d1,
            evt.d2,
            evt.d3
        );
        return Err(());
    }

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
