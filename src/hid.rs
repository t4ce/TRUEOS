use crate::{debugconf, xhci};
use crate::xhci::{Trb, TrbRing, XhciContext, trb_type};
use core::ptr::write_volatile;
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
        "hid: interrupt IN cc={} len={} ep=0x{:02X} proto={}\n",
        completion,
        runtime.ep.max_packet,
        runtime.ep.address,
        runtime.hid_kind
    );
    if !data.is_empty() {
        let preview_len = data.len().min(8);
        for i in 0..preview_len {
            debugconf!("hid:   data[{}]=0x{:02X}\n", i, data[i]);
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
                                    "usb: parse hid ep iface={} addr=0x{:02X} mps={} interval={} cfg={} subclass={} proto={}\n",
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
        debugconf!("usb: ep0 ring overflow for hid class req\n");
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
        debugconf!("usb: timeout waiting for hid class req {}\n", request);
        return Err(());
    };

    let completion = (evt.d2 >> 24) & 0xFF;
    debugconf!("usb: hid class req {} cc={} value=0x{:04X}\n", request, completion, value);
    if completion == 1 {
        Ok(())
    } else {
        Err(())
    }
}
