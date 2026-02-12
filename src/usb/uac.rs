//! Minimal USB Audio Class (UAC) bind + isoch OUT streaming.
//!
//! Current scope:
//! - Find an AudioStreaming OUT interface altsetting with an isoch OUT endpoint.
//! - SET_CONFIGURATION and SET_INTERFACE.
//! - Configure the xHCI isoch OUT endpoint.
//! - Provide a small API to feed isoch OUT packets.

use crate::audio::PcmFormat;
use crate::pci::dma;
use crate::usb::isoch::{IsochOutConfig, IsochOutPipe};
use crate::usb::xhci::{self, Trb, TrbRing, XhciContext, MAX_XHCI_CONTROLLERS};
use crate::wait;
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::{Deque, Vec};
use spin::Mutex;

use core::ptr::{write_bytes, write_volatile};
use core::sync::atomic::{AtomicU32, Ordering};
use core::task::Waker;

const DEMO_WAV_EMBEDDED: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/demo.wav"));

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_AUDIOCONTROL: u8 = 0x01;
const USB_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;

#[derive(Copy, Clone, Debug)]
struct AsOutEndpoint {
    configuration_value: u8,
    interface: u8,
    alt_setting: u8,
    ep_addr: u8,
    max_packet: u16,
    interval: u8,
    sync_type: u8,
    has_feedback_ep: bool,
    max_esit_payload: u16,
    ss_max_burst: u8,
    ss_mult: u8,
}

#[derive(Copy, Clone, Debug)]
struct DmaBuf {
    phys: u64,
    virt: *mut u8,
}

unsafe impl Send for DmaBuf {}
unsafe impl Sync for DmaBuf {}

/// Minimal UAC sink backed by an isoch OUT pipe.
pub struct UacSink {
    pub fmt: PcmFormat,
    pipe: Option<IsochOutPipe>,
}

impl UacSink {
    pub const fn new(fmt: PcmFormat) -> Self {
        Self { fmt, pipe: None }
    }

    pub async fn configure_isoch(
        &mut self,
        ctx: &XhciContext,
        cmd_ring: &mut TrbRing,
        dev_ctx_virt: *mut u8,
        ctx_stride_bytes: usize,
        ctx_stride_words: usize,
        slot_id: u32,
        ep_addr: u8,
        max_packet: u16,
        interval: u8,
        max_esit_payload: u16,
        ss_max_burst: u8,
        ss_mult: u8,
        speed_code: u32,
        target_port: u8,
    ) -> Result<(), ()> {
        let pipe = IsochOutPipe::create(
            ctx,
            cmd_ring,
            IsochOutConfig {
                slot_id,
                ep_addr,
                max_packet,
                interval,
                max_esit_payload,
                ss_max_burst,
                ss_mult,
                dev_ctx_virt,
                ctx_stride_bytes,
                ctx_stride_words,
                speed_code,
                target_port,
            },
        )
        .await?;
        self.pipe = Some(pipe);
        Ok(())
    }
}

pub struct AttachParams<'a> {
    pub ctx: &'a XhciContext,
    pub cmd_ring: &'a mut TrbRing,
    pub ep0_ring: &'a mut TrbRing,
    pub slot_id: u32,
    pub cfg: &'a [u8],
    pub dev_ctx_virt: *mut u8,
    pub ctx_stride_bytes: usize,
    pub ctx_stride_words: usize,
    pub speed_code: u32,
    pub target_port: u8,
}

struct UacRuntime {
    ctx: XhciContext,
    slot_id: u32,
    pipe_target: u32,
    pipe: IsochOutPipe,
    bufs: Vec<DmaBuf, 64>,
    buf_idx: usize,
    in_flight: usize,
    rate_hz: u32,
    channels: u16,
    bits_per_sample: u8,
    interval: u8,
    sync_type: u8,
    has_feedback_ep: bool,
    speed_code: u32,
    phase_accum: u64,
    fill_waker: Option<Waker>,
}

unsafe impl Send for UacRuntime {}
unsafe impl Sync for UacRuntime {}

static UAC_SLOT: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static UAC_RUNTIME: [Mutex<Option<UacRuntime>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];
static UAC_XFER_OK: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static UAC_XFER_ERR: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static UAC_XFER_LAST_CC: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
const UAC_EVENT_QUEUE_CAP: usize = 256;
static UAC_EVENT_OWNER_CPU: [AtomicU32; MAX_XHCI_CONTROLLERS] =
    [const { AtomicU32::new(0) }; MAX_XHCI_CONTROLLERS];
static UAC_EVENT_QUEUE: [Mutex<Deque<Trb, UAC_EVENT_QUEUE_CAP>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Deque::new()) }; MAX_XHCI_CONTROLLERS];

fn first_active_controller() -> Option<usize> {
    for id in 0..MAX_XHCI_CONTROLLERS {
        if UAC_SLOT[id].load(Ordering::Acquire) != 0 {
            return Some(id);
        }
    }
    None
}

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let mut guard = UAC_RUNTIME[controller_id].lock();
    if let Some(rt) = guard.as_ref() {
        if rt.slot_id == slot_id {
            if let Some(w) = rt.fill_waker.as_ref() {
                w.wake_by_ref();
            }
            *guard = None;
            UAC_SLOT[controller_id].store(0, Ordering::Release);
            UAC_XFER_OK[controller_id].store(0, Ordering::Release);
            UAC_XFER_ERR[controller_id].store(0, Ordering::Release);
            UAC_XFER_LAST_CC[controller_id].store(0, Ordering::Release);
            UAC_EVENT_QUEUE[controller_id].lock().clear();
            return true;
        }
    }
    false
}

fn process_transfer_event(controller_id: usize, evt: &Trb) -> bool {
    // We rely on time-based pacing and a large isoch ring; events are used for observability.
    let evt_type = (evt.d3 >> 10) & 0x3F;
    if evt_type != 32 {
        return false;
    }

    let evt_slot = (evt.d3 >> 24) & 0xFF;
    let evt_ep_id = (evt.d3 >> 16) & 0x1F;
    let cc = (evt.d2 >> 24) & 0xFF;

    let mut guard = UAC_RUNTIME[controller_id].lock();
    let Some(rt) = guard.as_mut() else {
        return false;
    };
    if rt.slot_id != evt_slot {
        return false;
    }
    if rt.pipe_target != evt_ep_id {
        return true;
    }

    if cc == 1 {
        UAC_XFER_OK[controller_id].fetch_add(1, Ordering::Relaxed);
    } else {
        UAC_XFER_ERR[controller_id].fetch_add(1, Ordering::Relaxed);
        UAC_XFER_LAST_CC[controller_id].store(cc, Ordering::Relaxed);
    }
    if rt.in_flight > 0 {
        rt.in_flight -= 1;
    }
    wait::take_and_wake(&mut rt.fill_waker);
    true
}

pub fn handle_transfer_event(controller_id: usize, evt: &Trb) -> bool {
    process_transfer_event(controller_id, evt)
}

fn this_cpu_tag() -> u32 {
    crate::percpu::this_cpu().cpu_index().saturating_add(1)
}

pub fn claim_event_queue_owner(controller_id: usize) -> bool {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return false;
    }
    let mine = this_cpu_tag();
    let owner = &UAC_EVENT_OWNER_CPU[controller_id];
    match owner.compare_exchange(0, mine, Ordering::AcqRel, Ordering::Acquire) {
        Ok(_) => true,
        Err(cur) => cur == mine,
    }
}

pub fn release_event_queue_owner(controller_id: usize) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }
    let mine = this_cpu_tag();
    let owner = &UAC_EVENT_OWNER_CPU[controller_id];
    if owner
        .compare_exchange(mine, 0, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        UAC_EVENT_QUEUE[controller_id].lock().clear();
    }
}

pub fn queue_transfer_event_if_owned(controller_id: usize, evt: &Trb) -> bool {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return false;
    }
    if UAC_EVENT_OWNER_CPU[controller_id].load(Ordering::Acquire) == 0 {
        return false;
    }
    let mut q = UAC_EVENT_QUEUE[controller_id].lock();
    q.push_back(*evt).is_ok()
}

pub fn drain_owned_event_queue(controller_id: usize, budget: usize) -> usize {
    if controller_id >= MAX_XHCI_CONTROLLERS || budget == 0 {
        return 0;
    }
    if UAC_EVENT_OWNER_CPU[controller_id].load(Ordering::Acquire) != this_cpu_tag() {
        return 0;
    }
    let mut drained = 0usize;
    for _ in 0..budget {
        let evt = {
            let mut q = UAC_EVENT_QUEUE[controller_id].lock();
            q.pop_front()
        };
        let Some(evt) = evt else {
            break;
        };
        let _ = process_transfer_event(controller_id, &evt);
        drained += 1;
    }
    drained
}

fn setup_std_nodata(bm_request_type: u8, request: u8, value: u16, index: u16) -> Trb {
    Trb {
        d0: (bm_request_type as u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: (index as u32),
        // Setup Stage TRB: TRB Transfer Length=8, TRT=0 (no data stage)
        d2: 8,
        d3: xhci::trb_type(2) | (1 << 6),
    }
}

fn parse_uac2(cfg: &[u8]) -> bool {
    let mut idx = 0usize;
    let mut in_ac = false;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];

        match ty {
            4 if len >= 9 => {
                let alt = cfg[idx + 3];
                let cls = cfg[idx + 5];
                let sub = cfg[idx + 6];
                in_ac = cls == USB_CLASS_AUDIO && sub == USB_SUBCLASS_AUDIOCONTROL && alt == 0;
            }
            0x24 if in_ac && len >= 5 => {
                let subtype = cfg[idx + 2];
                if subtype == 0x01 {
                    let bcdadc = u16::from_le_bytes([cfg[idx + 3], cfg[idx + 4]]);
                    return bcdadc >= 0x0200;
                }
            }
            _ => {}
        }

        idx += len;
    }

    false
}

#[derive(Default)]
struct UacRateInfo {
    rates: Vec<u32, 16>,
    range: Option<(u32, u32)>,
}

fn parse_as_sample_rates(cfg: &[u8], ifnum: u8, alt: u8, uac2: bool) -> UacRateInfo {
    let mut info = UacRateInfo::default();
    let mut idx = 0usize;
    let mut in_as = false;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];

        match ty {
            4 if len >= 9 => {
                let this_if = cfg[idx + 2];
                let this_alt = cfg[idx + 3];
                let cls = cfg[idx + 5];
                let sub = cfg[idx + 6];
                in_as = cls == USB_CLASS_AUDIO
                    && sub == USB_SUBCLASS_AUDIOSTREAMING
                    && this_if == ifnum
                    && this_alt == alt;
            }
            0x24 if in_as && len >= 8 => {
                let subtype = cfg[idx + 2];
                if subtype != 0x02 {
                    idx += len;
                    continue;
                }
                let format_type = cfg[idx + 3];
                if format_type != 0x01 {
                    idx += len;
                    continue;
                }

                if uac2 {
                    let sam_freq_type = cfg[idx + 6] as usize;
                    let data_off = idx + 7;
                    if sam_freq_type == 0 {
                        if data_off + 8 <= idx + len {
                            let min = u32::from_le_bytes([
                                cfg[data_off],
                                cfg[data_off + 1],
                                cfg[data_off + 2],
                                cfg[data_off + 3],
                            ]);
                            let max = u32::from_le_bytes([
                                cfg[data_off + 4],
                                cfg[data_off + 5],
                                cfg[data_off + 6],
                                cfg[data_off + 7],
                            ]);
                            info.range = Some((min, max));
                        }
                    } else {
                        let mut off = data_off;
                        for _ in 0..sam_freq_type {
                            if off + 4 > idx + len {
                                break;
                            }
                            let rate = u32::from_le_bytes([
                                cfg[off],
                                cfg[off + 1],
                                cfg[off + 2],
                                cfg[off + 3],
                            ]);
                            if info.rates.iter().all(|r| *r != rate) {
                                let _ = info.rates.push(rate);
                            }
                            off += 4;
                        }
                    }
                } else {
                    let sam_freq_type = cfg[idx + 7] as usize;
                    let data_off = idx + 8;
                    if sam_freq_type == 0 {
                        if data_off + 6 <= idx + len {
                            let min = (cfg[data_off] as u32)
                                | ((cfg[data_off + 1] as u32) << 8)
                                | ((cfg[data_off + 2] as u32) << 16);
                            let max = (cfg[data_off + 3] as u32)
                                | ((cfg[data_off + 4] as u32) << 8)
                                | ((cfg[data_off + 5] as u32) << 16);
                            info.range = Some((min, max));
                        }
                    } else {
                        let mut off = data_off;
                        for _ in 0..sam_freq_type {
                            if off + 3 > idx + len {
                                break;
                            }
                            let rate = (cfg[off] as u32)
                                | ((cfg[off + 1] as u32) << 8)
                                | ((cfg[off + 2] as u32) << 16);
                            if info.rates.iter().all(|r| *r != rate) {
                                let _ = info.rates.push(rate);
                            }
                            off += 3;
                        }
                    }
                }
            }
            _ => {}
        }

        idx += len;
    }

    info
}

fn select_sample_rate(info: &UacRateInfo) -> u32 {
    let preferred = crate::audio::DEFAULT_RATE_HZ;
    if !info.rates.is_empty() {
        if info.rates.iter().any(|r| *r == preferred) {
            return preferred;
        }
        if preferred != 44_100 && info.rates.iter().any(|r| *r == 44_100) {
            return 44_100;
        }
        if preferred != 48_000 && info.rates.iter().any(|r| *r == 48_000) {
            return 48_000;
        }
        return info.rates[0];
    }
    if let Some((min, max)) = info.range {
        if min <= preferred && preferred <= max {
            return preferred;
        }
        if preferred != 44_100 && min <= 44_100 && 44_100 <= max {
            return 44_100;
        }
        if preferred != 48_000 && min <= 48_000 && 48_000 <= max {
            return 48_000;
        }
        return min;
    }
    crate::audio::DEFAULT_RATE_HZ
}

fn parse_as_out_endpoint(cfg: &[u8]) -> Option<AsOutEndpoint> {
    if cfg.len() < 9 || cfg[1] != 2 {
        return None;
    }
    let configuration_value = cfg.get(5).copied().unwrap_or(1);

    let mut current_if: Option<(u8, u8, u8)> = None; // (ifnum, alt, (cls/sub) packed)
    let mut current_cls: u8 = 0;
    let mut current_sub: u8 = 0;

    let mut pending_ep: Option<(u8, u16, u8, u8)> = None; // (addr, max_packet, interval, sync_type)
    let mut pending_ss: Option<(u8, u8, u16)> = None; // (max_burst, mult, bytes_per_interval)
    let mut has_feedback_ep = false;
    // If we find a candidate AS isoch OUT endpoint, keep it until we see a SS
    // companion descriptor (0x30) or we move to a new interface.
    let mut candidate: Option<AsOutEndpoint> = None;

    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];

        match ty {
            4 if len >= 9 => {
                // Interface descriptor
                // If we are leaving an AS altsetting and already have a candidate endpoint,
                // return it now (we won't see a SS companion descriptor anymore).
                if candidate.is_some() {
                    return candidate;
                }

                let current_ifnum = cfg[idx + 2];
                let current_alt = cfg[idx + 3];
                current_cls = cfg[idx + 5];
                current_sub = cfg[idx + 6];
                current_if = Some((current_ifnum, current_alt, ((current_cls & 0xF) << 4) | (current_sub & 0xF)));

                // Reset endpoint state on new interface.
                pending_ep = None;
                pending_ss = None;
                has_feedback_ep = false;
            }
            5 if len >= 7 => {
                // Endpoint descriptor
                let ep_addr = cfg[idx + 2];
                let bm_attr = cfg[idx + 3];
                let xfer_ty = bm_attr & 0x3;
                let sync_type = (bm_attr >> 2) & 0x3;
                let usage_type = (bm_attr >> 4) & 0x3;
                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                let interval = cfg[idx + 6];

                // Isoch OUT only.
                let dir_in = (ep_addr & 0x80) != 0;
                if !dir_in && xfer_ty == 0x01 {
                    pending_ep = Some((ep_addr, max_packet, interval, sync_type));
                }
                // Optional explicit feedback endpoint for async OUT endpoints.
                if dir_in && xfer_ty == 0x01 && usage_type == 0x01 {
                    has_feedback_ep = true;
                }
            }
            0x30 if len >= 6 => {
                // SuperSpeed Endpoint Companion Descriptor
                let max_burst = cfg[idx + 2];
                let bm_attr = cfg[idx + 3];
                let mult = bm_attr & 0x3;
                let bytes_per_interval = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                pending_ss = Some((max_burst, mult, bytes_per_interval));

                // If we already captured a candidate endpoint, enrich it with SS companion.
                if let Some(mut c) = candidate {
                    c.ss_max_burst = max_burst;
                    c.ss_mult = mult;
                    c.max_esit_payload = bytes_per_interval;
                    return Some(c);
                }
            }
            _ => {}
        }

        if let (Some((ifnum, alt, _)), Some((ep_addr, max_packet, interval, sync_type))) =
            (current_if, pending_ep)
        {
            if current_cls == USB_CLASS_AUDIO
                && current_sub == USB_SUBCLASS_AUDIOSTREAMING
                && alt != 0
                && candidate.is_none()
            {
                let (ss_max_burst, ss_mult, max_esit_payload) = match pending_ss {
                    Some((b, m, bytes_per_interval)) => (b, m, bytes_per_interval),
                    None => (0, 0, max_packet),
                };
                candidate = Some(AsOutEndpoint {
                    configuration_value,
                    interface: ifnum,
                    alt_setting: alt,
                    ep_addr,
                    max_packet,
                    interval,
                    sync_type,
                    has_feedback_ep,
                    max_esit_payload,
                    ss_max_burst,
                    ss_mult,
                });
            }
        }

        idx += len;
    }

    candidate
}

#[derive(Copy, Clone, Debug)]
struct Uac2ClockSource {
    ac_interface: u8,
    clock_id: u8,
}

fn parse_uac2_clock_source(cfg: &[u8]) -> Option<Uac2ClockSource> {
    let mut idx = 0usize;
    let mut current_ac_if: Option<u8> = None;
    let mut in_ac = false;
    let mut uac2 = false;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];

        match ty {
            4 if len >= 9 => {
                // Interface descriptor
                let ifnum = cfg[idx + 2];
                let alt = cfg[idx + 3];
                let cls = cfg[idx + 5];
                let sub = cfg[idx + 6];
                in_ac = cls == USB_CLASS_AUDIO && sub == USB_SUBCLASS_AUDIOCONTROL && alt == 0;
                current_ac_if = if in_ac { Some(ifnum) } else { None };
                uac2 = false;
            }
            0x24 if in_ac && len >= 3 => {
                // Class-specific AC interface descriptor.
                let subtype = cfg[idx + 2];
                match subtype {
                    0x01 if len >= 5 => {
                        // HEADER. For UAC2 this includes bcdADC = 0x0200.
                        let bcdadc = u16::from_le_bytes([cfg[idx + 3], cfg[idx + 4]]);
                        uac2 = bcdadc >= 0x0200;
                    }
                    0x0A if uac2 && len >= 4 => {
                        // CLOCK_SOURCE (UAC2): bClockID at offset 3.
                        let clock_id = cfg[idx + 3];
                        if let Some(ac_if) = current_ac_if {
                            return Some(Uac2ClockSource { ac_interface: ac_if, clock_id });
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }

        idx += len;
    }

    None
}

pub(crate) fn has_as_out_endpoint(cfg: &[u8]) -> bool {
    parse_as_out_endpoint(cfg).is_some()
}


pub async fn attach_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
        ctx,
        cmd_ring,
        ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    } = params;

    let Some(as_out) = parse_as_out_endpoint(cfg) else {
        crate::log!("usb: uac: no AudioStreaming isoch OUT endpoint found\n");
        return Err(());
    };

    let is_uac2 = parse_uac2(cfg);
    let rate_info = parse_as_sample_rates(cfg, as_out.interface, as_out.alt_setting, is_uac2);
    let selected_rate = select_sample_rate(&rate_info);
    if !rate_info.rates.is_empty() {
        crate::log!(
            "usb: uac: supported rates={:?} selected={} uac2={}\n",
            rate_info.rates,
            selected_rate,
            is_uac2
        );
    } else if let Some((min, max)) = rate_info.range {
        crate::log!(
            "usb: uac: rate range {}..{} selected={} uac2={}\n",
            min,
            max,
            selected_rate,
            is_uac2
        );
    } else {
        crate::log!(
            "usb: uac: no rate info; defaulting to {} Hz uac2={}\n",
            selected_rate,
            is_uac2
        );
    }

    if let Some(cs) = parse_uac2_clock_source(cfg) {
        crate::log!(
            "usb: uac: uac2 clock source id={} ac_if={}\n",
            cs.clock_id,
            cs.ac_interface
        );

        // Best-effort: UAC2 clock source Sampling Frequency control.
        // Devices that don't support this will STALL; ignore errors.
        let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
        unsafe {
            let b = core::slice::from_raw_parts_mut(virt, 8);
            b[..4].copy_from_slice(&selected_rate.to_le_bytes());
        }
        let setup = Trb {
            // bmRequestType=0x21 (Class, Interface, Host->Device), bRequest=SET_CUR(0x01)
            // wValue=(CS=0x01 SamFreq) << 8, wIndex=(EntityID<<8)|Interface, wLength=4
            d0: 0x21 | (0x01 << 8) | (0x0100u32 << 16),
            d1: ((cs.clock_id as u32) << 8) | (cs.ac_interface as u32) | (4u32 << 16),
            d2: 8,
            d3: xhci::trb_type(2) | (1 << 6),
        };
        let _ = super::control_out(
            ctx,
            ep0_ring,
            slot_id,
            setup,
            Some(phys),
            4,
            "uac2-set-sample-rate",
            200,
        )
        .await;
    }

    // SET_CONFIGURATION
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_std_nodata(0x00, 0x09, as_out.configuration_value as u16, 0),
        None,
        0,
        "uac-set-configuration",
        800,
    )
    .await?;

    // SET_INTERFACE to chosen altsetting (crucial for many headsets).
    super::control_out(
        ctx,
        ep0_ring,
        slot_id,
        setup_std_nodata(0x01, 0x0B, as_out.alt_setting as u16, as_out.interface as u16),
        None,
        0,
        "uac-set-interface",
        800,
    )
    .await?;

    // Configure isoch endpoint.
    let mut sink = UacSink::new(PcmFormat {
        rate_hz: selected_rate,
        channels: crate::audio::DEFAULT_CHANNELS as u8,
        bits_per_sample: 16,
    });
    sink.configure_isoch(
        ctx,
        cmd_ring,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        slot_id,
        as_out.ep_addr,
        as_out.max_packet,
        as_out.interval,
        as_out.max_esit_payload,
        as_out.ss_max_burst,
        as_out.ss_mult,
        speed_code,
        target_port,
    )
    .await?;

    let pipe = sink.pipe.take().ok_or(())?;

    // Pre-allocate a small DMA buffer pool sized to the endpoint's packet size.
    let mut bufs: Vec<DmaBuf, 64> = Vec::new();
    for _ in 0..bufs.capacity() {
        let (phys, virt) = dma::alloc(pipe.max_packet as usize, 64).ok_or(())?;
        unsafe { write_bytes(virt, 0, pipe.max_packet as usize) };
        let _ = bufs.push(DmaBuf { phys, virt });
    }

    let pipe_target = pipe.ep_target;

    let controller_id = ctx.controller_id;
    *UAC_RUNTIME[controller_id].lock() = Some(UacRuntime {
        ctx: *ctx,
        slot_id,
        pipe_target,
        pipe,
        bufs,
        buf_idx: 0,
        in_flight: 0,
        rate_hz: sink.fmt.rate_hz,
        channels: sink.fmt.channels as u16,
        bits_per_sample: sink.fmt.bits_per_sample,
        interval: as_out.interval,
        sync_type: as_out.sync_type,
        has_feedback_ep: as_out.has_feedback_ep,
        speed_code,
        phase_accum: 0,
        fill_waker: None,
    });
    UAC_SLOT[controller_id].store(slot_id, Ordering::Release);

    crate::log!(
        "usb: uac attached slot={} if={} alt={} ep=0x{:02X} mps={} interval={} sync={} feedback={}\n",
        slot_id,
        as_out.interface,
        as_out.alt_setting,
        as_out.ep_addr,
        as_out.max_packet,
        as_out.interval,
        as_out.sync_type,
        as_out.has_feedback_ep
    );
    crate::v::readiness::set(crate::v::readiness::UAC_ATTACHED);

    // Best-effort: try to program sampling frequency via the UAC1 endpoint control.
    // Many UAC2 devices will STALL this; ignore errors and continue streaming.
    {
        let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
        let sr = selected_rate;
        unsafe {
            // 24-bit little-endian sampling frequency.
            let b = core::slice::from_raw_parts_mut(virt, 8);
            b[0] = (sr & 0xFF) as u8;
            b[1] = ((sr >> 8) & 0xFF) as u8;
            b[2] = ((sr >> 16) & 0xFF) as u8;
        }

        let setup = Trb {
            // bmRequestType=0x22 (Class, Endpoint, Host->Device), bRequest=SET_CUR(0x01)
            // wValue=(CS=0x01 Sampling Freq) << 8, wIndex=ep_addr, wLength=3
            d0: 0x22 | (0x01 << 8) | (0x0100u32 << 16),
            d1: (as_out.ep_addr as u32) | (3u32 << 16),
            d2: 8,
            d3: xhci::trb_type(2) | (1 << 6),
        };
        let _ = super::control_out(
            ctx,
            ep0_ring,
            slot_id,
            setup,
            Some(phys),
            3,
            "uac-set-sample-rate",
            200,
        )
        .await;
    }

    Ok(())
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DemoQueueError {
    NoDevice,
    NoRuntime,
    FormatMismatch,
    NoPacket,
}

#[derive(Copy, Clone, Debug)]
pub struct DemoPacketReservation {
    pub controller_id: usize,
    pub slot_id: u32,
    pub buf_phys: u64,
    pub buf_virt: *mut u8,
    pub packet_bytes: usize,
    pub payload_bytes: usize,
}

pub fn reserve_demo_packet() -> Result<DemoPacketReservation, DemoQueueError> {
    let controller_id = first_active_controller().ok_or(DemoQueueError::NoDevice)?;
    let mut guard = UAC_RUNTIME[controller_id].lock();
    let rt = guard.as_mut().ok_or(DemoQueueError::NoRuntime)?;

    if rt.in_flight >= rt.bufs.len() {
        return Err(DemoQueueError::NoPacket);
    }

    if rt.bits_per_sample != 16 {
        // Current demo stream is S16LE only.
        return Err(DemoQueueError::FormatMismatch);
    }

    let max_packet_bytes = rt.pipe.max_packet as usize;
    let max_samples = max_packet_bytes / 2;
    if max_samples == 0 {
        return Err(DemoQueueError::NoPacket);
    }

    let channels = core::cmp::max(1, rt.channels as usize);
    let mut samples_needed = if rt.sync_type == 0x02 {
        // Adaptive OUT endpoint: device tracks host pacing.
        // Safe fallback policy: always send full packet-sized payload.
        max_samples - (max_samples % channels)
    } else {
        // Async/synchronous OUT fallback:
        // - if explicit feedback endpoint exists, we still run nominal pacing for now.
        // - otherwise same nominal pacing path.
        let _has_feedback = rt.has_feedback_ep;
        let tick_us: u64 = if rt.speed_code == 1 {
            // Full-speed: bInterval is in 1ms frames.
            core::cmp::max(1, rt.interval as u64) * 1000
        } else {
            // High-/Super-speed: bInterval is 125us microframes as 2^(bInterval-1).
            125u64 * (1u64 << (rt.interval.saturating_sub(1) as u64))
        };

        rt.phase_accum = rt.phase_accum.saturating_add(rt.rate_hz as u64 * tick_us);
        let frames = rt.phase_accum / 1_000_000u64;
        rt.phase_accum %= 1_000_000u64;
        (frames as usize).saturating_mul(channels)
    };

    // Keep sample count aligned to whole frames.
    if samples_needed > max_samples {
        samples_needed = max_samples - (max_samples % channels);
    }

    if samples_needed == 0 {
        return Err(DemoQueueError::NoPacket);
    }

    let payload_bytes = samples_needed * 2;
    let packet_bytes = max_packet_bytes;
    let buf = rt.bufs[rt.buf_idx];
    rt.buf_idx = (rt.buf_idx + 1) % rt.bufs.len();

    Ok(DemoPacketReservation {
        controller_id,
        slot_id: rt.slot_id,
        buf_phys: buf.phys,
        buf_virt: buf.virt,
        packet_bytes,
        payload_bytes,
    })
}

pub fn submit_demo_packet(res: DemoPacketReservation) -> Result<bool, DemoQueueError> {
    let mut guard = UAC_RUNTIME[res.controller_id].lock();
    let rt = guard.as_mut().ok_or(DemoQueueError::NoRuntime)?;
    if rt.slot_id != res.slot_id {
        return Err(DemoQueueError::NoRuntime);
    }
    if rt.in_flight >= rt.bufs.len() {
        return Ok(false);
    }

    if !rt
        .pipe
        // Always schedule immediately (SIA=1) and let the controller place TRBs into
        // successive service intervals. Using a constant frame_id for burst-queued TRBs
        // causes audible burst/gap artifacts (classic crackle/click).
        .push_isoch_trb(res.buf_phys, res.payload_bytes as u32, false, true, None)
    {
        return Ok(false);
    }

    unsafe { write_volatile(rt.ctx.doorbell.add(rt.slot_id as usize), rt.pipe_target) };
    rt.in_flight = rt.in_flight.saturating_add(1);
    Ok(true)
}

#[embassy_executor::task]
pub async fn event_drain_task() {
    async move {
        const IDLE_SLEEP_MS: u64 = 5;
        const ACTIVE_SLEEP_MS: u64 = 1;
        const DRAIN_BUDGET: usize = 128;

        loop {
            let Some(controller_id) = first_active_controller() else {
                Timer::after(EmbassyDuration::from_millis(IDLE_SLEEP_MS)).await;
                continue;
            };

            if !claim_event_queue_owner(controller_id) {
                Timer::after(EmbassyDuration::from_millis(ACTIVE_SLEEP_MS)).await;
                continue;
            }

            loop {
                if UAC_SLOT[controller_id].load(Ordering::Acquire) == 0 {
                    release_event_queue_owner(controller_id);
                    break;
                }

                let drained = drain_owned_event_queue(controller_id, DRAIN_BUDGET);
                if drained == 0 {
                    Timer::after(EmbassyDuration::from_millis(ACTIVE_SLEEP_MS)).await;
                }
            }
        }
    }.await;
}


#[embassy_executor::task]
pub async fn song_task() {
    async move {
        const FRAME_BYTES: usize = 4; // s16le stereo

        let wav = DEMO_WAV_EMBEDDED;

        fn le_u16(s: &[u8]) -> Option<u16> {
            if s.len() < 2 {
                return None;
            }
            Some(u16::from_le_bytes([s[0], s[1]]))
        }
        fn le_u32(s: &[u8]) -> Option<u32> {
            if s.len() < 4 {
                return None;
            }
            Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        }
        fn parse_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Option<(usize, usize)> {
            if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
                return None;
            }
            let mut off = 12usize;
            let mut fmt_ok = false;
            let mut data: Option<(usize, usize)> = None;
            while off + 8 <= bytes.len() {
                let id = &bytes[off..off + 4];
                let sz = le_u32(&bytes[off + 4..off + 8])? as usize;
                let payload = off + 8;
                let end = payload.saturating_add(sz);
                if end > bytes.len() {
                    return None;
                }

                if id == b"fmt " {
                    if sz < 16 {
                        return None;
                    }
                    let fmt = &bytes[payload..payload + sz];
                    let audio_fmt = le_u16(&fmt[0..2])?;
                    let channels = le_u16(&fmt[2..4])?;
                    let rate = le_u32(&fmt[4..8])?;
                    let bits = le_u16(&fmt[14..16])?;
                    if audio_fmt == 1 && channels == 2 && rate == 48_000 && bits == 16 {
                        fmt_ok = true;
                    } else {
                        return None;
                    }
                } else if id == b"data" {
                    data = Some((payload, sz));
                    if fmt_ok {
                        break;
                    }
                }

                off = end + (sz & 1);
            }

            if !fmt_ok {
                return None;
            }
            data
        }

        let Some((data_off, data_len)) = parse_wav_pcm_s16_stereo_48k(wav) else {
            crate::log!("song: unsupported wav format (need pcm s16le stereo 48k)\n");
            return;
        };

        let data = &wav[data_off..data_off + data_len];
        let mut cursor = 0usize;
        let mut logged_playing = false;
        while cursor < data.len() {
            let res = match reserve_demo_packet() {
                Ok(v) => v,
                Err(DemoQueueError::NoDevice | DemoQueueError::NoRuntime) => {
                    Timer::after(EmbassyDuration::from_millis(25)).await;
                    continue;
                }
                Err(DemoQueueError::NoPacket) => {
                    Timer::after(EmbassyDuration::from_millis(1)).await;
                    continue;
                }
                Err(DemoQueueError::FormatMismatch) => {
                    crate::log!("song: runtime format mismatch\n");
                    return;
                }
            };

            let take = unsafe {
                let out = core::slice::from_raw_parts_mut(res.buf_virt, res.packet_bytes);
                let (payload, pad) = out.split_at_mut(res.payload_bytes);

                let remaining = data.len().saturating_sub(cursor);
                let mut take = core::cmp::min(payload.len(), remaining);
                take -= take % FRAME_BYTES;

                if take != 0 {
                    payload[..take].copy_from_slice(&data[cursor..cursor + take]);
                }
                payload[take..].fill(0);
                pad.fill(0);
                take
            };

            if take == 0 {
                // WAV data should be frame-aligned; if not, ignore trailing partial frame.
                break;
            }

            if submit_demo_packet(res).unwrap_or(false) {
                cursor += take;
                if !logged_playing {
                    logged_playing = true;
                    crate::log!("song: playing\n");
                }
            } else {
                // Ring/backpressure: retry same payload position instead of dropping audio.
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
        }
    }.await;
}
