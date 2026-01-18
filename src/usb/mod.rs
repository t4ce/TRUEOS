pub mod cdc;
pub mod cdc_acm;
pub mod hid;
pub mod hub;
pub mod input;
pub mod mass;
pub mod pen;
pub mod print;
pub mod isoch;
pub mod uac;
pub mod truekey;
pub mod xhci;
mod scout;

pub use scout::usb_scout;

use self::xhci::{
    decode_port_status, ep_avg_trb_len_bits, ep_cerr_bits, ep_max_packet_bits, ep_state_bits,
    ep_type_bits, hi, lo, trb_type, Trb, TrbRing, XhciContext, EP_STATE_DISABLED, EP_TYPE_CONTROL,
};
use crate::pci::dma;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;
use self::hub::{HubWork, LOG_PORTS_MAX, MAX_DEVICES};

use self::hid::BootAttachParams;
use self::mass::AttachParams as MassAttachParams;
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DeviceKind {
    Hid,
    Hub,
    Mass,
    Printer,
    Pen,
    Cdc,
    Uac,
}
#[derive(Copy, Clone, Debug)]
struct DeviceEntry {
    slot_id: u32,
    port: u8,
    kind: DeviceKind,
}

static DEVICES: Mutex<Vec<DeviceEntry, MAX_DEVICES>> = Mutex::new(Vec::new());
static ENUM_READY: AtomicBool = AtomicBool::new(false);
static NOT_CLAIMED_KEY: [AtomicU32; LOG_PORTS_MAX] = [const { AtomicU32::new(0) }; LOG_PORTS_MAX];
static NOT_CLAIMED_COUNT: [AtomicU32; LOG_PORTS_MAX] = [const { AtomicU32::new(0) }; LOG_PORTS_MAX];

struct UsbControllerState {
    info: xhci::XhcInfo,
    ctx: XhciContext,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    dcbaa_phys: u64,
    dcbaa_virt: *mut u8,
    scratchpad_array_phys: u64,
    scratchpad_array_virt: *mut u8,
    scratchpad_count: u32,
    cmd_ring: TrbRing,
    _cmd_phys: u64,
    _cmd_virt: *mut u8,
    _evt_phys: u64,
    _evt_virt: *mut u8,
    _erst_phys: u64,
    _erst_virt: *mut u8,
}

unsafe impl Send for UsbControllerState {}
unsafe impl Sync for UsbControllerState {}

const USB_LOG_VERBOSE: bool = false;

macro_rules! usbv {
    ($($tt:tt)*) => {{
        if USB_LOG_VERBOSE {
            crate::log!($($tt)*);
        }
    }};
}

fn trb_cc(evt: &Trb) -> u32 {
    (evt.d2 >> 24) & 0xFF
}


async fn submit_cmd_and_wait(
    ctx: &XhciContext,
    cmd_ring: &mut TrbRing,
    cmd: Trb,
    slot_filter: Option<u32>,
    what: &'static str,
    timeout_iters: usize,
    delay: EmbassyDuration,
) -> Result<Trb, ()> {
    if !cmd_ring.push(cmd) {
        usbv!("usb: {}: cmd ring full\n", what);
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    let evt = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 33 {
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

fn setup_get_descriptor(desc_type: u8, desc_index: u8, length: u16) -> Trb {
    // bmRequestType=0x80 (IN|Standard|Device), bRequest=0x06 (GET_DESCRIPTOR)
    let w_value = ((desc_type as u16) << 8) | (desc_index as u16);
    Trb {
        d0: (0x80u32) | (0x06u32 << 8) | ((w_value as u32) << 16),
        d1: (length as u32) << 16,  // wIndex=0
        d2: 8 | (2 << 16),          // 8-byte setup, TRT=IN
        d3: trb_type(2) | (1 << 6), // Setup Stage, IDT
    }
}

fn setup_get_string_descriptor(desc_index: u8, langid: u16, length: u16) -> Trb {
    // bmRequestType=0x80 (IN|Standard|Device), bRequest=0x06 (GET_DESCRIPTOR)
    // wValue = (STRING << 8) | index, wIndex = langid
    let w_value = ((3u16) << 8) | (desc_index as u16);
    Trb {
        d0: (0x80u32) | (0x06u32 << 8) | ((w_value as u32) << 16),
        d1: (langid as u32) | ((length as u32) << 16),
        d2: 8 | (2 << 16),          // 8-byte setup, TRT=IN
        d3: trb_type(2) | (1 << 6), // Setup Stage, IDT
    }
}

async fn fetch_first_langid(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
) -> Option<u16> {
    // String descriptor 0 returns supported LANGIDs.
    let (buf_phys, buf_virt) = dma::alloc(256, 64)?;
    unsafe { write_bytes(buf_virt, 0, 256) };
    let res = control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_string_descriptor(0, 0, 255),
        buf_phys,
        255,
        "get-str-langid",
        400,
    )
    .await;

    let lang = match res {
        Ok((_cc, transferred)) => unsafe {
            let n = (transferred as usize).min(256);
            let desc = core::slice::from_raw_parts(buf_virt, n);
            if desc.len() < 4 || desc[1] != 3 {
                None
            } else {
                Some(u16::from_le_bytes([desc[2], desc[3]]))
            }
        },
        Err(()) => None,
    };

    dma::dealloc(buf_virt, 256);
    lang
}

async fn fetch_serial_string(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    serial_index: u8,
) -> cdc_acm::UsbSerial {
    if serial_index == 0 {
        return cdc_acm::UsbSerial::none();
    }

    let langid = fetch_first_langid(ctx, ep0_ring, slot_id)
        .await
        .unwrap_or(0x0409);

    let (buf_phys, buf_virt) = match dma::alloc(256, 64) {
        Some(p) => p,
        None => return cdc_acm::UsbSerial::none(),
    };
    unsafe { write_bytes(buf_virt, 0, 256) };

    let res = control_in(
        ctx,
        ep0_ring,
        slot_id,
        setup_get_string_descriptor(serial_index, langid, 255),
        buf_phys,
        255,
        "get-serial",
        800,
    )
    .await;

    let mut out: heapless::Vec<u8, 64> = heapless::Vec::new();
    if let Ok((_cc, transferred)) = res {
        unsafe {
            let n = (transferred as usize).min(256);
            let desc = core::slice::from_raw_parts(buf_virt, n);
            if desc.len() >= 2 && desc[1] == 3 {
                let total = (desc[0] as usize).min(desc.len());
                let mut idx = 2usize;
                while idx + 1 < total {
                    let ch = u16::from_le_bytes([desc[idx], desc[idx + 1]]);
                    let b = if ch <= 0x7F { ch as u8 } else { b'?' };
                    let _ = out.push(b);
                    idx += 2;
                }
            }
        }
    }

    dma::dealloc(buf_virt, 256);
    cdc_acm::UsbSerial::from_bytes(out.as_slice())
}

async fn control_in(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: u64,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<(u32, u16), ()> {
    let data = Trb {
        d0: lo(buf_phys),
        d1: hi(buf_phys),
        d2: length as u32,
        d3: trb_type(3) | (1 << 16), // Data Stage, DIR=IN, no IOC
    };

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5), // Status Stage, IOC, DIR=OUT
    };

    if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
        usbv!("usb: {}: ep0 ring full\n", what);
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let evt = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
    )
    .await
    .ok_or(())
    .map_err(|_| {
        usbv!("usb: {}: timeout waiting for transfer event\n", what);
    })?;

    let completion = trb_cc(&evt);
    let remaining = (evt.d2 & 0x00FF_FFFF) as u32;
    let requested = length as u32;
    let transferred = requested.saturating_sub(remaining).min(requested) as u16;

    // CC=13 is a normal short packet completion on control-IN reads.
    if completion == 1 || completion == 13 {
        Ok((completion, transferred))
    } else {
        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        Err(())
    }
}

pub(super) async fn control_out(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: Option<u64>,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<(), ()> {
    if !ep0_ring.push(setup) {
        usbv!("usb: {}: ep0 ring full (setup)\n", what);
        return Err(());
    }

    if let Some(phys) = buf_phys {
        let data = Trb {
            d0: lo(phys),
            d1: hi(phys),
            d2: length as u32,
            d3: trb_type(3),
        };
        if !ep0_ring.push(data) {
            usbv!("usb: {}: ep0 ring full (data)\n", what);
            return Err(());
        }
    }

    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(status) {
        usbv!("usb: {}: ep0 ring full (status)\n", what);
        return Err(());
    }

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let evt = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
        },
        timeout_iters,
        EmbassyDuration::from_millis(5),
    )
    .await
    .ok_or(())
    .map_err(|_| {
        usbv!("usb: {}: timeout waiting for transfer event\n", what);
    })?;

    let completion = trb_cc(&evt);
    if completion == 1 {
        Ok(())
    } else {
        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        Err(())
    }
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

    if trb_cc(&eval_evt) != 1 {
        usbv!(
            "usb: eval-ctx-ep0 cc={} slot={}\n",
            trb_cc(&eval_evt),
            slot_id
        );
        return Err(());
    }

    Ok(())
}

async fn enumerate_port(
    state: &mut UsbControllerState,
    target_port: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
    let ctx = state.ctx;
    let dcbaa_virt = state.dcbaa_virt;
    let ctx_stride_bytes = state.ctx_stride_bytes;
    let ctx_stride_words = state.ctx_stride_words;

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

async fn enumerate_with_params(
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
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
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
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(input_ctx_virt, 0, 4096) };

    const EP0_TRBS: usize = 32;
    let (ep0_phys, ep0_virt_raw) = match dma::alloc(EP0_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            let _ = disable_slot(state, slot_id).await;
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

        let route_speed_ctx_entries = (route_string & 0xFFFFF) | (speed_code << 20) | (1 << 27);
        write_volatile(slot_ctx.add(0), route_speed_ctx_entries);
        let root_port = (root_port as u32) << 16;
        write_volatile(slot_ctx.add(1), root_port);
        if let Some((tt_hub_slot, tt_port)) = tt_info {
            let tt = (tt_hub_slot & 0xFF) | ((tt_port as u32) << 8);
            write_volatile(slot_ctx.add(2), tt);
        }

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
    }

    let addr_evt = match submit_cmd_and_wait(
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
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };

    if trb_cc(&addr_evt) != 1 {
        usbv!(
            "usb: enum port {} address-device cc={} slot={}\n",
            target_port,
            trb_cc(&addr_evt),
            slot_id
        );
        let _ = disable_slot(state, slot_id).await;
        return;
    }

    usbv!(
        "usb: enum port {} address-ok slot={}\n",
        target_port,
        slot_id
    );

    let (desc_phys, desc_virt) = match dma::alloc(64, 64) {
        Some(pair) => pair,
        None => {
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(desc_virt, 0, 64) };

    // First grab the 8-byte header to learn bMaxPacketSize0, then, if needed,
    // reprogram EP0 MPS via Evaluate Context before pulling the full descriptor.
    let mut dev_mps0_hdr: u8 = ep0_mps as u8;
    if let Ok((_cc, hdr_xfer)) = control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup_get_descriptor(1, 0, 8),
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
            let _ = disable_slot(state, slot_id).await;
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

    if control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup_get_descriptor(1, 0, 18),
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
        let _ = disable_slot(state, slot_id).await;
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

    let dev_serial = fetch_serial_string(&ctx, &mut ep0_ring, slot_id, dev_i_serial).await;

    let (cfg_phys, cfg_virt) = match dma::alloc(256, 64) {
        Some(pair) => pair,
        None => {
            let _ = disable_slot(state, slot_id).await;
            return;
        }
    };
    unsafe { write_bytes(cfg_virt, 0, 256) };

    let mut cfg_total_len: u16 = 0;
    if let Ok((_cc, hdr_xfer)) = control_in(
        &ctx,
        &mut ep0_ring,
        slot_id,
        setup_get_descriptor(2, 0, 9),
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
            let _ = control_in(
                &ctx,
                &mut ep0_ring,
                slot_id,
                setup_get_descriptor(2, 0, req_len),
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

    let mut tried_uac = false;
    if has_uac_out {
        tried_uac = true;
        if uac::attach_device(uac::AttachParams {
            ctx: &ctx,
            cmd_ring: &mut state.cmd_ring,
            ep0_ring: &mut ep0_ring,
            slot_id,
            cfg: cfg_slice,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            speed_code,
            target_port,
        })
        .await
        .is_ok()
        {
            usbv!(
                "usb: enum port {} claimed UAC slot={} vid=0x{:04X} pid=0x{:04X}\n",
                target_port,
                slot_id,
                dev_vid,
                dev_pid
            );
            register_device(slot_id as u32, target_port, DeviceKind::Uac);
            return;
        }
    }

    if hub::is_hub_device(dev_cls, dev_sub, dev_prot, cfg_slice) {
        if let Ok(desc) = hub::attach_device(hub::AttachParams {
            ctx: &ctx,
            ep0_ring: &mut ep0_ring,
            slot_id,
            cfg: cfg_slice,
            target_port,
        })
        .await
        {
            usbv!(
                "usb: enum port {} claimed HUB slot={} vid=0x{:04X} pid=0x{:04X}\n",
                target_port,
                slot_id,
                dev_vid,
                dev_pid
            );
            register_device(slot_id as u32, target_port, DeviceKind::Hub);
            hub::record_hub_ports(slot_id, desc.port_count);
            let _ = hub_queue.push(HubWork {
                hub_slot_id: slot_id,
                root_port,
                route_string,
                depth,
                hub_speed_code: speed_code,
                port_count: desc.port_count,
            });
            return;
        }
    }

    let hid_count = hid::attach_boot_devices(BootAttachParams {
        ctx: &ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring: &mut ep0_ring,
        slot_id,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    })
    .await
    .unwrap_or(0);

    if hid_count > 0 {
        usbv!(
            "usb: enum port {} claimed HID slot={} vid=0x{:04X} pid=0x{:04X} count={}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            hid_count
        );
        register_device(slot_id as u32, target_port, DeviceKind::Hid);
        return;
    }

    if mass::attach_mass_device(MassAttachParams {
        ctx: &ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring: &mut ep0_ring,
        slot_id,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    })
    .await
    .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed MASS slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Mass);
        return;
    }

    if pen::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PEN slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Pen);
        return;
    }
    if print::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PRINTER slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Printer);
        return;
    }

    if cdc_acm::attach_device(cdc_acm::AttachParams {
        ctx: &ctx,
        cmd_ring: &mut state.cmd_ring,
        ep0_ring: &mut ep0_ring,
        slot_id,
        dev_vid,
        dev_pid,
        dev_serial,
        cfg: cfg_slice,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
        desired_baud: 115_200,
    })
    .await
    .is_ok()
    {
        usbv!(
            "usb: enum port {} claimed CDC-ACM slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Cdc);
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
        .min(LOG_PORTS_MAX - 1);
    let key = ((dev_vid as u32) << 16) | (dev_pid as u32);
    let prev = NOT_CLAIMED_KEY[port_log_idx].load(Ordering::Relaxed);
    if prev != key {
        NOT_CLAIMED_KEY[port_log_idx].store(key, Ordering::Relaxed);
        NOT_CLAIMED_COUNT[port_log_idx].store(0, Ordering::Relaxed);
    }
    let count = NOT_CLAIMED_COUNT[port_log_idx].fetch_add(1, Ordering::Relaxed) + 1;
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

    let _ = disable_slot(state, slot_id).await;
    usbv!(
        "usb: enum port {} disable-slot (not claimed) slot={}\n",
        target_port,
        slot_id
    );
}

async fn enable_slot(state: &mut UsbControllerState, target_port: u8) -> Option<u32> {
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

    if trb_cc(&enable_evt) != 1 {
        usbv!("usb: enable-slot failed cc={}\n", trb_cc(&enable_evt));
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

async fn disable_slot(state: &mut UsbControllerState, slot_id: u32) -> Result<(), ()> {
    if slot_id == 0 {
        return Err(());
    }

    let disable = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(10) | (slot_id << 24),
    };
    if !state.cmd_ring.push(disable) {
        return Err(());
    }
    unsafe {
        write_volatile(state.ctx.doorbell.add(0), 0);
    }

    let Some(evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 33 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
        },
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        return Err(());
    };

    let completion = (evt.d2 >> 24) & 0xFF;
    if completion != 1 {
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

fn register_device(slot_id: u32, port: u8, kind: DeviceKind) {
    let mut guard = DEVICES.lock();
    if let Some(existing) = guard.iter_mut().find(|d| d.slot_id == slot_id) {
        existing.kind = kind;
        existing.port = port;
        return;
    }
    if guard
        .push(DeviceEntry {
            slot_id,
            port,
            kind,
        })
        .is_err()
    {
        crate::log!("usb: device table full, dropping slot {}\n", slot_id);
    }
    // Signal that at least one device is enumerated so poll_task can start.
    ENUM_READY.store(true, Ordering::Release);
    crate::log!(
        "usb: device claimed slot={} port={} kind={:?}\n",
        slot_id,
        port,
        kind
    );
}

fn device_kind_for_slot(slot_id: u32) -> Option<DeviceKind> {
    DEVICES
        .lock()
        .iter()
        .find(|d| d.slot_id == slot_id)
        .map(|d| d.kind)
}

fn any_hid_registered() -> bool {
    DEVICES.lock().iter().any(|d| d.kind == DeviceKind::Hid)
}

#[embassy_executor::task]
pub async fn poll_task(info: xhci::XhcInfo) {
    let ctx = unsafe { XhciContext::new(info) };
    let mut heartbeat: u32 = 0;
    let mut idle_timeouts: u32 = 0;

    loop {
        if !ENUM_READY.load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(5)).await;
            continue;
        }

        heartbeat = heartbeat.wrapping_add(1);

        let evt_opt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) as u32;
                device_kind_for_slot(evt_slot).is_some()
            },
            400,
            EmbassyDuration::from_millis(5),
        )
        .await;

        let Some(evt) = evt_opt else {
            idle_timeouts = idle_timeouts.wrapping_add(1);
            continue;
        };

        idle_timeouts = 0;

        let evt_slot = (evt.d3 >> 24) as u32;

        match device_kind_for_slot(evt_slot) {
            Some(DeviceKind::Hid) => {
                if !any_hid_registered() {
                    continue;
                }

                let ep_target = (evt.d3 >> 16) & 0x1F;
                if ep_target == 0 {
                    continue;
                }

                let handled = hid::with_runtime_mut_by_slot_and_target(evt_slot, ep_target, |runtime| {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    let residual = evt.d2 & 0x00FF_FFFF;
                    let data_len = runtime.report_len.min(runtime.ep.max_packet as u32) as usize;
                    let data = unsafe {
                        core::slice::from_raw_parts(runtime.report_virt, data_len)
                    };
                    hid::handle_report(runtime, completion, data, residual);

                    let normal = Trb {
                        d0: lo(runtime.report_phys),
                        d1: hi(runtime.report_phys),
                        d2: runtime.report_len,
                        d3: trb_type(1) | (1 << 5),
                    };

                    let before = runtime.ep_ring.state_snapshot();
                    if !runtime.ep_ring.push(normal) {
                        crate::log!("usb: failed to requeue HID interrupt IN transfer\n");
                    } else {
                        let after = runtime.ep_ring.state_snapshot();
                        if hid::HID_LOGS {
                            crate::log!(
                                "[hid] requeue slot={} target={} ring_before=({}, {}) ring_after=({}, {})\n",
                                runtime.slot_id,
                                runtime.ep_target,
                                before.0,
                                before.1 as u8,
                                after.0,
                                after.1 as u8
                            );
                        }
                        unsafe {
                            write_volatile(
                                ctx.doorbell.add(runtime.slot_id as usize),
                                runtime.ep_target
                            );
                        }
                        if hid::HID_LOGS {
                            crate::log!(
                                "[hid] doorbell slot={} target={} rung\n",
                                runtime.slot_id,
                                runtime.ep_target
                            );
                        }
                    }
                    true
                })
                .unwrap_or(false);

                if !handled {
                    usbv!(
                        "usb: ignoring transfer event slot={} (no HID runtime)\n",
                        evt_slot
                    );
                }
            }
            Some(DeviceKind::Mass) => {
                // Mass storage transfers are driven by the mass driver; nothing to do here yet.
            }
            Some(DeviceKind::Cdc) => {
                if !cdc_acm::handle_transfer_event(&evt) {
                    usbv!(
                        "usb: ignoring transfer event slot={} (no CDC runtime)\n",
                        evt_slot
                    );
                }
            }
            Some(DeviceKind::Uac) => {
                let _ = uac::handle_transfer_event(&evt);
            }
            Some(DeviceKind::Hub) => {
                // Hub class driver not implemented yet.
            }
            Some(DeviceKind::Printer) => {}
            Some(DeviceKind::Pen) => {}
            None => {
                // A device may complete transfers during attach (while the enum path is still
                // running) before `register_device()` marks the slot kind. Handle CDC events
                // opportunistically so TX/RX doesn't stall.
                let _ = cdc_acm::handle_transfer_event(&evt);
            }
        }
    }
}
