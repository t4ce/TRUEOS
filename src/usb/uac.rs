//! Minimal USB Audio Class (UAC) bind + isoch OUT streaming.
//!
//! Current scope:
//! - Find an AudioStreaming OUT interface altsetting with an isoch OUT endpoint.
//! - SET_CONFIGURATION and SET_INTERFACE.
//! - Configure the xHCI isoch OUT endpoint.
//! - Run a small Embassy task that streams the built-in demo PCM.

use crate::audio::{PcmFormat, PcmSink};
use crate::pci::dma;
use crate::usb::isoch::{IsochOutConfig, IsochOutPipe};
use crate::usb::xhci::{self, Trb, TrbRing, XhciContext};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;
use core::ptr::{write_bytes, write_volatile};
use core::sync::atomic::{AtomicU32, Ordering};
use core::sync::atomic::AtomicU64;
use super::resample_44k1_to_48k::{
    RESAMPLE_COEFFS_Q15, RESAMPLE_OFFSETS, RESAMPLE_PHASES, RESAMPLE_SHIFT, RESAMPLE_TAPS,
};

// One DMA buffer per usable isoch TRB ring slot.
// `TrbRing` reserves the last entry for the Link TRB, so `len=256` => `usable=255`.
const ISOCH_BUF_SLOTS: usize = 255;

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;
const UAC_CS_SAMPLING_FREQ: u16 = 0x01;

#[derive(Copy, Clone, Debug)]
struct AsOutEndpoint {
    configuration_value: u8,
    interface: u8,
    alt_setting: u8,
    ep_addr: u8,
    ep_attr: u8,
    sync_type: u8,
    usage_type: u8,
    synch_address: u8,
    max_packet: u16,
    interval: u8,
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

    fn pipe_mut(&mut self) -> Option<&mut IsochOutPipe> {
        self.pipe.as_mut()
    }
}

impl PcmSink for UacSink {
    fn write(&mut self, _pcm: &[u8]) -> usize {
        0
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
    free_bufs: Vec<DmaBuf, ISOCH_BUF_SLOTS>,
    inflight_bufs: [Option<DmaBuf>; ISOCH_BUF_SLOTS],
    td_lens: [u8; ISOCH_BUF_SLOTS],
    sample_idx: usize,
    in_flight: u32,
    src_frame: u32,
    src_frac160: u16,
    src_rate_hz: u32,
    rate_hz: u32,
    channels: u16,
    bits_per_sample: u8,
    interval: u8,
    speed_code: u32,
}

unsafe impl Send for UacRuntime {}
unsafe impl Sync for UacRuntime {}

static UAC_SLOT: AtomicU32 = AtomicU32::new(0);
static UAC_RUNTIME: Mutex<Option<UacRuntime>> = Mutex::new(None);
static UAC_FORMAT_LOGGED: AtomicU32 = AtomicU32::new(0);
static UAC_PUSHED_PACKETS: AtomicU64 = AtomicU64::new(0);
static UAC_COMPLETED_PACKETS: AtomicU64 = AtomicU64::new(0);
static UAC_COMPLETION_EVENTS: AtomicU64 = AtomicU64::new(0);
static UAC_BAD_EVENT_LOGGED: AtomicU32 = AtomicU32::new(0);

#[embassy_executor::task]
pub async fn stats_task() {
    loop {
        Timer::after(EmbassyDuration::from_millis(1000)).await;
        if UAC_SLOT.load(Ordering::Acquire) == 0 {
            // Keep counters from growing unbounded if the device is unplugged.
            let _ = UAC_PUSHED_PACKETS.swap(0, Ordering::Relaxed);
            let _ = UAC_COMPLETED_PACKETS.swap(0, Ordering::Relaxed);
            let _ = UAC_COMPLETION_EVENTS.swap(0, Ordering::Relaxed);
            continue;
        }
        let pushed = UAC_PUSHED_PACKETS.swap(0, Ordering::Relaxed);
        let completed = UAC_COMPLETED_PACKETS.swap(0, Ordering::Relaxed);
        let events = UAC_COMPLETION_EVENTS.swap(0, Ordering::Relaxed);
        let in_flight = UAC_RUNTIME
            .lock()
            .as_ref()
            .map(|rt| rt.in_flight)
            .unwrap_or(0);
        crate::log!(
            "usb: uac isoch stats pushed={}pkt/s completed={}pkt/s events={}evt/s inflight={}\n",
            pushed,
            completed,
            events,
            in_flight
        );
    }
}

pub fn unregister_runtime(slot_id: u32) -> bool {
    let mut guard = UAC_RUNTIME.lock();
    if let Some(rt) = guard.as_ref() {
        if rt.slot_id == slot_id {
            *guard = None;
            UAC_SLOT.store(0, Ordering::Release);
            return true;
        }
    }
    false
}

pub fn handle_transfer_event(evt: &Trb) -> bool {
    let slot_id = ((evt.d3 >> 24) & 0xFF) as u32;
    let ep_target = (evt.d3 >> 16) & 0x1F;
    let completion = (evt.d2 >> 24) & 0xFF;

    let mut guard = UAC_RUNTIME.lock();
    let Some(rt) = guard.as_mut() else {
        return false;
    };

    if rt.slot_id != slot_id || rt.pipe_target != ep_target {
        return false;
    }

    // Transfer Event TRB Pointer (masked) tells us which TRB completed. Use that
    // to recycle the DMA buffer tied to that ring slot.
    let trb_ptr = ((evt.d0 as u64) | ((evt.d1 as u64) << 32)) & !0xFu64;
    let ring_base = rt.pipe.ring.phys & !0xFu64;
    let ring_end = ring_base + (rt.pipe.ring.len as u64 * core::mem::size_of::<Trb>() as u64);
    if trb_ptr >= ring_base && trb_ptr < ring_end {
        let idx = ((trb_ptr - ring_base) / core::mem::size_of::<Trb>() as u64) as usize;
        if idx < ISOCH_BUF_SLOTS {
            // Completion pointer is the last TRB in the TD. Recycle all buffers in this TD.
            let td_len = rt.td_lens[idx].max(1) as usize;
            rt.td_lens[idx] = 0;
            for back in 0..td_len {
                let slot = (idx + ISOCH_BUF_SLOTS - back) % ISOCH_BUF_SLOTS;
                rt.td_lens[slot] = 0;
                if let Some(buf) = rt.inflight_bufs[slot].take() {
                    let _ = rt.free_bufs.push(buf);
                }
            }

            rt.in_flight = rt.in_flight.saturating_sub(td_len as u32);
            UAC_COMPLETED_PACKETS.fetch_add(td_len as u64, Ordering::Relaxed);
            UAC_COMPLETION_EVENTS.fetch_add(1, Ordering::Relaxed);
            return true;
        }
    }

    let _ = completion; // reserved for future error stats
    if UAC_BAD_EVENT_LOGGED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        crate::log!(
            "usb: uac bad transfer evt slot={} target={} cc={} trb_ptr=0x{:016X} ring=[0x{:016X}..0x{:016X}) slots={}\n",
            slot_id,
            ep_target,
            completion,
            trb_ptr,
            ring_base,
            ring_end,
            ISOCH_BUF_SLOTS
        );
    }
    false
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

fn parse_as_out_endpoint(cfg: &[u8]) -> Option<AsOutEndpoint> {
    if cfg.len() < 9 || cfg[1] != 2 {
        return None;
    }
    let configuration_value = cfg.get(5).copied().unwrap_or(1);

    let mut current_if: Option<(u8, u8, u8)> = None; // (ifnum, alt, (cls/sub) packed)
    let mut current_cls: u8 = 0;
    let mut current_sub: u8 = 0;
    let mut current_alt: u8 = 0;
    let mut current_ifnum: u8 = 0;

    struct PendingEp {
        addr: u8,
        attr: u8,
        sync_type: u8,
        usage_type: u8,
        synch_address: u8,
        max_packet: u16,
        interval: u8,
    }

    let mut pending_ep: Option<PendingEp> = None;
    let mut pending_ss: Option<(u8, u8, u16)> = None; // (max_burst, mult, bytes_per_interval)
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

                current_ifnum = cfg[idx + 2];
                current_alt = cfg[idx + 3];
                current_cls = cfg[idx + 5];
                current_sub = cfg[idx + 6];
                current_if = Some((current_ifnum, current_alt, ((current_cls & 0xF) << 4) | (current_sub & 0xF)));

                // Reset endpoint state on new interface.
                pending_ep = None;
                pending_ss = None;
            }
            5 if len >= 7 => {
                // Endpoint descriptor
                let ep_addr = cfg[idx + 2];
                let bm_attr = cfg[idx + 3];
                let xfer_ty = bm_attr & 0x3;
                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                let interval = cfg[idx + 6];
                let synch_address = if len >= 9 { cfg[idx + 8] } else { 0 };
                let sync_type = (bm_attr >> 2) & 0x3;
                let usage_type = (bm_attr >> 4) & 0x3;

                // Isoch OUT only.
                let dir_in = (ep_addr & 0x80) != 0;
                if !dir_in && xfer_ty == 0x01 {
                    pending_ep = Some(PendingEp {
                        addr: ep_addr,
                        attr: bm_attr,
                        sync_type,
                        usage_type,
                        synch_address,
                        max_packet,
                        interval,
                    });
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

        if let (Some((ifnum, alt, _)), Some(ep)) = (current_if, pending_ep.as_ref()) {
            if current_cls == USB_CLASS_AUDIO
                && current_sub == USB_SUBCLASS_AUDIOSTREAMING
                && alt != 0
                && candidate.is_none()
            {
                let (ss_max_burst, ss_mult, max_esit_payload) = match pending_ss {
                    Some((b, m, bytes_per_interval)) => (b, m, bytes_per_interval),
                    None => (0, 0, ep.max_packet),
                };
                candidate = Some(AsOutEndpoint {
                    configuration_value,
                    interface: ifnum,
                    alt_setting: alt,
                    ep_addr: ep.addr,
                    ep_attr: ep.attr,
                    sync_type: ep.sync_type,
                    usage_type: ep.usage_type,
                    synch_address: ep.synch_address,
                    max_packet: ep.max_packet,
                    interval: ep.interval,
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

#[derive(Clone, Debug)]
struct AsFormatInfo {
    channels: u8,
    subframe_bytes: u8,
    bit_resolution: u8,
    rates: heapless::Vec<u32, 16>,
    rate_min: u32,
    rate_max: u32,
}

fn parse_as_format_info(cfg: &[u8], target_if: u8, target_alt: u8) -> Option<AsFormatInfo> {
    let mut current_ifnum: u8 = 0;
    let mut current_alt: u8 = 0;
    let mut current_cls: u8 = 0;
    let mut current_sub: u8 = 0;

    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];

        match ty {
            4 if len >= 9 => {
                current_ifnum = cfg[idx + 2];
                current_alt = cfg[idx + 3];
                current_cls = cfg[idx + 5];
                current_sub = cfg[idx + 6];
            }
            0x24 if len >= 8 => {
                let subtype = cfg[idx + 2];
                if current_ifnum == target_if
                    && current_alt == target_alt
                    && current_cls == USB_CLASS_AUDIO
                    && current_sub == USB_SUBCLASS_AUDIOSTREAMING
                    && subtype == 0x02
                {
                    let channels = cfg[idx + 4];
                    let subframe_bytes = cfg[idx + 5];
                    let bit_resolution = cfg[idx + 6];
                    let samfreq_type = cfg[idx + 7];
                    let mut rates: heapless::Vec<u32, 16> = heapless::Vec::new();
                    let mut rate_min = 0u32;
                    let mut rate_max = 0u32;

                    if samfreq_type == 0 && len >= 14 {
                        rate_min = u32::from_le_bytes([cfg[idx + 8], cfg[idx + 9], cfg[idx + 10], 0]);
                        rate_max = u32::from_le_bytes([cfg[idx + 11], cfg[idx + 12], cfg[idx + 13], 0]);
                    } else {
                        let count = samfreq_type as usize;
                        let mut off = idx + 8;
                        for _ in 0..count {
                            if off + 3 > idx + len {
                                break;
                            }
                            let rate = u32::from_le_bytes([cfg[off], cfg[off + 1], cfg[off + 2], 0]);
                            let _ = rates.push(rate);
                            off += 3;
                        }
                    }

                    return Some(AsFormatInfo {
                        channels,
                        subframe_bytes,
                        bit_resolution,
                        rates,
                        rate_min,
                        rate_max,
                    });
                }
            }
            _ => {}
        }

        idx += len;
    }

    None
}

async fn set_sampling_freq_endpoint(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    ep_addr: u8,
    rate_hz: u32,
) -> Result<u32, ()> {
    let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
    unsafe {
        let out = core::slice::from_raw_parts_mut(virt, 3);
        out[0] = (rate_hz & 0xFF) as u8;
        out[1] = ((rate_hz >> 8) & 0xFF) as u8;
        out[2] = ((rate_hz >> 16) & 0xFF) as u8;
    }

    let setup = Trb {
        // bmRequestType=0x22 (Host->Dev | Class | Endpoint)
        d0: 0x22 | ((0x01u32) << 8) | ((UAC_CS_SAMPLING_FREQ as u32) << 16),
        d1: ep_addr as u32,
        d2: 8,
        d3: xhci::trb_type(2) | (1 << 6),
    };

    match super::control_out_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        Some(phys),
        3,
        "uac-set-sampling-freq-ep",
        800,
    )
    .await
    {
        Ok(cc) => {
            crate::log!(
                "usb: uac set-cur ep bm=0x22 wValue=0x{:04X} wIndex=0x{:02X} len=3 cc={}\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                ep_addr,
                cc
            );
            Ok(cc)
        }
        Err(()) => {
            crate::log!(
                "usb: uac set-cur ep bm=0x22 wValue=0x{:04X} wIndex=0x{:02X} len=3 failed\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                ep_addr
            );
            Err(())
        }
    }
}

async fn set_sampling_freq_interface(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    rate_hz: u32,
) -> Result<u32, ()> {
    let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
    unsafe {
        let out = core::slice::from_raw_parts_mut(virt, 3);
        out[0] = (rate_hz & 0xFF) as u8;
        out[1] = ((rate_hz >> 8) & 0xFF) as u8;
        out[2] = ((rate_hz >> 16) & 0xFF) as u8;
    }

    let setup = Trb {
        // bmRequestType=0x21 (Host->Dev | Class | Interface)
        d0: 0x21 | ((0x01u32) << 8) | ((UAC_CS_SAMPLING_FREQ as u32) << 16),
        d1: iface as u32,
        d2: 8,
        d3: xhci::trb_type(2) | (1 << 6),
    };

    match super::control_out_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        Some(phys),
        3,
        "uac-set-sampling-freq-if",
        800,
    )
    .await
    {
        Ok(cc) => {
            crate::log!(
                "usb: uac set-cur if bm=0x21 wValue=0x{:04X} wIndex=0x{:02X} len=3 cc={}\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                iface,
                cc
            );
            Ok(cc)
        }
        Err(()) => {
            crate::log!(
                "usb: uac set-cur if bm=0x21 wValue=0x{:04X} wIndex=0x{:02X} len=3 failed\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                iface
            );
            Err(())
        }
    }
}

async fn get_sampling_freq_endpoint(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    ep_addr: u8,
) -> Result<(u32, u32), ()> {
    let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
    unsafe { write_bytes(virt, 0, 8) };

    let setup = Trb {
        // bmRequestType=0xA2 (Dev->Host | Class | Endpoint)
        d0: 0xA2 | ((0x01u32) << 8) | ((UAC_CS_SAMPLING_FREQ as u32) << 16),
        d1: ep_addr as u32,
        d2: 8,
        d3: xhci::trb_type(2) | (1 << 6),
    };

    match super::control_in_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        phys,
        3,
        "uac-get-sampling-freq-ep",
        800,
    )
    .await
    {
        Ok((cc, transferred)) => {
            let rate = if transferred >= 3 {
                unsafe {
                    let bytes = core::slice::from_raw_parts(virt as *const u8, 3);
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
                }
            } else {
                0
            };

            crate::log!(
                "usb: uac get-cur ep bm=0xA2 wValue=0x{:04X} wIndex=0x{:02X} len=3 cc={} rate={} Hz\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                ep_addr,
                cc,
                rate
            );
            Ok((cc, rate))
        }
        Err(()) => {
            crate::log!(
                "usb: uac get-cur ep bm=0xA2 wValue=0x{:04X} wIndex=0x{:02X} len=3 failed\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                ep_addr
            );
            Err(())
        }
    }
}

async fn get_sampling_freq_interface(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
) -> Result<(u32, u32), ()> {
    let (phys, virt) = dma::alloc(8, 8).ok_or(())?;
    unsafe { write_bytes(virt, 0, 8) };

    let setup = Trb {
        // bmRequestType=0xA1 (Dev->Host | Class | Interface)
        d0: 0xA1 | ((0x01u32) << 8) | ((UAC_CS_SAMPLING_FREQ as u32) << 16),
        d1: iface as u32,
        d2: 8,
        d3: xhci::trb_type(2) | (1 << 6),
    };

    match super::control_in_cc(
        ctx,
        ep0_ring,
        slot_id,
        setup,
        phys,
        3,
        "uac-get-sampling-freq-if",
        800,
    )
    .await
    {
        Ok((cc, transferred)) => {
            let rate = if transferred >= 3 {
                unsafe {
                    let bytes = core::slice::from_raw_parts(virt as *const u8, 3);
                    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0])
                }
            } else {
                0
            };

            crate::log!(
                "usb: uac get-cur if bm=0xA1 wValue=0x{:04X} wIndex=0x{:02X} len=3 cc={} rate={} Hz\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                iface,
                cc,
                rate
            );
            Ok((cc, rate))
        }
        Err(()) => {
            crate::log!(
                "usb: uac get-cur if bm=0xA1 wValue=0x{:04X} wIndex=0x{:02X} len=3 failed\n",
                (UAC_CS_SAMPLING_FREQ << 8),
                iface
            );
            Err(())
        }
    }
}

fn sync_type_str(sync_type: u8) -> &'static str {
    match sync_type & 0x3 {
        0 => "no",
        1 => "async",
        2 => "adapt",
        3 => "sync",
        _ => "unk",
    }
}

fn usage_type_str(usage_type: u8) -> &'static str {
    match usage_type & 0x3 {
        0 => "data",
        1 => "feedback",
        2 => "implfb",
        3 => "resv",
        _ => "unk",
    }
}

fn find_endpoint_attr(cfg: &[u8], target_if: u8, target_alt: u8, addr: u8) -> Option<u8> {
    let mut current_ifnum: u8 = 0;
    let mut current_alt: u8 = 0;

    let mut idx = 0usize;
    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        let ty = cfg[idx + 1];
        match ty {
            4 if len >= 9 => {
                current_ifnum = cfg[idx + 2];
                current_alt = cfg[idx + 3];
            }
            5 if len >= 4 => {
                if current_ifnum == target_if && current_alt == target_alt && cfg[idx + 2] == addr {
                    return Some(cfg[idx + 3]);
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

    let demo = trueos_audio_assets::demo::DEMO;
    if UAC_FORMAT_LOGGED.load(Ordering::Acquire) == 0 {
        if let Some(fmt) = parse_as_format_info(cfg, as_out.interface, as_out.alt_setting) {
            crate::log!("usb: uac format table\n");
            crate::log!("usb: uac fmt | if | alt | ch | bits | subfrm | rates\n");
            crate::log!(
                "usb: uac fmt | {:>2} | {:>3} | {:>2} | {:>4} | {:>6} | {}\n",
                as_out.interface,
                as_out.alt_setting,
                fmt.channels,
                fmt.bit_resolution,
                fmt.subframe_bytes,
                if fmt.rates.is_empty() {
                    if fmt.rate_min != 0 && fmt.rate_max != 0 {
                        "range"
                    } else {
                        "unknown"
                    }
                } else {
                    "discrete"
                }
            );
            crate::log!("usb: uac ep  | addr | sync  | usage  | bSynch\n");
            crate::log!(
                "usb: uac ep  | 0x{:02X} | {:<5} | {:<6} | 0x{:02X}\n",
                as_out.ep_addr,
                sync_type_str(as_out.sync_type),
                usage_type_str(as_out.usage_type),
                as_out.synch_address
            );
            if !fmt.rates.is_empty() {
                for rate in fmt.rates.iter() {
                    crate::log!("usb: uac rate | {} Hz\n", rate);
                }
            } else if fmt.rate_min != 0 && fmt.rate_max != 0 {
                crate::log!("usb: uac rate range: {}..{} Hz\n", fmt.rate_min, fmt.rate_max);
            }
            UAC_FORMAT_LOGGED.store(1, Ordering::Release);
        }
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

    // If the endpoint is asynchronous, a feedback mechanism is required to avoid drift.
    // We don't implement feedback yet; fail early with an explicit message.
    if as_out.sync_type == 1 {
        if as_out.synch_address == 0 {
            crate::log!(
                "usb: uac: async isoch OUT needs feedback endpoint; bSynchAddress=0 (implicit feedback not implemented)\n"
            );
            return Err(());
        }
        let Some(fb_attr) =
            find_endpoint_attr(cfg, as_out.interface, as_out.alt_setting, as_out.synch_address)
        else {
            crate::log!(
                "usb: uac: async isoch OUT needs feedback endpoint; bSynchAddress=0x{:02X} not found\n",
                as_out.synch_address
            );
            return Err(());
        };
        let fb_xfer_ty = fb_attr & 0x3;
        let fb_sync_type = (fb_attr >> 2) & 0x3;
        let fb_usage_type = (fb_attr >> 4) & 0x3;
        if (as_out.synch_address & 0x80) == 0
            || fb_xfer_ty != 0x01
            || fb_usage_type != 1
            || fb_sync_type != 0
        {
            crate::log!(
                "usb: uac: feedback ep 0x{:02X} has unexpected bmAttributes=0x{:02X} (need IN isoch usage=feedback)\n",
                as_out.synch_address,
                fb_attr
            );
            return Err(());
        }
        crate::log!(
            "usb: uac: async feedback endpoint detected ep=0x{:02X}\n",
            as_out.synch_address
        );
    }

    // NOTE: We intentionally do not attempt UAC1 sampling-frequency control (SET_CUR/GET_CUR).
    // Many headsets advertise multiple discrete rates but still STALL these requests, and on
    // some devices/controllers this can wedge our early bringup. We instead stream at a stable
    // cadence (48kHz when the endpoint matches the common 192B@1ms path) and resample the demo
    // in software.

    // Configure isoch endpoint.
    //
    // Many UAC1 headsets advertise multiple discrete rates but still reject
    // SET_CUR/GET_CUR for sampling frequency (STALL). Empirically, a lot of
    // full-speed devices run their 2ch/16-bit/1ms AudioStreaming path at 48kHz.
    //
    // If our endpoint looks like that path (mps=192, interval=1), stream at 48kHz
    // and resample the demo in the streaming loop.
    let stream_rate_hz = if speed_code == 1
        && as_out.interval == 1
        && as_out.max_packet == 192
        && demo.channels == 2
    {
        48_000u32
    } else {
        demo.sample_rate_hz
    };

    if stream_rate_hz != demo.sample_rate_hz {
        crate::log!(
            "usb: uac: resample demo {} Hz -> {} Hz\n",
            demo.sample_rate_hz,
            stream_rate_hz
        );
    }

    let mut sink = UacSink::new(PcmFormat {
        rate_hz: stream_rate_hz,
        channels: demo.channels as u8,
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

    let mut pipe = sink.pipe.take().ok_or(())?;

    // Pre-allocate DMA buffers sized to the endpoint's packet size.
    // These are recycled via Transfer Events (one buffer per isoch TRB).
    let mut free_bufs: Vec<DmaBuf, ISOCH_BUF_SLOTS> = Vec::new();
    for _ in 0..free_bufs.capacity() {
        let (phys, virt) = dma::alloc(pipe.max_packet as usize, 64).ok_or(())?;
        unsafe { write_bytes(virt, 0, pipe.max_packet as usize) };
        let _ = free_bufs.push(DmaBuf { phys, virt });
    }

    let pipe_target = pipe.ep_target;
    *UAC_RUNTIME.lock() = Some(UacRuntime {
        ctx: *ctx,
        slot_id,
        pipe_target,
        pipe,
        free_bufs,
        inflight_bufs: [None; ISOCH_BUF_SLOTS],
        td_lens: [0; ISOCH_BUF_SLOTS],
        sample_idx: 0,
        in_flight: 0,
        src_frame: 0,
        src_frac160: 0,
        src_rate_hz: demo.sample_rate_hz,
        rate_hz: sink.fmt.rate_hz,
        channels: sink.fmt.channels as u16,
        bits_per_sample: sink.fmt.bits_per_sample,
        interval: as_out.interval,
        speed_code,
    });
    UAC_SLOT.store(slot_id, Ordering::Release);

    crate::log!(
        "usb: uac attached slot={} if={} alt={} ep=0x{:02X} mps={} interval={}\n",
        slot_id,
        as_out.interface,
        as_out.alt_setting,
        as_out.ep_addr,
        as_out.max_packet,
        as_out.interval
    );
    crate::log!(
        "usb: uac isoch schedule speed={} bInterval={} tick={}us backlog={}\n",
        speed_code,
        as_out.interval,
        if speed_code == 1 {
            core::cmp::max(1, as_out.interval as u64) * 1000
        } else {
            125u64 * (1u64 << (as_out.interval.saturating_sub(1) as u64))
        },
        32
    );

    Ok(())
}

#[embassy_executor::task]
pub async fn play_demo_task() {
    let pcm = trueos_audio_assets::demo::DEMO;
    let samples = pcm.samples_interleaved_i16;
    if samples.is_empty() {
        loop {
            Timer::after(EmbassyDuration::from_millis(1000)).await;
        }
    }

    loop {
        if UAC_SLOT.load(Ordering::Acquire) == 0 {
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        }

        // Queue-ahead: keep the isoch ring filled so occasional scheduler jitter
        // doesn't starve the endpoint.
        //
        // Important: never enqueue faster than the controller consumes, otherwise we
        // lap the transfer ring and playback turns into periodic bursts.
        // QEMU's `usb-host` backend can be severely limited in how many completed isoch
        // transfers per second it surfaces to the guest. To keep the on-wire cadence at
        // 1kHz on a full-speed endpoint, batch many 1ms packets into a single xHCI TD
        // (Chain + single IOC). The hardware still schedules each packet per interval,
        // but the guest only needs to process ~O(TDs/sec) events.
        //
        // With the current HyperX headset passthrough, we observe ~15 completion events/s,
        // so TD=64 yields ~960 packets/s (close to 1kHz).
        const TD_MAX_PACKETS: usize = 64;
        const TARGET_BACKLOG_PACKETS: usize = 128;
        const MAX_PUSH_PER_ITER: usize = 128;

        #[derive(Copy, Clone)]
        struct PendingPacket {
            buf: DmaBuf,
            bytes: usize,
        }

        // Phase 1: lock briefly to pick buffers + compute packet sizes.
        let (
            packets,
            mut sample_idx,
            mut src_frame,
            mut src_frac160,
            pipe_target,
            slot_id,
            ctx,
            tick_us,
            do_resample,
        ) =
            {
                let mut guard = UAC_RUNTIME.lock();
                match guard.as_mut() {
                    None => (
                        Vec::<PendingPacket, 32>::new(),
                        0usize,
                        0u32,
                        0u16,
                        0u32,
                        0u32,
                        None,
                        10_000u64,
                        false,
                    ),
                    Some(rt) => {
                        let max_packet_bytes = rt.pipe.max_packet as usize;
                        if rt.bits_per_sample != 16
                            || rt.free_bufs.is_empty()
                            || max_packet_bytes == 0
                        {
                            (
                                Vec::<PendingPacket, 32>::new(),
                                rt.sample_idx,
                                rt.src_frame,
                                rt.src_frac160,
                                rt.pipe_target,
                                rt.slot_id,
                                Some(rt.ctx),
                                10_000u64,
                                false,
                            )
                        } else {
                            let tick_us: u64 = if rt.speed_code == 1 {
                                core::cmp::max(1, rt.interval as u64) * 1000
                            } else {
                                125u64 * (1u64 << (rt.interval.saturating_sub(1) as u64))
                            };

                            let channels = core::cmp::max(1, rt.channels as usize);

                            // Full-speed 48kHz stereo S16LE: fixed 192B @ 1ms.
                            let fixed_packet_bytes = if rt.speed_code == 1
                                && rt.interval == 1
                                && rt.rate_hz == 48_000
                                && channels == 2
                                && rt.bits_per_sample == 16
                            {
                                Some(core::cmp::min(max_packet_bytes, 192usize))
                            } else {
                                None
                            };

                            let do_resample = fixed_packet_bytes.is_some()
                                && rt.src_rate_hz == 44_100
                                && rt.rate_hz == 48_000
                                && channels == 2
                                && RESAMPLE_TAPS > 0
                                && RESAMPLE_PHASES > 0;

                            let mut packets: Vec<PendingPacket, 32> = Vec::new();
                            let need =
                                TARGET_BACKLOG_PACKETS.saturating_sub(rt.in_flight as usize);
                            let to_push = core::cmp::min(need, MAX_PUSH_PER_ITER);
                            for _ in 0..to_push {
                                let bytes = if let Some(b) = fixed_packet_bytes {
                                    b
                                } else {
                                    let max_samples = max_packet_bytes / 2;
                                    if max_samples == 0 {
                                        break;
                                    }
                                    // Fall back to a simple "frames per tick" accumulator.
                                    // (Not used for the current fixed-packet full-speed path.)
                                    let frames = ((rt.rate_hz as u64) * tick_us) / 1_000_000u64;
                                    let mut samples_needed =
                                        (frames as usize).saturating_mul(channels);
                                    if samples_needed > max_samples {
                                        samples_needed = max_samples - (max_samples % channels);
                                    }
                                    core::cmp::min(max_packet_bytes, samples_needed * 2)
                                };
                                if bytes == 0 {
                                    break;
                                }

                                let Some(buf) = rt.free_bufs.pop() else {
                                    break;
                                };
                                let _ = packets.push(PendingPacket { buf, bytes });
                            }

                            (
                                packets,
                                rt.sample_idx,
                                rt.src_frame,
                                rt.src_frac160,
                                rt.pipe_target,
                                rt.slot_id,
                                Some(rt.ctx),
                                tick_us,
                                do_resample,
                            )
                        }
                    }
                }
            };

        // Phase 2: fill buffers without holding the global lock.
        if ctx.is_none() {
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        }
        let _ctx = ctx.unwrap();

        let channels = 2usize;
        let in_frames = samples.len() / channels;

        for pkt in packets.iter() {
            unsafe {
                let out = core::slice::from_raw_parts_mut(pkt.buf.virt, pkt.bytes);
                let mut written = 0usize;

                if do_resample && in_frames != 0 && pkt.bytes == 192 {
                    // Specialized 44.1kHz -> 48kHz (ratio 147/160) using a cheap
                    // accumulator to avoid 64-bit divisions in the hot path.
                    const STEP_NUM_160: u16 = 147;
                    const STEP_DEN_160: u16 = 160;
                    let out_frames = pkt.bytes / (2 * channels);
                    for _ in 0..out_frames {
                        let base = src_frame as i32;
                        let phase = ((src_frac160 as usize * RESAMPLE_PHASES) / (STEP_DEN_160 as usize))
                            .min(RESAMPLE_PHASES - 1);

                        // Advance fractional position for next output frame.
                        let mut next = src_frac160.wrapping_add(STEP_NUM_160);
                        if next >= STEP_DEN_160 {
                            next -= STEP_DEN_160;
                            src_frame = src_frame.wrapping_add(1);
                            if (src_frame as usize) >= in_frames {
                                src_frame = 0;
                            }
                        }
                        src_frac160 = next;

                        let frames_i32 = in_frames as i32;
                        for c in 0..channels {
                            let mut acc: i64 = 0;
                            for t in 0..RESAMPLE_TAPS {
                                let off = RESAMPLE_OFFSETS[t] as i32;
                                let mut idx_i = base + off;
                                if idx_i < 0 {
                                    idx_i += frames_i32;
                                } else if idx_i >= frames_i32 {
                                    idx_i -= frames_i32;
                                }
                                let idx = idx_i as usize;
                                let s = samples[idx * channels + c] as i64;
                                let k = RESAMPLE_COEFFS_Q15[phase][t] as i64;
                                acc += s * k;
                            }
                            let round = 1i64 << (RESAMPLE_SHIFT - 1);
                            let v = ((acc + round) >> RESAMPLE_SHIFT)
                                .clamp(i16::MIN as i64, i16::MAX as i64) as i16;
                            let le = v.to_le_bytes();
                            out[written] = le[0];
                            out[written + 1] = le[1];
                            written += 2;
                        }
                    }
                } else {
                    while written + 1 < pkt.bytes {
                        let s = samples[sample_idx];
                        sample_idx += 1;
                        if sample_idx >= samples.len() {
                            sample_idx = 0;
                        }
                        let le = s.to_le_bytes();
                        out[written] = le[0];
                        out[written + 1] = le[1];
                        written += 2;
                    }
                }
            }
        }

        // Phase 3: lock briefly to push TRBs + update indices.
        {
            let mut guard = UAC_RUNTIME.lock();
            match guard.as_mut() {
                None => 0usize,
                Some(rt) => {
                    rt.sample_idx = sample_idx;
                    rt.src_frame = src_frame;
                    rt.src_frac160 = src_frac160;

                    let mut pushed = 0usize;
                    while pushed < packets.len() {
                        let (enqueue, _cycle) = rt.pipe.ring.state_snapshot();
                        let usable = rt.pipe.ring.len.saturating_sub(1);
                        let remaining = usable.saturating_sub(enqueue);
                        if remaining == 0 {
                            break;
                        }
                        let td_len = core::cmp::min(
                            TD_MAX_PACKETS,
                            core::cmp::min(remaining, packets.len() - pushed),
                        );

                        for i in 0..td_len {
                            let idx = rt.pipe.ring.state_snapshot().0;
                            let chain = i + 1 < td_len;
                            let ioc = !chain;
                            let pkt = packets[pushed + i];
                            if !rt
                                .pipe
                                .push_isoch_trb(pkt.buf.phys, pkt.bytes as u32, chain, ioc)
                            {
                                // Return the remaining packets' buffers.
                                for pkt in packets.iter().skip(pushed + i) {
                                    let _ = rt.free_bufs.push(pkt.buf);
                                }
                                break;
                            }
                            if idx < ISOCH_BUF_SLOTS {
                                rt.inflight_bufs[idx] = Some(pkt.buf);
                                rt.td_lens[idx] = 0;
                                if ioc {
                                    rt.td_lens[idx] = td_len as u8;
                                }
                            }
                        }
                        pushed += td_len;
                    }
                    for pkt in packets.iter().skip(pushed) {
                        let _ = rt.free_bufs.push(pkt.buf);
                    }
                    if pushed > 0 {
                        rt.in_flight = rt.in_flight.saturating_add(pushed as u32);
                        UAC_PUSHED_PACKETS.fetch_add(pushed as u64, Ordering::Relaxed);
                        unsafe {
                            write_volatile(rt.ctx.doorbell.add(slot_id as usize), pipe_target);
                        }
                    }
                    pushed
                }
            }
        };

        // Yield: cadence is driven by xHCI's periodic schedule once packets are queued.
        let _ = tick_us;
        Timer::after(EmbassyDuration::from_micros(200)).await;
    }
}
