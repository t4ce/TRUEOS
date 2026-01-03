use super::xhci::{
    self, context_index, endpoint_target, hi, lo, trb_type, Trb, TrbRing, XhciContext,
};
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use embassy_time::Duration as EmbassyDuration;
use heapless::Vec;
use spin::Mutex;

const USB_CLASS_MASS_STORAGE: u8 = 0x08;
const USB_SUBCLASS_SCSI: u8 = 0x06;
const USB_PROTO_BULK_ONLY: u8 = 0x50;
const MAX_MASS_DEVICES: usize = 4;

pub struct BulkPair {
    pub configuration: u8,
    pub interface: u8,
    pub ep_in: u8,
    pub ep_out: u8,
    pub max_packet_in: u16,
    pub max_packet_out: u16,
}

pub struct MassRuntime {
    pub info: BulkPair,
    pub slot_id: u32,
    pub ep_in_target: u32,
    pub ep_out_target: u32,
    pub ring_in: TrbRing,
    pub ring_out: TrbRing,
}

unsafe impl Send for MassRuntime {}
unsafe impl Sync for MassRuntime {}

static MASS_RUNTIMES: Mutex<Vec<MassRuntime, MAX_MASS_DEVICES>> = Mutex::new(Vec::new());

pub fn register_runtime(rt: MassRuntime) {
    let mut guard = MASS_RUNTIMES.lock();
    if let Some(existing) = guard.iter_mut().find(|r| {
        r.slot_id == rt.slot_id
            && r.ep_in_target == rt.ep_in_target
            && r.ep_out_target == rt.ep_out_target
    }) {
        *existing = rt;
        return;
    }
    let _ = guard.push(rt);
}

pub fn unregister_runtime(slot_id: u32) -> bool {
    let mut guard = MASS_RUNTIMES.lock();
    let mut removed = false;
    let mut idx = 0usize;
    while idx < guard.len() {
        if guard[idx].slot_id == slot_id {
            let _ = guard.remove(idx);
            removed = true;
        } else {
            idx += 1;
        }
    }
    removed
}

pub fn has_runtime() -> bool {
    !MASS_RUNTIMES.lock().is_empty()
}

pub fn with_runtime_by_slot<R, F>(slot_id: u32, f: F) -> Option<R>
where
    F: FnOnce(&MassRuntime) -> R,
{
    MASS_RUNTIMES
        .lock()
        .iter()
        .find(|r| r.slot_id == slot_id)
        .map(f)
}

pub fn parse_mass_interface(cfg: &[u8]) -> Option<BulkPair> {
    let mut idx = 0usize;
    let mut config_value: u8 = 1;
    let mut current_iface: Option<u8> = None;
    let mut current_alt: u8 = 0;
    let mut current_subclass: u8 = 0;
    let mut current_proto: u8 = 0;
    let mut ep_in: Option<(u8, u16)> = None;
    let mut ep_out: Option<(u8, u16)> = None;

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
                if len >= 9 {
                    current_iface = Some(cfg[idx + 2]);
                    current_alt = cfg[idx + 3];
                    current_subclass = cfg[idx + 6];
                    current_proto = cfg[idx + 7];
                    ep_in = None;
                    ep_out = None;
                } else {
                    current_iface = None;
                }
            }
            5 => {
                if let Some(iface) = current_iface {
                    if current_alt == 0
                        && current_subclass == USB_SUBCLASS_SCSI
                        && current_proto == USB_PROTO_BULK_ONLY
                    {
                        if len >= 7 {
                            let ep_addr = cfg[idx + 2];
                            let attrs = cfg[idx + 3];
                            if (attrs & 0x3) == 0x2 {
                                let max_packet = u16::from_le_bytes([cfg[idx + 4], cfg[idx + 5]]);
                                if (ep_addr & 0x80) != 0 {
                                    if ep_in.is_none() {
                                        ep_in = Some((ep_addr, max_packet));
                                    }
                                } else if ep_out.is_none() {
                                    ep_out = Some((ep_addr, max_packet));
                                }

                                if let (Some((in_addr, in_mps)), Some((out_addr, out_mps))) =
                                    (ep_in, ep_out)
                                {
                                    return Some(BulkPair {
                                        configuration: config_value,
                                        interface: iface,
                                        ep_in: in_addr,
                                        ep_out: out_addr,
                                        max_packet_in: in_mps,
                                        max_packet_out: out_mps,
                                    });
                                }
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

pub async fn attach_mass_device(params: AttachParams<'_>) -> Result<(), ()> {
    let AttachParams {
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

    let Some(pair) = parse_mass_interface(cfg) else {
        return Err(());
    };

    crate::log!(
        "usb: mass storage iface={} cfg={} ep_in=0x{:02X} ep_out=0x{:02X}\n",
        pair.interface,
        pair.configuration,
        pair.ep_in,
        pair.ep_out
    );

    // Set configuration once.
    let setup_cfg = Trb {
        d0: 0x0000 | ((9u32) << 8) | ((pair.configuration as u32) << 16),
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
        crate::log!("usb: ep0 ring overflow for set_configuration (mass)\n");
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
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        crate::log!("usb: timeout waiting for set-configuration (mass)\n");
        return Err(());
    };

    let completion = (set_cfg_evt.d2 >> 24) & 0xFF;
    if completion != 1 {
        return Err(());
    }

    // Prepare endpoint rings.
    let (ring_in_phys, ring_in_virt) = match dma::alloc(64 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc bulk IN ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(ring_in_virt, 0, 64 * size_of::<Trb>()) };
    let mut ring_in = unsafe { TrbRing::new(ring_in_phys, ring_in_virt as *mut Trb, 64) };

    let (ring_out_phys, ring_out_virt) = match dma::alloc(64 * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc bulk OUT ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(ring_out_virt, 0, 64 * size_of::<Trb>()) };
    let mut ring_out = unsafe { TrbRing::new(ring_out_phys, ring_out_virt as *mut Trb, 64) };

    let (input_cfg_phys, input_cfg_virt) = match dma::alloc(4096, 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc input ctx for mass endpoints\n");
            return Err(());
        }
    };
    unsafe { write_bytes(input_cfg_virt, 0, 4096) };

    let ep_in_target = endpoint_target(pair.ep_in);
    let ep_out_target = endpoint_target(pair.ep_out);
    let ep_in_ctx_index = context_index(pair.ep_in);
    let ep_out_ctx_index = context_index(pair.ep_out);
    let highest_ep_ctx = core::cmp::max(ep_in_ctx_index, ep_out_ctx_index);

    // Add Context Flags bit index (slot=0, ep0=1, ep1out=2, ep1in=3, ...)
    let ep_in_add_bit = ep_in_ctx_index - 1;
    let ep_out_add_bit = ep_out_ctx_index - 1;

    unsafe {
        let add_flags_ptr = input_cfg_virt as *mut u32;
        write_volatile(
            add_flags_ptr.add(1),
            (1 << 0) | (1 << ep_in_add_bit) | (1 << ep_out_add_bit),
        );

        let slot_ctx = input_cfg_virt.add(ctx_stride_bytes) as *mut u32;
        let ep_in_ctx_off: usize = ctx_stride_bytes * (ep_in_ctx_index as usize);
        let ep_out_ctx_off: usize = ctx_stride_bytes * (ep_out_ctx_index as usize);
        let ep_in_ctx = input_cfg_virt.add(ep_in_ctx_off) as *mut u32;
        let ep_out_ctx = input_cfg_virt.add(ep_out_ctx_off) as *mut u32;

        let dev_slot_ctx = dev_ctx_virt as *const u32;
        for i in 0..ctx_stride_words {
            write_volatile(slot_ctx.add(i), read_volatile(dev_slot_ctx.add(i)));
        }

        let mut dw0 = read_volatile(slot_ctx.add(0));
        dw0 = (dw0 & !(0x1F << 27)) | ((highest_ep_ctx - 1) << 27);
        write_volatile(slot_ctx.add(0), dw0);

        let mut dw1 = read_volatile(slot_ctx.add(1));
        dw1 = (dw1 & !(0xFF << 16)) | ((target_port as u32) << 16);
        write_volatile(slot_ctx.add(1), dw1);

        let mps_in = (pair.max_packet_in as u32) & 0x7FF;
        let mps_out = (pair.max_packet_out as u32) & 0x7FF;

        const EP_TYPE_BULK_OUT: u32 = 2;
        const EP_TYPE_BULK_IN: u32 = 6;

        // Bulk IN endpoint
        write_volatile(ep_in_ctx.add(0), 0);
        write_volatile(
            ep_in_ctx.add(1),
            (mps_in << 16) | (EP_TYPE_BULK_IN << 3) | 3,
        );
        let dq_in = ring_in.dequeue_ptr();
        write_volatile(ep_in_ctx.add(2), lo(dq_in));
        write_volatile(ep_in_ctx.add(3), hi(dq_in));
        write_volatile(ep_in_ctx.add(4), (mps_in << 16) | mps_in);

        // Bulk OUT endpoint
        write_volatile(ep_out_ctx.add(0), 0);
        write_volatile(
            ep_out_ctx.add(1),
            (mps_out << 16) | (EP_TYPE_BULK_OUT << 3) | 3,
        );
        let dq_out = ring_out.dequeue_ptr();
        write_volatile(ep_out_ctx.add(2), lo(dq_out));
        write_volatile(ep_out_ctx.add(3), hi(dq_out));
        write_volatile(ep_out_ctx.add(4), (mps_out << 16) | mps_out);
    }

    let cfg_ep_cmd = Trb {
        d0: lo(input_cfg_phys),
        d1: hi(input_cfg_phys),
        d2: 0,
        d3: trb_type(12) | (slot_id << 24),
    };
    if !cmd_ring.push(cfg_ep_cmd) {
        crate::log!("usb: cmd ring full before configure-endpoint (mass)\n");
        return Err(());
    }
    unsafe { write_volatile(ctx.doorbell.add(0), 0) };

    let Some(cfg_evt) = xhci::wait_for_event(
        |evt| {
            let evt_type = (evt.d3 >> 10) & 0x3F;
            evt_type == 33
        },
        400,
        EmbassyDuration::from_millis(5),
    )
    .await
    else {
        crate::log!("usb: timeout waiting for configure-endpoint (mass)\n");
        return Err(());
    };

    let completion_cfg = (cfg_evt.d2 >> 24) & 0xFF;
    if completion_cfg != 1 {
        return Err(());
    }

    register_runtime(MassRuntime {
        info: pair,
        slot_id,
        ep_in_target,
        ep_out_target,
        ring_in,
        ring_out,
    });

    Ok(())
}
