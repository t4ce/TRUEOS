use super::hid;
use super::xhci::{
    self, context_index, endpoint_target, ep_avg_trb_len_bits, ep_cerr_bits, ep_interval_bits,
    ep_max_esit_payload_lo_bits, ep_max_packet_bits, ep_state_bits, ep_type_bits, hi, lo,
    trb_type, Trb, TrbRing, XhciContext, EP_STATE_DISABLED, EP_TYPE_BULK_IN, EP_TYPE_BULK_OUT,
    EP_TYPE_INT_IN, EP_TYPE_INT_OUT,
};
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;
use embassy_time::Timer;
use heapless::Vec;
use spin::Mutex;

const LED_VID: u16 = 0x0416;
const LED_PID: u16 = 0xA125;

#[derive(Copy, Clone, Debug)]
struct LedIfaceInfo {
    configuration: u8,
    interface: u8,
    protocol: u8,
    report_desc_len: u16,
    ep_in: Option<(u8, u16, u8, u32)>,  // (addr, mps, interval, xhci_ep_type)
    ep_out: Option<(u8, u16, u8, u32)>, // (addr, mps, interval, xhci_ep_type)
}

pub struct LedRuntime {
    pub controller_id: usize,
    pub slot_id: u32,
    pub out_report_id: u8,
    pub out_report_total_len: u16,
    pub ep_out_target: u32,
    pub ep_out_ring: TrbRing,
    pub out_ring_virt: *mut u8,
    pub out_ring_bytes: usize,
}

unsafe impl Send for LedRuntime {}
unsafe impl Sync for LedRuntime {}

const MAX_LED_DEVICES: usize = 4;
static LED_RUNTIMES: Mutex<Vec<LedRuntime, MAX_LED_DEVICES>> = Mutex::new(Vec::new());

pub fn unregister_runtime(controller_id: usize, slot_id: u32) -> bool {
    let mut guard = LED_RUNTIMES.lock();
    let mut removed = false;
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].controller_id == controller_id && guard[idx].slot_id == slot_id {
            let rt = guard.remove(idx);
            dma::dealloc(rt.out_ring_virt, rt.out_ring_bytes);
            removed = true;
        } else {
            idx += 1;
        }
    }
    removed
}

pub fn is_online() -> bool {
    !LED_RUNTIMES.lock().is_empty()
}

fn first_runtime_key() -> Option<(usize, u32)> {
    let guard = LED_RUNTIMES.lock();
    guard.first().map(|rt| (rt.controller_id, rt.slot_id))
}

/// Send a single HID OUT report to the first attached LED controller.
///
/// This is intentionally small and driver-like: higher layers decide policy, rate,
/// effects, and multiplexing.
pub async fn send_output_report_first(report_id: u8, payload: &[u8]) -> Result<(), ()> {
    let Some((controller_id, slot_id)) = first_runtime_key() else {
        return Err(());
    };
    send_output_report(controller_id, slot_id, report_id, payload).await
}

/// Send an OUT report using the device's preferred Report ID and expected total length
/// (derived from its HID report descriptor), padding with zeros as needed.
pub async fn send_preferred_output_report_first(payload: &[u8]) -> Result<(), ()> {
    let Some((controller_id, slot_id)) = first_runtime_key() else {
        return Err(());
    };

    let (preferred_id, preferred_total_len) = {
        let guard = LED_RUNTIMES.lock();
        let Some(rt) = guard
            .iter()
            .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
        else {
            return Err(());
        };
        (rt.out_report_id, rt.out_report_total_len)
    };

    if preferred_total_len == 0 {
        return send_output_report(controller_id, slot_id, preferred_id, payload).await;
    }

    let mut padded: Vec<u8, 64> = Vec::new();
    let want_total = core::cmp::min(preferred_total_len as usize, 64);
    let want_payload = want_total.saturating_sub((preferred_id != 0) as usize);
    let take = core::cmp::min(payload.len(), want_payload);
    let _ = padded.extend_from_slice(&payload[..take]);
    while padded.len() < want_payload {
        let _ = padded.push(0);
    }
    send_output_report(controller_id, slot_id, preferred_id, &padded).await
}

async fn send_output_report(
    controller_id: usize,
    slot_id: u32,
    report_id: u8,
    payload: &[u8],
) -> Result<(), ()> {
    // Resolve controller MMIO base.
    let mut info_opt = None;
    for info in crate::usb::xhci::xhc_list().iter().copied() {
        if info.controller_id == controller_id {
            info_opt = Some(info);
            break;
        }
    }
    let Some(info) = info_opt else {
        return Err(());
    };

    let ctx = unsafe { XhciContext::new(info) };

    // Take a snapshot of the OUT ring state and endpoint target/address.
    let (ep_out_target, mut ring_state, pad_total_len) = {
        let mut guard = LED_RUNTIMES.lock();
        let Some(rt) = guard
            .iter_mut()
            .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
        else {
            return Err(());
        };
        (rt.ep_out_target, rt.ep_out_ring.snapshot(), rt.out_report_total_len)
    };

    // Build report bytes. For report_id=0, many devices expect no ID byte; for non-zero,
    // prefix with the ID.
    let mut report: Vec<u8, 64> = Vec::new();
    if report_id != 0 {
        let _ = report.push(report_id);
    }
    // If we know the device's expected total report length, pad/truncate to that.
    let target_total = if pad_total_len != 0 {
        core::cmp::min(pad_total_len as usize, 64)
    } else {
        0
    };
    let target_payload = if target_total != 0 {
        target_total.saturating_sub(report.len())
    } else {
        64usize.saturating_sub(report.len())
    };
    let n = core::cmp::min(payload.len(), target_payload);
    let _ = report.extend_from_slice(&payload[..n]);
    if target_total != 0 {
        while report.len() < target_total {
            let _ = report.push(0);
        }
    }

    // Allocate DMA for the report payload.
    let (buf_phys, buf_virt) = dma::alloc(report.len().max(1), 64).ok_or(())?;
    unsafe {
        write_bytes(buf_virt, 0, report.len().max(1));
        if !report.is_empty() {
            core::ptr::copy_nonoverlapping(report.as_ptr(), buf_virt, report.len());
        }
    }

    // Submit a Normal TRB on the interrupt OUT endpoint.
    let mut ring = unsafe { TrbRing::from_state(ring_state) };
    let trb = Trb {
        d0: lo(buf_phys),
        d1: hi(buf_phys),
        d2: report.len() as u32,
        d3: trb_type(1) | (1 << 5), // Normal TRB, IOC
    };
    let Some(trb_phys) = ring.push_with_phys(trb) else {
        dma::dealloc(buf_virt, report.len().max(1));
        return Err(());
    };

    // Persist updated ring state.
    ring_state = ring.snapshot();
    {
        let mut guard = LED_RUNTIMES.lock();
        if let Some(rt) = guard
            .iter_mut()
            .find(|r| r.controller_id == controller_id && r.slot_id == slot_id)
        {
            rt.ep_out_ring = unsafe { TrbRing::from_state(ring_state) };
        }
    }

    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_out_target) };

    let evt = xhci::wait_for_event(
        &ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            if evt_slot != slot_id {
                return false;
            }
            let evt_ep_target = (evt.d3 >> 16) & 0x1F;
            if evt_ep_target != ep_out_target {
                return false;
            }
            let evt_ptr = (evt.d0 as u64) | ((evt.d1 as u64) << 32);
            (evt_ptr & !0xFu64) == (trb_phys & !0xFu64)
        },
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    .ok_or(())?;

    let cc = (evt.d2 >> 24) & 0xFF;
    dma::dealloc(buf_virt, report.len().max(1));
    if cc == 1 {
        Ok(())
    } else {
        Err(())
    }
}

fn parse_led_hid_iface(cfg: &[u8]) -> Option<LedIfaceInfo> {
    let mut idx = 0usize;
    let mut config_value = 1u8;

    // Track one interface at a time, and keep the best candidate seen.
    // This device may be HID class or vendor-specific; we mainly require an interrupt OUT.
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_class: u8 = 0;
    let mut current_proto: u8 = 0;
    let mut current_report_len: u16 = 0;
    let mut current_ep_in: Option<(u8, u16, u8, u32)> = None;
    let mut current_ep_out: Option<(u8, u16, u8, u32)> = None;

    let mut best: Option<LedIfaceInfo> = None;

    fn score(class: u8, report_len: u16) -> u8 {
        ((class == 0x03) as u8) * 2 + ((report_len != 0) as u8)
    }

    fn consider_current(
        best: &mut Option<LedIfaceInfo>,
        config_value: u8,
        iface: Option<u8>,
        alt: u8,
        class: u8,
        proto: u8,
        report_len: u16,
        ep_in: Option<(u8, u16, u8, u32)>,
        ep_out: Option<(u8, u16, u8, u32)>,
    ) {
        let Some(iface) = iface else {
            return;
        };
        if alt != 0 {
            return;
        }
        if ep_out.is_none() {
            return;
        }

        let candidate = LedIfaceInfo {
            configuration: config_value,
            interface: iface,
            protocol: proto,
            report_desc_len: report_len,
            ep_in,
            ep_out,
        };

        match best {
            None => *best = Some(candidate),
            Some(existing) => {
                if score(class, report_len) > score(class, existing.report_desc_len) {
                    *best = Some(candidate);
                }
            }
        }
    }

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }

        match ty {
            0x02 => {
                // Configuration
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            0x04 => {
                // Interface
                // Finalize previous interface before switching.
                consider_current(
                    &mut best,
                    config_value,
                    current_iface,
                    current_alt,
                    current_class,
                    current_proto,
                    current_report_len,
                    current_ep_in,
                    current_ep_out,
                );
                if len >= 9 {
                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_class = cfg[idx + 5];
                    current_proto = cfg[idx + 7];
                    current_report_len = 0;
                    current_ep_in = None;
                    current_ep_out = None;
                } else {
                    current_iface = None;
                }
            }
            0x21 => {
                // HID descriptor
                if len >= 9 {
                    current_report_len = u16::from_le_bytes([cfg[idx + 7], cfg[idx + 8]]);
                }
            }
            0x05 => {
                if current_iface.is_some() && current_alt == 0 && len >= 7 {
                    let ep_addr = cfg[idx + 2];
                    let attrs = cfg[idx + 3];
                    let mps = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                    let interval = cfg[idx + 6];
                    // Interrupt or Bulk endpoints.
                    let xfer = attrs & 0x3;
                    let xhci_ep_type = match (xfer, (ep_addr & 0x80) != 0) {
                        (0x3, true) => Some(EP_TYPE_INT_IN),
                        (0x3, false) => Some(EP_TYPE_INT_OUT),
                        (0x2, true) => Some(EP_TYPE_BULK_IN),
                        (0x2, false) => Some(EP_TYPE_BULK_OUT),
                        _ => None,
                    };

                    if let Some(ep_type) = xhci_ep_type {
                        if (ep_addr & 0x80) != 0 {
                            current_ep_in.get_or_insert((ep_addr, mps, interval, ep_type));
                        } else {
                            current_ep_out.get_or_insert((ep_addr, mps, interval, ep_type));
                        }
                    }
                }
            }
            _ => {}
        }
        idx += len;
    }

    // Consider the last interface in the descriptor.
    consider_current(
        &mut best,
        config_value,
        current_iface,
        current_alt,
        current_class,
        current_proto,
        current_report_len,
        current_ep_in,
        current_ep_out,
    );
    best
}

async fn set_configuration(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    config_value: u8,
) -> Result<(), ()> {
    // SET_CONFIGURATION (bRequest=9)
    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((config_value as u32) << 16),
        d1: 0,
        d2: 8,
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5) | (1 << 16),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let Some(evt) = xhci::wait_for_event(
        ctx,
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            if evt_type != 32 {
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
    if completion == 1 {
        Ok(())
    } else {
        Err(())
    }
}

async fn configure_endpoint(
    ctx: &XhciContext,
    cmd_ring: &mut TrbRing,
    slot_id: u32,
    dev_ctx_virt: *mut u8,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    speed_code: u32,
    target_port: u8,
    ep_addr: u8,
    ep_max_packet: u16,
    ep_interval: u8,
    ep_type: u32,
) -> Option<(u32, TrbRing, *mut u8, usize)> {
    let trbs = 32usize;
    let ring_bytes = trbs * size_of::<Trb>();
    let (ep_ring_phys, ep_ring_virt) = dma::alloc(ring_bytes, 64)?;
    unsafe { write_bytes(ep_ring_virt, 0, ring_bytes) };
    let ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, trbs) };

    let (input_cfg_phys, input_cfg_virt) = dma::alloc(4096, 64)?;
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_target = endpoint_target(ep_addr);
    let ep_ctx_index = context_index(ep_addr);
    let ep_add_bit = ep_ctx_index - 1;

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_add_bit));

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | (ep_add_bit << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        let mps = (ep_max_packet as u32) & 0x7FF;
        // xHCI Interval is meaningful for interrupt/isoch; for bulk it should be 0.
        let interval = if ep_type == EP_TYPE_BULK_IN || ep_type == EP_TYPE_BULK_OUT {
            0u32
        } else if speed_code == 3 {
            core::cmp::min(15u32, ep_interval.saturating_sub(1) as u32)
        } else {
            ep_interval as u32
        };

        write_volatile(
            ep_ctx.add(0),
            ep_state_bits(EP_STATE_DISABLED) | ep_interval_bits(interval),
        );
        let mut ep_cfg = ep_cerr_bits(3);
        ep_cfg |= ep_type_bits(ep_type);
        ep_cfg |= ep_max_packet_bits(mps);
        write_volatile(ep_ctx.add(1), ep_cfg);
        let dq = ep_ring.dequeue_ptr();
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));
        write_volatile(
            ep_ctx.add(4),
            ep_avg_trb_len_bits(mps) | ep_max_esit_payload_lo_bits(mps),
        );
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    let res = xhci::submit_cmd_and_wait(
        ctx,
        cmd_ring,
        cfg_ep_cmd,
        Some(slot_id),
        "leds-config-ep",
        400,
        EmbassyDuration::from_millis(5),
    )
    .await;

    dma::dealloc(input_cfg_virt, 4096);
    if res.is_err() {
        dma::dealloc(ep_ring_virt, ring_bytes);
        return None;
    }

    Some((ep_target, ep_ring, ep_ring_virt, ring_bytes))
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
    pub dev_vid: u16,
    pub dev_pid: u16,
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
        dev_vid,
        dev_pid,
    } = params;

    if dev_vid != LED_VID || dev_pid != LED_PID {
        return Err(());
    }

    crate::log!(
        "usb: leds: attach start port={} slot={} cfg_len={}\n",
        target_port,
        slot_id,
        cfg.len()
    );

    let Some(info) = parse_led_hid_iface(cfg) else {
        crate::log!(
            "usb: leds: {}:{} no HID interface found (cfg_len={})\n",
            dev_vid,
            dev_pid,
            cfg.len()
        );
        return Err(());
    };

    crate::log!(
        "usb: leds: probing port={} slot={} iface={} cfg={} proto={} rep_len={} in={:?} out={:?}\n",
        target_port,
        slot_id,
        info.interface,
        info.configuration,
        info.protocol,
        info.report_desc_len,
        info.ep_in,
        info.ep_out
    );

    set_configuration(ctx, ep0_ring, slot_id, info.configuration).await?;
    // Some devices need a small settle time after configuration before responding
    // to HID control reads.
    Timer::after(EmbassyDuration::from_millis(10)).await;

    let mut preferred_out_report_id: u8 = 0;
    let mut preferred_out_total_len: u16 = 0;

    if info.report_desc_len > 0 {
        let full_len = info.report_desc_len as usize;
        match hid::fetch_report_descriptor(ctx, ep0_ring, slot_id, info.interface, full_len).await {
            Ok(desc) => {
                hid::log_report_descriptor(slot_id, info.interface, &desc);

                if let Some(fmt) = hid::parse_output_report_format(&desc) {
                    preferred_out_report_id = fmt.report_id;
                    preferred_out_total_len = fmt.total_len_bytes;
                    crate::log!(
                        "usb: leds: output report format slot={} iface={} rid={} total_len={}\n",
                        slot_id,
                        info.interface,
                        preferred_out_report_id,
                        preferred_out_total_len
                    );
                }
            }
            Err(_err) => {
                // crate::log!(
                //     "usb: leds: report-desc fetch failed iface={} len={} err={:?}\n",
                //     info.interface,
                //     info.report_desc_len,
                //     err
                // );

                let mut recovered = false;
                let short_len = core::cmp::min(full_len, 64);
                if short_len < full_len {
                    match hid::fetch_report_descriptor(
                        ctx,
                        ep0_ring,
                        slot_id,
                        info.interface,
                        short_len,
                    )
                    .await
                    {
                        Ok(desc) => {
                            crate::log!(
                                "usb: leds: report-desc fallback short len={} iface={}\n",
                                short_len,
                                info.interface
                            );
                            hid::log_report_descriptor(slot_id, info.interface, &desc);
                            recovered = true;
                        }
                        Err(_err2) => {
                            // crate::log!(
                            //     "usb: leds: report-desc fallback short failed iface={} len={} err={:?}\n",
                            //     info.interface,
                            //     short_len,
                            //     err2
                            // );
                        }
                    }
                }

                if !recovered && info.interface != 0 {
                    match hid::fetch_report_descriptor(ctx, ep0_ring, slot_id, 0, full_len).await {
                        Ok(desc) => {
                            crate::log!(
                                "usb: leds: report-desc fallback iface=0 len={}\n",
                                full_len
                            );
                            hid::log_report_descriptor(slot_id, 0, &desc);
                            recovered = true;
                        }
                        Err(_err2) => {
                            // crate::log!(
                            //     "usb: leds: report-desc fallback iface=0 failed len={} err={:?}\n",
                            //     full_len,
                            //     err2
                            // );
                        }
                    }
                }

                if !recovered {
                    match hid::fetch_report_descriptor_device(ctx, ep0_ring, slot_id, full_len).await {
                        Ok(desc) => {
                            crate::log!(
                                "usb: leds: report-desc fallback device len={}\n",
                                full_len
                            );
                            hid::log_report_descriptor(slot_id, 0, &desc);
                            recovered = true;
                        }
                        Err(_err2) => {
                            // crate::log!(
                            //     "usb: leds: report-desc fallback device failed len={} err={:?}\n",
                            //     full_len,
                            //     err2
                            // );
                        }
                    }
                }

                if !recovered {
                    let get_report_len = core::cmp::min(full_len, 64);
                    match hid::fetch_hid_get_report(
                        ctx,
                        ep0_ring,
                        slot_id,
                        info.interface,
                        3,
                        0,
                        get_report_len,
                    )
                    .await
                    {
                        Ok(desc) => {
                            crate::log!(
                                "usb: leds: get-report feature iface={} len={}\n",
                                info.interface,
                                get_report_len
                            );
                            hid::log_report_descriptor(slot_id, info.interface, &desc);
                        }
                        Err(_err2) => {
                            // crate::log!(
                            //     "usb: leds: get-report feature failed iface={} len={} err={:?}\n",
                            //     info.interface,
                            //     get_report_len,
                            //     err2
                            // );
                        }
                    }
                }
            }
        }
    } else {
        crate::log!("usb: leds: report-desc len=0 iface={}\n", info.interface);
    }

    // Configure OUT first (we expect control of LEDs via OUT reports).
    let Some((ep_out_addr, ep_out_mps, ep_out_interval, ep_out_type)) = info.ep_out else {
        crate::log!(
            "usb: leds: no interrupt OUT endpoint (port={} slot={} iface={})\n",
            target_port,
            slot_id,
            info.interface
        );
        return Err(());
    };

    let Some((ep_out_target, ep_out_ring, out_ring_virt, out_ring_bytes)) = configure_endpoint(
            ctx,
            cmd_ring,
            slot_id,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            speed_code,
            target_port,
            ep_out_addr,
            ep_out_mps,
            ep_out_interval,
            ep_out_type,
        )
        .await
    else {
        crate::log!("usb: leds: config OUT endpoint failed\n");
        return Err(());
    };

    // Optional IN endpoint (some devices send status/acks).
    if let Some((ep_in_addr, ep_in_mps, ep_in_interval, ep_in_type)) = info.ep_in {
        let _ = configure_endpoint(
            ctx,
            cmd_ring,
            slot_id,
            dev_ctx_virt,
            ctx_stride_bytes,
            ctx_stride_words,
            speed_code,
            target_port,
            ep_in_addr,
            ep_in_mps,
            ep_in_interval,
            ep_in_type,
        )
        .await;
    }

    {
        let mut guard = LED_RUNTIMES.lock();
        let _ = guard.push(LedRuntime {
            controller_id: ctx.controller_id,
            slot_id,
            out_report_id: preferred_out_report_id,
            out_report_total_len: preferred_out_total_len,
            ep_out_target: ep_out_target as u32,
            ep_out_ring,
            out_ring_virt,
            out_ring_bytes,
        });
    }

    crate::log!(
        "usb: leds: attached slot={} port={} ep_out=0x{:02X} target={}\n",
        slot_id,
        target_port,
        ep_out_addr,
        ep_out_target
    );

    Ok(())
}
