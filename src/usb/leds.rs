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
use heapless::Vec;
use spin::Mutex;

const LED_VID: u16 = 0x0416;
const LED_PID: u16 = 0xA125;

// Keep this fairly small; report descriptors can be large but this device's cfg_len is tiny.
const MAX_REPORT_DESC: usize = 1024;

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
    pub interface: u8,
    pub ep_out_target: u32,
    pub ep_out_addr: u8,
    pub ep_out_mps: u16,
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

async fn fetch_hid_report_descriptor(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    iface: u8,
    len: usize,
) -> Option<Vec<u8, MAX_REPORT_DESC>> {
    let want_len = core::cmp::min(len, MAX_REPORT_DESC);
    let (phys, virt) = dma::alloc(want_len, 64)?;
    unsafe { write_bytes(virt, 0, want_len) };

    // bmRequestType=0x81 (IN|Standard|Interface), bRequest=GET_DESCRIPTOR,
    // wValue=(REPORT<<8)|0, wIndex=interface.
    let setup = Trb {
        d0: (0x81u32) | ((0x06u32) << 8) | ((0x22u32) << 16),
        d1: iface as u32,
        d2: want_len as u32,
        d3: trb_type(2) | (1 << 6),
    };
    let data = Trb {
        d0: lo(phys),
        d1: hi(phys),
        d2: want_len as u32,
        d3: trb_type(3) | (1 << 16),
    };
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5),
    };

    if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
        dma::dealloc(virt, want_len);
        return None;
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
        dma::dealloc(virt, want_len);
        return None;
    };

    let completion = (evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        dma::dealloc(virt, want_len);
        return None;
    }

    let mut out = Vec::<u8, MAX_REPORT_DESC>::new();
    let data_slice = unsafe { core::slice::from_raw_parts(virt, want_len) };
    let _ = out.extend_from_slice(data_slice);
    dma::dealloc(virt, want_len);
    Some(out)
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

    if info.report_desc_len > 0 {
        if let Some(desc) =
            fetch_hid_report_descriptor(ctx, ep0_ring, slot_id, info.interface, info.report_desc_len as usize).await
        {
            let n = core::cmp::min(desc.len(), 64);
            crate::log!(
                "usb: leds: report-desc iface={} len={} head={:02X?}\n",
                info.interface,
                desc.len(),
                &desc[..n]
            );
        } else {
            crate::log!("usb: leds: report-desc fetch failed iface={}\n", info.interface);
        }
    }

    {
        let mut guard = LED_RUNTIMES.lock();
        let _ = guard.push(LedRuntime {
            controller_id: ctx.controller_id,
            slot_id,
            interface: info.interface,
            ep_out_target: ep_out_target as u32,
            ep_out_addr,
            ep_out_mps,
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
