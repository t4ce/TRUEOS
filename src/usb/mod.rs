pub mod hid;
pub mod pen;
pub mod print;

use crate::{debugconf, dma, osal, xhci};
use crate::xhci::{
    decode_port_status,
    write_reg64,
    trb_type,
    lo,
    hi,
    EventRing,
    Trb,
    TrbRing,
    XhciContext,
    ErstEntry,
};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

use self::hid::BootAttachParams;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum DeviceKind {
    Hid,
    Printer,
    Pen,
}
#[derive(Copy, Clone, Debug)]
struct DeviceEntry {
    slot_id: u32,
    port: u8,
    kind: DeviceKind,
}

const MAX_DEVICES: usize = 8;
static DEVICES: Mutex<Vec<DeviceEntry, MAX_DEVICES>> = Mutex::new(Vec::new());
static HID_TEST_INJECTED: AtomicBool = AtomicBool::new(false);

fn register_device(slot_id: u32, port: u8, kind: DeviceKind) {
    let mut guard = DEVICES.lock();
    if let Some(existing) = guard.iter_mut().find(|d| d.slot_id == slot_id) {
        existing.kind = kind;
        existing.port = port;
        return;
    }
    if guard.push(DeviceEntry { slot_id, port, kind }).is_err() {
        debugconf!("usb: device table full, dropping slot {}\n", slot_id);
    }
}

fn device_kind_for_slot(slot_id: u32) -> Option<DeviceKind> {
    DEVICES.lock().iter().find(|d| d.slot_id == slot_id).map(|d| d.kind)
}

fn any_hid_registered() -> bool {
    DEVICES.lock().iter().any(|d| d.kind == DeviceKind::Hid)
}

#[embassy_executor::task]
pub async fn usb_scout(info: xhci::ControllerInfo) {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
    let ctx_stride_bytes: usize = if csz_64 { 0x40 } else { 0x20 };
    let ctx_stride_words: usize = ctx_stride_bytes / 4;

    let max_slots = (ctx.hcsparams1 & 0xFF) as usize;
    let (dcbaa_phys, dcbaa_virt) = match dma::alloc((max_slots + 1) * 8, 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc dcbaa\n");
            return;
        }
    };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    const CMD_RING_TRBS: usize = 64;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc cmd ring\n");
            return;
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let mut cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 64;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc event ring\n");
            return;
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let mut event_ring = unsafe {
        EventRing::new(
            evt_phys,
            evt_virt_raw as *mut Trb,
            EVENT_RING_TRBS,
        )
    };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ERST\n");
            return;
        }
    };
    unsafe {
        write_bytes(erst_virt, 0, size_of::<ErstEntry>());
        let entry = &mut *(erst_virt as *mut ErstEntry);
        entry.seg_base_lo = lo(evt_phys);
        entry.seg_base_hi = hi(evt_phys);
        entry.seg_size = EVENT_RING_TRBS as u32;
    }

    let mut connected: Vec<(u8, u32), 16> = Vec::new();
    for port in 0..ctx.port_count {
        let status = unsafe { ctx.portsc(port as usize) };
        let (connected_flag, _, _) = decode_port_status(status);
        if connected_flag {
            let _ = connected.push(((port + 1) as u8, status));
        }
    }

    if connected.is_empty() {
        debugconf!("usb: no connected devices detected\n");
        return;
    }

    unsafe {
        write_reg64(ctx.op_base, 0x30, dcbaa_phys);
        write_volatile(ctx.op_base.add(0x38 / 4), 1);

        const IMAN: usize = 0x00 / 4;
        const ERSTSZ: usize = 0x08 / 4;
        const ERSTBA: usize = 0x10 / 4;
        let intr0 = ctx.runtime.add(0x20 / 4);
        write_volatile(intr0.add(ERSTSZ), 1);
        write_volatile(intr0.add(ERSTBA), lo(erst_phys));
        write_volatile(intr0.add(ERSTBA + 1), hi(erst_phys));
        event_ring.update_erdp(intr0);
        xhci::install_event_ring(event_ring, intr0);
        const IMAN_IE: u32 = 1 << 1;
        write_volatile(intr0.add(IMAN), IMAN_IE);

        write_reg64(ctx.op_base, 0x18, cmd_ring.crcr_value());

        const USBCMD: usize = 0x00 / 4;
        const USBSTS: usize = 0x04 / 4;
        const USBCMD_RS: u32 = 1 << 0;
        const USBCMD_INTE: u32 = 1 << 2;
        const USBSTS_HCH: u32 = 1 << 0;
        write_volatile(ctx.op_base.add(USBCMD), USBCMD_RS | USBCMD_INTE);
        let mut spin: u32 = 1_000_000;
        while spin > 0 {
            let sts = read_volatile(ctx.op_base.add(USBSTS));
            if (sts & USBSTS_HCH) == 0 {
                break;
            }
            spin -= 1;
        }
    }

    for (target_port, mut port_status) in connected.into_iter() {
        let enable_slot = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(9),
        };
        if !cmd_ring.push(enable_slot) {
            debugconf!("usb: cmd ring full before enable-slot\n");
            continue;
        }
        unsafe {
            write_volatile(ctx.doorbell.add(0), 0);
        }

        let Some(enable_evt) = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type == 33 {
                    true
                } else {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    debugconf!(
                        "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        evt_type,
                        completion,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    false
                }
            },
            400,
            EmbassyDuration::from_millis(5)
        )
        .await
        else {
            debugconf!("usb: timeout waiting for enable-slot completion (port {})\n", target_port);
            continue;
        };

        let completion = (enable_evt.d2 >> 24) & 0xFF;
        if completion != 1 {
            debugconf!(
                "usb: enable-slot failed cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                completion,
                enable_evt.d0,
                enable_evt.d1,
                enable_evt.d2,
                enable_evt.d3
            );
            continue;
        }

        let slot_id = (enable_evt.d3 >> 24) & 0xFF;
        if slot_id == 0 {
            debugconf!("usb: enable-slot returned slot 0\n");
            continue;
        }
        debugconf!("usb: enable-slot ok slot={} port={}\n", slot_id, target_port);

        let port_idx = (target_port - 1) as usize;
        const PORTSC_PED: u32 = 1 << 1;
        const PORTSC_PR: u32 = 1 << 4;
        unsafe {
            ctx.reset_port(port_idx);
        }
        let mut reset_polls = 0;
        loop {
            port_status = unsafe { ctx.portsc(port_idx) };
            let pr_clear = (port_status & PORTSC_PR) == 0;
            let ped_set = (port_status & PORTSC_PED) != 0;
            if pr_clear && ped_set {
                break;
            }
            reset_polls += 1;
            if reset_polls > 400 {
                debugconf!(
                    "usb: port {} reset timed out status=0x{:08X}\n",
                    target_port,
                    port_status
                );
                break;
            }
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
        if reset_polls > 400 {
            continue;
        }

        let speed_code = (port_status >> 10) & 0xF;
        let max_packet = match speed_code {
            2 => 8,
            1 => 8,
            3 => 64,
            4 => 512,
            _ => 8,
        } as u16;

        let (dev_ctx_phys, dev_ctx_virt) = match dma::alloc(4096, 64) {
            Some(pair) => pair,
            None => {
                debugconf!("usb: failed to alloc device context\n");
                continue;
            }
        };
        unsafe { write_bytes(dev_ctx_virt, 0, 4096) };

        let (input_ctx_phys, input_ctx_virt) = match dma::alloc(4096, 64) {
            Some(pair) => pair,
            None => {
                debugconf!("usb: failed to alloc input context\n");
                continue;
            }
        };
        unsafe { write_bytes(input_ctx_virt, 0, 4096) };

        const EP0_TRBS: usize = 32;
        let (ep0_phys, ep0_virt_raw) = match dma::alloc(EP0_TRBS * size_of::<Trb>(), 64) {
            Some(pair) => pair,
            None => {
                debugconf!("usb: failed to alloc ep0 ring\n");
                continue;
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

            const EP_TYPE_CONTROL: u32 = 4;
            let ep_type_field = EP_TYPE_CONTROL << 16;
            write_volatile(ep0_ctx.add(0), ep_type_field);
            let max_packet_field = max_packet as u32;
            write_volatile(ep0_ctx.add(1), max_packet_field);
            let dq = ep0_ring.dequeue_ptr();
            write_volatile(ep0_ctx.add(2), lo(dq));
            write_volatile(ep0_ctx.add(3), hi(dq));
            write_volatile(ep0_ctx.add(4), 8);
        }

        let addr_dev = Trb {
            d0: lo(input_ctx_phys),
            d1: hi(input_ctx_phys),
            d2: 0,
            d3: trb_type(11) | (slot_id << 24),
        };
        if !cmd_ring.push(addr_dev) {
            debugconf!("usb: cmd ring full before address-device\n");
            continue;
        }
        unsafe { write_volatile(ctx.doorbell.add(0), 0) };

        let Some(addr_evt) = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type == 33 {
                    true
                } else {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    let evt_slot = (evt.d3 >> 24) & 0xFF;
                    debugconf!(
                        "usb: unexpected event type={} cc={} slot={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        evt_type,
                        completion,
                        evt_slot,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    false
                }
            },
            400,
            EmbassyDuration::from_millis(500)
        )
        .await
        else {
            debugconf!("usb: timeout waiting for address-device port {}\n", target_port);
            continue;
        };

        let completion = (addr_evt.d2 >> 24) & 0xFF;
        let evt_slot = (addr_evt.d3 >> 24) & 0xFF;
        if completion != 1 || evt_slot != slot_id {
            debugconf!(
                "usb: address-device unexpected completion cc={} slot={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                completion,
                evt_slot,
                addr_evt.d0,
                addr_evt.d1,
                addr_evt.d2,
                addr_evt.d3
            );
            continue;
        }

        let (desc_phys, desc_virt) = match dma::alloc(64, 64) {
            Some(pair) => pair,
            None => {
                debugconf!("usb: failed to alloc desc buffer\n");
                continue;
            }
        };
        unsafe { write_bytes(desc_virt, 0, 64) };

        let setup = Trb {
            d0: 0x0680 | (0x0100 << 16),
            d1: 18 << 16,
            d2: 8 | (2 << 16),
            d3: trb_type(2) | (1 << 6),
        };

        let data = Trb {
            d0: lo(desc_phys),
            d1: hi(desc_phys),
            d2: 18,
            d3: trb_type(3) | (1 << 16) | (1 << 5),
        };

        let status = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(4) | (1 << 5),
        };

        if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
            debugconf!("usb: ep0 ring overflow for setup\n");
            continue;
        }

        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

        let Some(_desc_evt) = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type == 32 {
                    true
                } else {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    debugconf!(
                        "usb: unexpected event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        evt_type,
                        completion,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    false
                }
            },
            800,
            EmbassyDuration::from_millis(5)
        )
        .await
        else {
            debugconf!("usb: timeout waiting for transfer event port {}\n", target_port);
            continue;
        };

        let (cfg_phys, cfg_virt) = match dma::alloc(256, 64) {
            Some(pair) => pair,
            None => {
                debugconf!("usb: failed to alloc cfg buffer\n");
                continue;
            }
        };
        unsafe { write_bytes(cfg_virt, 0, 256) };

    async fn get_cfg(
        ctx: &XhciContext,
        ep0_ring: &mut TrbRing,
        slot_id: u32,
        cfg_phys: u64,
        length: u16,
    ) -> Result<(), ()> {
        let setup = Trb {
            d0: 0x0680 | (0x0200 << 16),
            d1: (length as u32) << 16,
            d2: 8 | (2 << 16),
            d3: trb_type(2) | (1 << 6),
        };
        let data = Trb {
            d0: lo(cfg_phys),
            d1: hi(cfg_phys),
            d2: length as u32,
            d3: trb_type(3) | (1 << 16) | (1 << 5),
        };
        let status = Trb {
            d0: 0,
            d1: 0,
            d2: 0,
            d3: trb_type(4) | (1 << 5),
        };
        if !ep0_ring.push(setup) || !ep0_ring.push(data) || !ep0_ring.push(status) {
            debugconf!("usb: ep0 ring overflow for config\n");
            return Err(());
        }
        unsafe { write_volatile(ctx.doorbell.add(slot_id as usize), 1) };

        let Some(evt) = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type == 32 {
                    true
                } else {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    debugconf!(
                        "usb: unexpected cfg event type={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
                        evt_type,
                        completion,
                        evt.d0,
                        evt.d1,
                        evt.d2,
                        evt.d3
                    );
                    false
                }
            },
            800,
            EmbassyDuration::from_millis(5)
        )
        .await
        else {
            debugconf!("usb: timeout waiting for cfg transfer len={}\n", length);
            return Err(());
        };

        let completion = (evt.d2 >> 24) & 0xFF;
        if completion == 1 {
            Ok(())
        } else {
            Err(())
        }
    }

        let mut cfg_total_len: u16 = 0;
        if get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, 9).await.is_ok() {
            cfg_total_len = unsafe { *(cfg_virt.add(2) as *const u16) };
            let req_len = cfg_total_len.min(256) as u16;
            if req_len > 9 {
                let _ = get_cfg(&ctx, &mut ep0_ring, slot_id, cfg_phys, req_len).await;
            }
        }

        let cfg_slice_len = cfg_total_len.min(256) as usize;
        let cfg_slice = unsafe { core::slice::from_raw_parts(cfg_virt, cfg_slice_len) };

        {
            let mut idx = 0usize;
            while idx + 2 <= cfg_slice.len() {
                let len = cfg_slice[idx] as usize;
                if len == 0 || idx + len > cfg_slice.len() {
                    break;
                }
                let ty = cfg_slice[idx + 1];
                debugconf!("usb: cfg desc idx={} len={} ty=0x{:02X}\n", idx, len, ty);
                idx += len;
            }
        }

        let mut handled = false;
        if hid::attach_boot_device(BootAttachParams {
            ctx: &ctx,
            cmd_ring: &mut cmd_ring,
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
            register_device(slot_id as u32, target_port, DeviceKind::Hid);
            handled = true;
        } else if pen::try_handle(cfg_slice, target_port) {
            register_device(slot_id as u32, target_port, DeviceKind::Pen);
            handled = true;
        } else if print::try_handle(cfg_slice, target_port) {
            register_device(slot_id as u32, target_port, DeviceKind::Printer);
            handled = true;
        }

        if !handled {
            debugconf!("usb: device on port {} not claimed\n", target_port);
        }
    }
}

#[embassy_executor::task]
pub async fn poll_task(info: xhci::ControllerInfo) {
    let ctx = unsafe { XhciContext::new(info) };

    loop {
        let evt_opt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                evt_type == 32
            },
            400,
            EmbassyDuration::from_millis(5)
        )
        .await;

        let Some(evt) = evt_opt else {
            Timer::after(EmbassyDuration::from_millis(5)).await;
            continue;
        };

        let evt_slot = (evt.d3 >> 24) as u32;
        let evt_type = (evt.d3 >> 10) & 0x3F;
        let evt_cc = (evt.d2 >> 24) & 0xFF;
        debugconf!(
            "usb: transfer event type={} slot={} cc={} trb=[0x{:08X} 0x{:08X} 0x{:08X} 0x{:08X}]\n",
            evt_type,
            evt_slot,
            evt_cc,
            evt.d0,
            evt.d1,
            evt.d2,
            evt.d3
        );

        match device_kind_for_slot(evt_slot) {
            Some(DeviceKind::Hid) => {
                if !any_hid_registered() {
                    continue;
                }

                if !HID_TEST_INJECTED.swap(true, Ordering::SeqCst) {
                    Timer::after(EmbassyDuration::from_millis(200)).await;
                    hid::inject_test_report();
                }

                let handled = hid::with_runtime_mut_by_slot(evt_slot, |runtime| {
                    let completion = (evt.d2 >> 24) & 0xFF;
                    let residual = evt.d2 & 0x00FF_FFFF;
                    let data_len = runtime.report_len.min(runtime.ep.max_packet as u32) as usize;
                    let data = unsafe {
                        core::slice::from_raw_parts(runtime.report_virt, data_len)
                    };
                    hid::handle_report(runtime, completion, data, residual);

                    let normal = Trb {
                        d0: lo(runtime.report_phys),
                        d1: hi(runtime.report_phys),
                        d2: runtime.report_len,
                        d3: trb_type(1) | (1 << 5),
                    };

                    if !runtime.ep_ring.push(normal) {
                        debugconf!("usb: failed to requeue HID interrupt IN transfer\n");
                    } else {
                        unsafe {
                            write_volatile(
                                ctx.doorbell.add(runtime.slot_id as usize),
                                runtime.ep_target
                            );
                        }
                    }
                    true
                })
                .unwrap_or(false);

                if !handled {
                    debugconf!(
                        "usb: ignoring transfer event slot={} (no HID runtime)\n",
                        evt_slot
                    );
                }
            }
            Some(DeviceKind::Printer) => {
                debugconf!("usb: printer devices are not supported yet\n");
            }
            Some(DeviceKind::Pen) => {
                debugconf!("usb: mass-storage devices are not supported yet\n");
            }
            None => {
                debugconf!("usb: transfer event for unknown slot {}\n", evt_slot);
            }
        }
    }
}
