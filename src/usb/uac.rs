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
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::Vec;
use spin::Mutex;
use core::ptr::{write_bytes, write_volatile};
use core::sync::atomic::{AtomicU32, Ordering};

const USB_CLASS_AUDIO: u8 = 0x01;
const USB_SUBCLASS_AUDIOSTREAMING: u8 = 0x02;

#[derive(Copy, Clone, Debug)]
struct AsOutEndpoint {
    configuration_value: u8,
    interface: u8,
    alt_setting: u8,
    ep_addr: u8,
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
    bufs: Vec<DmaBuf, 64>,
    buf_idx: usize,
    sample_idx: usize,
    rate_hz: u32,
    channels: u16,
    bits_per_sample: u8,
    interval: u8,
    speed_code: u32,
    phase_accum: u64,
}

unsafe impl Send for UacRuntime {}
unsafe impl Sync for UacRuntime {}

static UAC_SLOT: AtomicU32 = AtomicU32::new(0);
static UAC_RUNTIME: Mutex<Option<UacRuntime>> = Mutex::new(None);

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

pub fn handle_transfer_event(_evt: &Trb) -> bool {
    // We currently rely on time-based pacing and a large isoch ring.
    // Still consuming events in the global poll loop prevents event buffer buildup.
    UAC_RUNTIME.lock().is_some()
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

    let mut pending_ep: Option<(u8, u16, u8)> = None; // (addr, max_packet, interval)
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

                // Isoch OUT only.
                let dir_in = (ep_addr & 0x80) != 0;
                if !dir_in && xfer_ty == 0x01 {
                    pending_ep = Some((ep_addr, max_packet, interval));
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

        if let (Some((ifnum, alt, _)), Some((ep_addr, max_packet, interval))) = (current_if, pending_ep)
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
        rate_hz: 48_000,
        channels: 2,
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

    // Pre-allocate a small DMA buffer pool sized to the endpoint's packet size.
    let mut bufs: Vec<DmaBuf, 64> = Vec::new();
    for _ in 0..bufs.capacity() {
        let (phys, virt) = dma::alloc(pipe.max_packet as usize, 64).ok_or(())?;
        unsafe { write_bytes(virt, 0, pipe.max_packet as usize) };
        let _ = bufs.push(DmaBuf { phys, virt });
    }

    let pipe_target = pipe.ep_target;

    *UAC_RUNTIME.lock() = Some(UacRuntime {
        ctx: *ctx,
        slot_id,
        pipe_target,
        pipe,
        bufs,
        buf_idx: 0,
        sample_idx: 0,
        rate_hz: sink.fmt.rate_hz,
        channels: sink.fmt.channels as u16,
        bits_per_sample: sink.fmt.bits_per_sample,
        interval: as_out.interval,
        speed_code,
        phase_accum: 0,
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

    let mut last_log = Instant::now();
    let mut queued: u64 = 0;
    let mut missed: u64 = 0;
    let mut fmt_mismatch: u64 = 0;
    let mut no_runtime: u64 = 0;

    loop {
        if UAC_SLOT.load(Ordering::Acquire) == 0 {
            Timer::after(EmbassyDuration::from_millis(50)).await;
            continue;
        }

        // Stream one packet per endpoint interval.
        // Intentionally minimal: no feedback endpoints, no format negotiation.
        let (did_queue, tick_us) = {
            let mut guard = UAC_RUNTIME.lock();
            match guard.as_mut() {
                None => {
                    no_runtime = no_runtime.saturating_add(1);
                    (false, 10_000u64)
                }
                Some(rt) => {
                    let max_packet_bytes = rt.pipe.max_packet as usize;
                    if rt.bits_per_sample != 16 {
                        // Current demo stream is S16LE only.
                        fmt_mismatch = fmt_mismatch.saturating_add(1);
                        (false, 10_000u64)
                    } else {
                        let tick_us: u64 = if rt.speed_code == 1 {
                            // Full-speed: bInterval is in 1ms frames.
                            core::cmp::max(1, rt.interval as u64) * 1000
                        } else {
                            // High-/Super-speed: bInterval is 125us microframes as 2^(bInterval-1).
                            125u64 * (1u64 << (rt.interval.saturating_sub(1) as u64))
                        };

                        // Target PCM frames to send this tick.
                        rt.phase_accum = rt.phase_accum.saturating_add(rt.rate_hz as u64 * tick_us);
                        let frames = rt.phase_accum / 1_000_000u64;
                        rt.phase_accum %= 1_000_000u64;

                        let channels = core::cmp::max(1, rt.channels as usize);
                        let mut samples_needed = (frames as usize).saturating_mul(channels);
                        let max_samples = max_packet_bytes / 2;
                        if max_samples == 0 {
                            (false, tick_us)
                        } else {
                            // Keep sample count aligned to whole frames.
                            if samples_needed > max_samples {
                                samples_needed = max_samples - (max_samples % channels);
                            }
                            let packet_bytes = core::cmp::min(max_packet_bytes, samples_needed * 2);
                            if packet_bytes == 0 {
                                (false, tick_us)
                            } else {
                                let buf = rt.bufs[rt.buf_idx];
                                rt.buf_idx = (rt.buf_idx + 1) % rt.bufs.len();

                                // Fill packet with interleaved i16 PCM, little-endian.
                                let mut written = 0usize;
                                unsafe {
                                    let out = core::slice::from_raw_parts_mut(buf.virt, packet_bytes);
                                    while written + 1 < packet_bytes {
                                        let s = samples[rt.sample_idx];
                                        rt.sample_idx += 1;
                                        if rt.sample_idx >= samples.len() {
                                            rt.sample_idx = 0;
                                        }
                                        let le = s.to_le_bytes();
                                        out[written] = le[0];
                                        out[written + 1] = le[1];
                                        written += 2;
                                    }
                                }

                                if !rt.pipe.push_isoch_trb(buf.phys, packet_bytes as u32, true) {
                                    missed = missed.saturating_add(1);
                                    (false, tick_us)
                                } else {
                                    unsafe {
                                        write_volatile(
                                            rt.ctx.doorbell.add(rt.slot_id as usize),
                                            rt.pipe_target,
                                        )
                                    };
                                    queued = queued.saturating_add(1);
                                    (true, tick_us)
                                }
                            }
                        }
                    }
                }
            }
        };

        if Instant::now().duration_since(last_log) >= EmbassyDuration::from_secs(1) {
            crate::log!(
                "usb: uac demo stats queued={} missed={} fmt_mismatch={} no_runtime={}\n",
                queued,
                missed,
                fmt_mismatch,
                no_runtime
            );
            last_log = Instant::now();
            queued = 0;
            missed = 0;
            fmt_mismatch = 0;
            no_runtime = 0;
        }

        if !did_queue {
            Timer::after(EmbassyDuration::from_millis(10)).await;
        } else {
            Timer::after(EmbassyDuration::from_micros(tick_us)).await;
        }
    }
}
