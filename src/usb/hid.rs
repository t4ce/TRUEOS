use crate::{debugconf, dma, xhci};
use crate::xhci::{Trb, TrbRing, XhciContext, context_index, endpoint_target, hi, lo, trb_type};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use spin::Mutex;
use embassy_time::Duration as EmbassyDuration;

#[derive(Copy, Clone, Debug)]
pub struct HidEpInfo {
    pub configuration: u8,
    pub interface: u8,
    pub address: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub protocol: u8,
}

#[derive(Copy, Clone, Debug)]
pub struct HidRuntime {
    pub ep: HidEpInfo,
    pub report_virt: *mut u8,
    pub hid_kind: u8,
}

unsafe impl Send for HidRuntime {}
unsafe impl Sync for HidRuntime {}

static HID_RUNTIME: Mutex<Option<HidRuntime>> = Mutex::new(None);

pub fn hid_kind_from_protocol(protocol: u8) -> u8 {
    // Placeholder: higher-level input stack not wired in yet.
    protocol
}

pub fn register_runtime(runtime: HidRuntime) {
    let mut guard = HID_RUNTIME.lock();
    *guard = Some(runtime);
}

pub fn runtime() -> Option<HidRuntime> {
    *HID_RUNTIME.lock()
}

pub fn handle_report(runtime: &HidRuntime, completion: u32, data: &[u8]) {
    debugconf!(
        "[hid] interrupt IN cc={} len={} ep=0x{:02X} proto={}\n",
        completion,
        runtime.ep.max_packet,
        runtime.ep.address,
        runtime.hid_kind
    );
    if !data.is_empty() {
        let preview_len = data.len().min(8);
        for i in 0..preview_len {
            debugconf!("[hid]   data[{}]=0x{:02X}\n", i, data[i]);
        }
    }
    // TODO: route data to input subsystem when available (e.g., inbound::push_report).
}

pub fn parse_boot_endpoint(cfg: &[u8]) -> Option<HidEpInfo> {
    let mut idx = 0usize;
    let mut config_value = 1u8;
    let mut current_iface: Option<u8> = None;
    let mut current_proto: u8 = 0;
    let mut current_subclass: u8 = 0;

    while idx + 2 <= cfg.len() {
        let len = cfg[idx] as usize;
        let ty = cfg[idx + 1];
        if len == 0 || idx + len > cfg.len() {
            break;
        }
        match ty {
            2 => {
                if len >= 6 {
                    config_value = cfg[idx + 5];
                }
            }
            4 => {
                if len >= 8 {
                    current_iface = Some(cfg[idx + 2]);
                    current_subclass = cfg[idx + 6];
                    current_proto = cfg[idx + 7];
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    let class = 0x03u8;
                    let subclass = current_subclass;
                    let proto = current_proto;
                    if class == 0x03 && subclass == 0x01 && (proto == 0x01 || proto == 0x02) {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                            let interval = cfg[idx + 6];
                            if (attrs & 0x3) == 0x3 && (ep_addr & 0x80) != 0 {
                                debugconf!(
                                    "[hid] parse ep iface={} addr=0x{:02X} mps={} interval={} cfg={} subclass={} proto={}\n",
                                    iface,
                                    ep_addr,
                                    max_packet,
                                    interval,
                                    config_value,
                                    subclass,
                                    proto
                                );
                                return Some(HidEpInfo {
                                    configuration: config_value,
                                    interface: iface,
                                    address: ep_addr,
                                    max_packet,
                                    interval,
                                    protocol: proto,
                                });
                            }
                        }
                    }
                }
            }
            _ => {}
        }
        idx += len;
    }
    None
}

pub struct BootAttachParams<'a> {
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

pub async fn attach_boot_device(params: BootAttachParams<'_>) -> Result<(), ()> {
    let BootAttachParams {
        ctx,
        mut cmd_ring,
        mut ep0_ring,
        slot_id,
        cfg,
        dev_ctx_virt,
        ctx_stride_bytes,
        ctx_stride_words,
        speed_code,
        target_port,
    } = params;

    if cfg.is_empty() {
        debugconf!("[hid] empty configuration descriptor\n");
        return Err(());
    }

    let Some(ep) = parse_boot_endpoint(cfg) else {
        debugconf!("[hid] no HID boot interrupt IN endpoint found\n");
        return Err(());
    };
    debugconf!(
        "usb: hid ep addr=0x{:02X} maxpkt={} interval={} iface={} cfg={} proto={}\n",
        ep.address,
        ep.max_packet,
        ep.interval,
        ep.interface,
        ep.configuration,
        ep.protocol
    );
    let hid_kind = hid_kind_from_protocol(ep.protocol);

    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((ep.configuration as u32) << 16),
        d1: 0,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    };
    let status_cfg = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5),
    };
    if !ep0_ring.push(setup_cfg) || !ep0_ring.push(status_cfg) {
        debugconf!("usb: ep0 ring overflow for set_configuration\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };
    let Some(set_cfg_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 32
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for set-configuration\n");
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    let _ = class_request_nodata(ctx, &mut ep0_ring, slot_id, 0x0B, 0, ep.interface as u16).await;
    let _ = class_request_nodata(ctx, &mut ep0_ring, slot_id, 0x0A, 0, ep.interface as u16).await;

    let (ep_ring_phys, ep_ring_virt) = match dma::alloc(32 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ep ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(ep_ring_virt, 0, 32 * size_of::<Trb>()) };
    let mut ep_ring = unsafe { TrbRing::new(ep_ring_phys, ep_ring_virt as *mut Trb, 32) };

    let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc input ctx for cfg-ep\n");
            return Err(());
        }
    };
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_target = endpoint_target(ep.address);
    let ep_ctx_index = context_index(ep.address);

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        write_volatile(add_flags_ptr.add(1), (1 << 0) | (1 << ep_ctx_index));

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_ctx_off: usize = ctx_stride_bytes * (ep_ctx_index as usize);
        let ep_ctx = input_cfg_virt.add(ep_ctx_off) as *mut u32;

        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | ((ep_ctx_index + 1) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        const EP_TYPE_INT_IN: u32 = 7;
        let mps = (ep.max_packet as u32) & 0x7FF;
        let interval = if speed_code == 3 {
            core::cmp::min(15u32, ep.interval.saturating_sub(1) as u32)
        } else {
            ep.interval as u32
        };

        write_volatile(ep_ctx.add(0), interval << 16);
        write_volatile(ep_ctx.add(1), (mps << 16) | (EP_TYPE_INT_IN << 3) | (3 << 1));
        let dq = ep_ring.phys & !0xF;
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));
        let avg_trb_len = 8u32;
        let max_esit_payload = 4u32;
        write_volatile(ep_ctx.add(4), (avg_trb_len << 16) | max_esit_payload);
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    if !cmd_ring.push(cfg_ep_cmd) {
        debugconf!("usb: cmd ring full before configure-endpoint\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    let Some(cfg_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 33
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("usb: timeout waiting for configure-endpoint\n");
        return Err(());
    };

    let completion = (cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    let (rep_phys, rep_virt) = match dma::alloc(16, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc report buffer\n");
            return Err(());
        }
    };
    unsafe { write_bytes(rep_virt, 0, 16) };

    register_runtime(HidRuntime {
        ep,
        report_virt: rep_virt,
        hid_kind,
    });

    let normal = Trb {
        d0: lo(rep_phys),
        d1: hi(rep_phys),
        d2: 8,
        d3: trb_type(1) | (1 << 5),
    };
    if !ep_ring.push(normal) {
        debugconf!("usb: ep ring full before interrupt IN\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

    Ok(())
}

pub async fn class_request_nodata(
    ctx: &XhciContext,
    ep0_ring: &mut TrbRing,
    slot_id: u32,
    request: u8,
    value: u16,
    index: u16,
) -> Result<(), ()> {
    let setup = Trb {
        d0: (0x21u32) | ((request as u32) << 8) | ((value as u32) << 16),
        d1: index as u32,
        d2: 8 | (2 << 16),
        d3: trb_type(2) | (1 << 6),
    };
    let status = Trb {
        d0: 0,
        d1: 0,
        d2: 0,
        d3: trb_type(4) | (1 << 5),
    };
    if !ep0_ring.push(setup) || !ep0_ring.push(status) {
        debugconf!("[hid] ep0 ring overflow for class request\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

    let Some(evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 32
        },
        400,
        EmbassyDuration::from_millis(5)
    )
    .await
    else {
        debugconf!("[hid] timeout waiting for class request {}\n", request);
        return Err(());
    };

    let completion = (evt.d2 >> 24) & 0xFF;
    debugconf!("[hid] class req {} cc={} value=0x{:04X}\n", request, completion, value);
    if completion == 1 {
        Ok(())
    } else {
        Err(())
    }
}
