pub mod cdc;
pub mod cdc_acm;
pub mod hid;
pub mod input;
pub mod mass;
pub mod pen;
pub mod print;
pub mod isoch;
pub mod uac;
pub mod resample_44k1_to_48k;
pub mod truekey;
pub mod xhci;

use self::xhci::{
    decode_port_status, ep_avg_trb_len_bits, ep_cerr_bits, ep_max_packet_bits, ep_state_bits,
    ep_type_bits, hi, lo, trb_type, write_reg64, ErstEntry, EventRing, Trb, TrbRing,
    XhciContext, EP_STATE_DISABLED, EP_TYPE_CONTROL,
};
use crate::pci::{dma, osal};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use self::hid::BootAttachParams;
use self::mass::AttachParams as MassAttachParams;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DeviceKind {
    Hid,
    Headset,
    Mass,
    Printer,
    Pen,
    Cdc,
    Uac,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct DeviceKinds(u32);

impl DeviceKinds {
    const HID: u32 = 1 << 0;
    const HEADSET: u32 = 1 << 1;
    const MASS: u32 = 1 << 2;
    const PRINTER: u32 = 1 << 3;
    const PEN: u32 = 1 << 4;
    const CDC: u32 = 1 << 5;
    const UAC: u32 = 1 << 6;

    const fn empty() -> Self {
        Self(0)
    }

    const fn mask(kind: DeviceKind) -> u32 {
        match kind {
            DeviceKind::Hid => Self::HID,
            DeviceKind::Headset => Self::HEADSET,
            DeviceKind::Mass => Self::MASS,
            DeviceKind::Printer => Self::PRINTER,
            DeviceKind::Pen => Self::PEN,
            DeviceKind::Cdc => Self::CDC,
            DeviceKind::Uac => Self::UAC,
        }
    }

    fn insert(&mut self, kind: DeviceKind) {
        self.0 |= Self::mask(kind);
    }

    fn contains(self, kind: DeviceKind) -> bool {
        (self.0 & Self::mask(kind)) != 0
    }

    fn any_hidlike(self) -> bool {
        (self.0 & (Self::HID | Self::HEADSET)) != 0
    }

    fn is_empty(self) -> bool {
        self.0 == 0
    }
}
#[derive(Copy, Clone, Debug)]
struct DeviceEntry {
    slot_id: u32,
    port: u8,
    kinds: DeviceKinds,
}

const SCRATCHPAD_BUF_SIZE: usize = 4096;
const MAX_DEVICES: usize = 8;
static DEVICES: Mutex<Vec<DeviceEntry, MAX_DEVICES>> = Mutex::new(Vec::new());
static ENUM_READY: AtomicBool = AtomicBool::new(false);
static SCOUT_RUNNING: AtomicBool = AtomicBool::new(false);

const LOG_PORTS_MAX: usize = 32;
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

static USB_CTRL: Mutex<Option<UsbControllerState>> = Mutex::new(None);

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

pub(super) async fn control_in_cc(
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
    Ok((completion, transferred))
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
    let (completion, transferred) = control_in_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        buf_phys,
        length,
        what,
        timeout_iters,
    )
    .await?;

    // CC=13 is a normal short packet completion on control-IN reads.
    if completion == 1 || completion == 13 {
        Ok((completion, transferred))
    } else {
        usbv!("usb: {}: transfer failed cc={}\n", what, completion);
        Err(())
    }
}

pub(super) async fn control_out_cc(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    setup: Trb,
    buf_phys: Option<u64>,
    length: u16,
    what: &'static str,
    timeout_iters: usize,
) -> Result<u32, ()> {
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
    Ok(completion)
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
    let completion = control_out_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        buf_phys,
        length,
        what,
        timeout_iters,
    )
    .await?;
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

async fn enumerate_port(state: &mut UsbControllerState, target_port: u8) {
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
            return;
        }
    };

    if trb_cc(&enable_evt) != 1 {
        usbv!("usb: enable-slot failed cc={}\n", trb_cc(&enable_evt));
        return;
    }

    let slot_id = (enable_evt.d3 >> 24) & 0xFF;
    if slot_id == 0 {
        return;
    }

    usbv!(
        "usb: enum port {} enable-slot-ok slot={}\n",
        target_port,
        slot_id
    );

    // From here on: always disable the slot on failure.

    let speed_code = (port_status >> 10) & 0xF;
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

        let route_speed_ctx_entries = (speed_code << 20) | (1 << 27);
        write_volatile(slot_ctx.add(0), route_speed_ctx_entries);
        let root_port = (target_port as u32) << 16;
        write_volatile(slot_ctx.add(1), root_port);

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

    let (_ccs, _ped, speed_str) = decode_port_status(port_status);
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
    let mut claimed_any = false;
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
            claimed_any = true;
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
        claimed_any = true;
    }

    let headset_count = hid::attach_hid_devices(BootAttachParams {
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

    if headset_count > 0 {
        usbv!(
            "usb: enum port {} claimed HID headset slot={} vid=0x{:04X} pid=0x{:04X} count={}\n",
            target_port,
            slot_id,
            dev_vid,
            dev_pid,
            headset_count
        );
        register_device(slot_id as u32, target_port, DeviceKind::Headset);
        claimed_any = true;
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
        claimed_any = true;
    }

    if pen::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PEN slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Pen);
        claimed_any = true;
    }
    if print::try_handle(cfg_slice, target_port) {
        usbv!(
            "usb: enum port {} claimed PRINTER slot={}\n",
            target_port,
            slot_id
        );
        register_device(slot_id as u32, target_port, DeviceKind::Printer);
        claimed_any = true;
    }

    if !tried_uac
        && uac::attach_device(uac::AttachParams {
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
        claimed_any = true;
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
        claimed_any = true;
    }

    if claimed_any {
        return;
    }

    // Not claimed: rate-limited log + free the slot so we don't leak up to MaxSlots.
    let portsc = unsafe { ctx.portsc((target_port - 1) as usize) };
    let (ccs, ped, speed) = decode_port_status(portsc);
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

fn init_controller(info: xhci::XhcInfo) -> Result<UsbControllerState, ()> {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    let page_size_mask = unsafe { ctx.page_size_mask() };
    if (page_size_mask & 0x1) == 0 {
        crate::log!(
            "usb: xhci lacks 4K page support PAGESIZE=0x{:X}\n",
            page_size_mask
        );
        return Err(());
    }
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
    let ctx_stride_bytes: usize = if csz_64 { 0x40 } else { 0x20 };
    let ctx_stride_words: usize = ctx_stride_bytes / 4;

    let max_slots = (ctx.hcsparams1 & 0xFF) as usize;
    let (dcbaa_phys, dcbaa_virt) = match dma::alloc((max_slots + 1) * 8, 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc dcbaa\n");
            return Err(());
        }
    };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    let scratchpad_count = ctx.max_scratchpad_buffers() as usize;
    let mut scratchpad_array_phys: u64 = 0;
    let mut scratchpad_array_virt: *mut u8 = core::ptr::null_mut();
    if scratchpad_count > 0 {
        let array_bytes = scratchpad_count * core::mem::size_of::<u64>();
        let (sp_array_phys, sp_array_virt) = match dma::alloc(array_bytes, 64) {
            Some(pair) => pair,
            None => {
                crate::log!(
                    "usb: failed to alloc scratchpad array count={}\n",
                    scratchpad_count
                );
                return Err(());
            }
        };
        unsafe { write_bytes(sp_array_virt, 0, array_bytes) };

        for idx in 0..scratchpad_count {
            let (buf_phys, buf_virt) = match dma::alloc(SCRATCHPAD_BUF_SIZE, SCRATCHPAD_BUF_SIZE) {
                Some(pair) => pair,
                None => {
                    crate::log!(
                        "usb: failed to alloc scratchpad buffer {}/{}\n",
                        idx + 1,
                        scratchpad_count
                    );
                    return Err(());
                }
            };
            unsafe {
                write_bytes(buf_virt, 0, SCRATCHPAD_BUF_SIZE);
                let arr_ptr = sp_array_virt as *mut u64;
                write_volatile(arr_ptr.add(idx), buf_phys);
            }
        }

        unsafe {
            let dcbaa = dcbaa_virt as *mut u64;
            write_volatile(dcbaa, sp_array_phys);
        }

        scratchpad_array_phys = sp_array_phys;
        scratchpad_array_virt = sp_array_virt;
        crate::log!(
            "usb: scratchpads={} array=0x{:X}\n",
            scratchpad_count,
            sp_array_phys
        );
    }

    const CMD_RING_TRBS: usize = 64;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc cmd ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let mut cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 64;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc event ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let mut event_ring =
        unsafe { EventRing::new(evt_phys, evt_virt_raw as *mut Trb, EVENT_RING_TRBS) };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc ERST\n");
            return Err(());
        }
    };
    unsafe {
        write_bytes(erst_virt, 0, size_of::<ErstEntry>());
        let entry = &mut *(erst_virt as *mut ErstEntry);
        entry.seg_base_lo = lo(evt_phys);
        entry.seg_base_hi = hi(evt_phys);
        entry.seg_size = EVENT_RING_TRBS as u32;
    }

    unsafe {
        write_reg64(ctx.op_base, 0x30, dcbaa_phys);
        // CONFIG.MaxSlotsEn: must be >= number of slots we intend to use.
        // Real hardware is much less forgiving than QEMU if this is too small.
        let slots_en = core::cmp::max(1, core::cmp::min(255, max_slots)) as u32;
        write_volatile(ctx.op_base.add(0x38 / 4), slots_en);

        const ERSTSZ: usize = 0x08 / 4;
        const ERSTBA: usize = 0x10 / 4;
        let intr0 = ctx.runtime.add(0x20 / 4);
        write_volatile(intr0.add(ERSTSZ), 1);
        write_volatile(intr0.add(ERSTBA), lo(erst_phys));
        write_volatile(intr0.add(ERSTBA + 1), hi(erst_phys));
        event_ring.update_erdp(intr0);
        xhci::install_event_ring(event_ring, intr0);

        write_reg64(ctx.op_base, 0x18, cmd_ring.crcr_value());

        const USBCMD: usize = 0x00 / 4;
        const USBSTS: usize = 0x04 / 4;
        const USBCMD_RS: u32 = 1 << 0;
        const USBSTS_HCH: u32 = 1 << 0;

        // Clear sticky status bits that are RW1C. On some real machines these can be
        // left set by firmware (notably SRE) and the controller may refuse to run.
        // Bits: HSE(2), EINT(3), PCD(4), SRE(10)
        const USBSTS_RW1C_MASK: u32 = (1 << 2) | (1 << 3) | (1 << 4) | (1 << 10);
        let sts0 = read_volatile(ctx.op_base.add(USBSTS));
        let clear = sts0 & USBSTS_RW1C_MASK;
        if clear != 0 {
            write_volatile(ctx.op_base.add(USBSTS), clear);
        }

        write_volatile(ctx.op_base.add(USBCMD), USBCMD_RS);
        let mut spin: u32 = 1_000_000;
        while spin > 0 {
            let sts = read_volatile(ctx.op_base.add(USBSTS));
            if (sts & USBSTS_HCH) == 0 {
                break;
            }
            spin -= 1;
        }
    }

    crate::log!("usb: controller initialized; ready for rescans\n");

    Ok(UsbControllerState {
        info,
        ctx,
        ctx_stride_bytes,
        ctx_stride_words,
        dcbaa_phys,
        dcbaa_virt,
        scratchpad_array_phys,
        scratchpad_array_virt,
        scratchpad_count: scratchpad_count as u32,
        cmd_ring,
        _cmd_phys: cmd_phys,
        _cmd_virt: cmd_virt_raw,
        _evt_phys: evt_phys,
        _evt_virt: evt_virt_raw,
        _erst_phys: erst_phys,
        _erst_virt: erst_virt,
    })
}

fn has_device_on_port(port: u8) -> bool {
    DEVICES.lock().iter().any(|d| d.port == port)
}

async fn cleanup_disconnected<const N: usize>(
    connected: &Vec<(u8, u32), N>,
    state: &mut UsbControllerState,
) {
    let mut removed: Vec<(u32, DeviceKinds), MAX_DEVICES> = Vec::new();
    {
        let mut guard = DEVICES.lock();
        let mut idx = 0usize;
        while idx < guard.len() {
            let port = guard[idx].port;
            let still_connected = connected.iter().any(|(p, _)| *p == port);
            if still_connected {
                idx += 1;
                continue;
            }
            let entry = guard.remove(idx);
            let _ = removed.push((entry.slot_id, entry.kinds));
        }
    }

    for (slot_id, kinds) in removed.into_iter() {
        if let Err(()) = disable_slot(state, slot_id).await {
            crate::log!("usb: disable-slot for slot {} failed\n", slot_id);
        }
        if kinds.any_hidlike() {
            let _ = hid::unregister_runtime(slot_id);
        }
        if kinds.contains(DeviceKind::Mass) {
            let _ = mass::unregister_runtime(slot_id);
        }
        if kinds.contains(DeviceKind::Cdc) {
            let _ = cdc_acm::unregister_runtime(slot_id);
        }
        if kinds.contains(DeviceKind::Uac) {
            let _ = uac::unregister_runtime(slot_id);
        }
        crate::log!("usb: dropped device slot={} (disconnected)\n", slot_id);
    }
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

struct EnumReadyGuard;

impl Drop for EnumReadyGuard {
    fn drop(&mut self) {
        ENUM_READY.store(true, Ordering::Release);
    }
}

fn register_device(slot_id: u32, port: u8, kind: DeviceKind) {
    let mut guard = DEVICES.lock();
    if let Some(existing) = guard.iter_mut().find(|d| d.slot_id == slot_id) {
        existing.kinds.insert(kind);
        existing.port = port;
        return;
    }
    if guard
        .push(DeviceEntry {
            slot_id,
            port,
            kinds: {
                let mut kinds = DeviceKinds::empty();
                kinds.insert(kind);
                kinds
            },
        })
        .is_err()
    {
        crate::log!("usb: device table full, dropping slot {}\n", slot_id);
    }
    // Signal that at least one device is enumerated so poll_task can start.
    ENUM_READY.store(true, Ordering::Release);
    crate::log!(
        "usb: device claimed slot={} port={} kinds=0x{:02X}\n",
        slot_id,
        port,
        guard
            .iter()
            .find(|d| d.slot_id == slot_id)
            .map(|d| d.kinds.0)
            .unwrap_or(0)
    );
}

fn device_kinds_for_slot(slot_id: u32) -> Option<DeviceKinds> {
    DEVICES
        .lock()
        .iter()
        .find(|d| d.slot_id == slot_id)
        .map(|d| d.kinds)
}

fn any_hid_registered() -> bool {
    DEVICES
        .lock()
        .iter()
        .any(|d| d.kinds.any_hidlike())
}

#[embassy_executor::task]
pub async fn usb_scout(info: xhci::XhcInfo) {
    if SCOUT_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("usb: scout already running; skipping\n");
        return;
    }

    struct ScoutRunGuard;
    impl Drop for ScoutRunGuard {
        fn drop(&mut self) {
            SCOUT_RUNNING.store(false, Ordering::Release);
        }
    }

    let _scout_guard = ScoutRunGuard;

    // Take controller state out of the mutex so we don't hold a spinlock across `.await`.
    let state = USB_CTRL.lock().take();
    let mut state = match state {
        Some(existing) => existing,
        None => {
            // First run: do controller init. Keep ENUM_READY false until the first scan completes.
            ENUM_READY.store(false, Ordering::Release);
            let _guard = EnumReadyGuard;
            match init_controller(info) {
                Ok(s) => s,
                Err(()) => {
                    return;
                }
            }
        }
    };

    // Always rescan ports; enumerate newly connected devices.
    if USB_LOG_VERBOSE {
        xhci::log_ports_table(&state.ctx);
    }

    let mut connected: Vec<(u8, u32), 64> = Vec::new();
    let mut connected_overflowed = false;
    for port in 0..state.ctx.port_count {
        let status = unsafe { state.ctx.portsc(port as usize) };
        let (connected_flag, _, _) = decode_port_status(status);
        if connected_flag {
            if connected.push(((port + 1) as u8, status)).is_err() {
                connected_overflowed = true;
            }
        }
    }

    // If we couldn't record all connected ports, don't treat missing entries as disconnects.
    if connected_overflowed {
        crate::log!("usb: connected port list overflow; skipping disconnect cleanup this pass\n");
    } else {
        cleanup_disconnected(&connected, &mut state).await;
    }

    for (target_port, _port_status) in connected.iter().copied() {
        if has_device_on_port(target_port) {
            continue;
        }

        enumerate_port(&mut state, target_port).await;
    }

    *USB_CTRL.lock() = Some(state);
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

        // Keep transfer-event dispatch reasonably responsive for periodic endpoints.
        // (UAC batches IOC, so we don't need to poll at extreme rates here.)
        let evt_opt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) as u32;
                device_kinds_for_slot(evt_slot).is_some()
            },
            4_000,
            EmbassyDuration::from_micros(250),
        )
        .await;

        let Some(evt) = evt_opt else {
            idle_timeouts = idle_timeouts.wrapping_add(1);
            continue;
        };

        idle_timeouts = 0;

        let evt_slot = (evt.d3 >> 24) as u32;

        let Some(kinds) = device_kinds_for_slot(evt_slot) else {
            // A device may complete transfers during attach (while the enum path is still
            // running) before `register_device()` marks the slot kind. Handle CDC events
            // opportunistically so TX/RX doesn't stall.
            let _ = cdc_acm::handle_transfer_event(&evt);
            continue;
        };

        let mut handled = false;
        if kinds.any_hidlike() && any_hid_registered() {
            let ep_target = (evt.d3 >> 16) & 0x1F;
            if ep_target != 0 {
                handled = hid::with_runtime_mut_by_slot_and_target(evt_slot, ep_target, |runtime| {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    let residual = evt.d2 & 0x00FF_FFFF;
                    let data_len = runtime.report_len.min(runtime.ep.max_packet as u32) as usize;
                    let data = unsafe { core::slice::from_raw_parts(runtime.report_virt, data_len) };
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
        }

        if handled {
            continue;
        }

        if kinds.contains(DeviceKind::Mass) {
            // Mass storage transfers are driven by the mass driver; nothing to do here yet.
            continue;
        }
        if kinds.contains(DeviceKind::Cdc) {
            if !cdc_acm::handle_transfer_event(&evt) {
                usbv!(
                    "usb: ignoring transfer event slot={} (no CDC runtime)\n",
                    evt_slot
                );
            }
            continue;
        }
        if kinds.contains(DeviceKind::Uac) {
            let _ = uac::handle_transfer_event(&evt);
            continue;
        }
    }
}
