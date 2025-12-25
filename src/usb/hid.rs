use crate::debugconf;
use crate::pci::{dma, xhci};
use crate::pci::xhci::{Trb, TrbRing, XhciContext, context_index, endpoint_target, hi, lo, trb_type};
use crate::usb::input;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use spin::Mutex;
use embassy_time::Duration as EmbassyDuration;
use heapless::Vec;

pub struct HidEpInfo {
    pub configuration: u8,
    pub interface: u8,
    pub address: u8,
    pub max_packet: u16,
    pub interval: u8,
    pub protocol: u8,
}

pub struct HidRuntime {
    pub ep: HidEpInfo,
    pub report_phys: u64,
    pub report_virt: *mut u8,
    pub report_len: u32,
    pub hid_kind: u8,
    pub slot_id: u32,
    pub ep_target: u32,
    pub ep_ring: TrbRing,
    pub seq: u64,
    pub last_nonzero_seq: u64,
}

unsafe impl Send for HidRuntime {}
unsafe impl Sync for HidRuntime {}

const MAX_HID_DEVICES: usize = 4;
static HID_RUNTIMES: Mutex<Vec<HidRuntime, MAX_HID_DEVICES>> = Mutex::new(Vec::new());
pub fn hid_kind_from_protocol(protocol: u8) -> u8 {
    // Placeholder: higher-level input stack not wired in yet.
    protocol
}

pub fn register_runtime(runtime: HidRuntime) {
    let mut guard = HID_RUNTIMES.lock();
    if let Some(existing) = guard.iter_mut().find(|r| r.slot_id == runtime.slot_id) {
        *existing = runtime;
        return;
    }
    let _ = guard.push(runtime);
}

pub fn has_runtime() -> bool {
    !HID_RUNTIMES.lock().is_empty()
}

pub fn with_runtime_mut_by_slot<F, R>(slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&mut HidRuntime) -> R,
{
    let mut guard = HID_RUNTIMES.lock();
    guard.iter_mut().find(|r| r.slot_id == slot_id).map(f)
}

pub fn debug_dump_hid_state() {
    let guard = HID_RUNTIMES.lock();
    if guard.is_empty() {
        debugconf!("[hid] runtimes: none\n");
        return;
    }

    debugconf!("[hid] runtimes: {}\n", guard.len());
    for r in guard.iter() {
        let (enq, cyc) = r.ep_ring.state_snapshot();
        debugconf!(
            "[hid] slot={} ep=0x{:02X} proto={} seq={} last_nonzero={} ring_enq={} ring_cyc={}\n",
            r.slot_id,
            r.ep.address,
            r.hid_kind,
            r.seq,
            r.last_nonzero_seq,
            enq,
            cyc as u8
        );
    }
}

pub fn handle_report(runtime: &mut HidRuntime, completion: u32, data: &[u8], residual: u32) {
    runtime.seq = runtime.seq.wrapping_add(1);

    debugconf!(
        "[hid] interrupt IN slot={} cc={} rem={} len={} ep=0x{:02X} proto={} seq={} phys=0x{:08X} data={:02X?}\n",
        runtime.slot_id,
        completion,
        residual,
        data.len(),
        runtime.ep.address,
        runtime.hid_kind,
        runtime.seq,
        lo(runtime.report_phys),
        data
    );

    if runtime.hid_kind == 2 {
        // Boot mouse: buttons, dx, dy, wheel (optional).
        if data.len() >= 3 {
            let buttons = data[0];
            let dx = data[1] as i8;
            let dy = data[2] as i8;
            let wheel = if data.len() > 3 { data[3] as i8 } else { 0 };
            if buttons != 0 || dx != 0 || dy != 0 || wheel != 0 {
                runtime.last_nonzero_seq = runtime.seq;
                debugconf!(
                    "[mouse] buttons=0x{:02X} dx={} dy={} wheel={} (slot={} seq={})\n",
                    buttons,
                    dx,
                    dy,
                    wheel,
                    runtime.slot_id,
                    runtime.seq
                );
            }
            input::push_event(input::InputEvent::Mouse(input::MouseEvent { buttons, dx, dy, wheel }));
        }
    } else if runtime.hid_kind == 1 {
        // Boot keyboard: modifiers + 6 keycodes
        if data.len() >= 8 {
            let modifiers = data[0];
            let mut keys = [0u8; 6];
            keys.copy_from_slice(&data[2..8]);
            if keys.iter().any(|&k| k != 0) || modifiers != 0 {
                runtime.last_nonzero_seq = runtime.seq;
                debugconf!(
                    "[kbd] mods=0x{:02X} keys={:02X} {:02X} {:02X} {:02X} {:02X} {:02X}\n",
                    modifiers,
                    keys[0],
                    keys[1],
                    keys[2],
                    keys[3],
                    keys[4],
                    keys[5]
                );
            }
            input::push_event(input::InputEvent::Keyboard(input::KeyboardEvent { modifiers, keys }));
        }
    }
}

// NOTE: No synthetic HID injections; reports now only reflect real device data.

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
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
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
    // Input context array index (slot=1, ep0=2, ep1out=3, ep1in=4, ...)
    let ep_ctx_index = context_index(ep.address);
    // Add Context Flags bit index (slot=0, ep0=1, ep1out=2, ep1in=3, ...)
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
        // Context Entries = highest valid endpoint context index in *device* context
        // (slot=0, ep0=1, ep1out=2, ep1in=3, ...), which corresponds to (ep_ctx_index - 1).
        dw0 = (dw0 & !(0x1F << 27)) | (ep_add_bit << 27);
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
        // CErr is the low 2 bits of DW1.
        write_volatile(ep_ctx.add(1), (mps << 16) | (EP_TYPE_INT_IN << 3) | 3);
        // Set dequeue pointer with the current ring cycle bit (DCS) set.
        // Using the raw phys address would leave DCS cleared and the host would
        // ignore our queued transfer ring.
        let dq = ep_ring.dequeue_ptr();
        write_volatile(ep_ctx.add(2), lo(dq));
        write_volatile(ep_ctx.add(3), hi(dq));

        // Use the endpoint's packet size consistently for scheduling hints.
        let avg_trb_len = mps;
        let max_esit_payload = mps;
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

    let report_bytes = core::cmp::max(usize::from(ep.max_packet), 8);
    let (rep_phys, rep_virt) = match dma::alloc(report_bytes, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc report buffer\n");
            return Err(());
        }
    };
    unsafe { write_bytes(rep_virt, 0, report_bytes) };

    let report_len = ep.max_packet as u32;

    let normal = Trb {
        d0: lo(rep_phys),
        d1: hi(rep_phys),
        d2: report_len,
        d3: trb_type(1) | (1 << 5),
    };
    if !ep_ring.push(normal) {
        debugconf!("usb: ep ring full before interrupt IN\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), ep_target as u32) };

    register_runtime(HidRuntime {
        ep,
        report_phys: rep_phys,
        report_virt: rep_virt,
        report_len,
        hid_kind,
        slot_id,
        ep_target: ep_target as u32,
        ep_ring,
        seq: 0,
        last_nonzero_seq: 0,
    });

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
            if evt_type != 32 {
                return false;
            }
            let evt_slot = (evt.d3 >> 24) & 0xFF;
            evt_slot == slot_id
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
