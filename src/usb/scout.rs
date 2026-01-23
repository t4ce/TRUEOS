use super::hub::{HubChild, HubWork, MAX_DEVICES};
use super::xhci::{
    self, decode_port_status, hi, lo, write_reg64, ErstEntry, EventRing, Trb, TrbRing, XhciContext,
    MAX_XHCI_CONTROLLERS,
};
use super::{
    cdc_acm, disable_slot, enable_slot, enumerate_port, enumerate_with_params, hid, hub, mass,
    uac, DeviceKind, UsbControllerState, DEVICES, ENUM_READY, USB_LOG_VERBOSE,
};
use crate::pci::{dma, osal};
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use heapless::Vec;
use spin::Mutex;

static USB_CTRL: [Mutex<Option<UsbControllerState>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];
static SCOUT_RUNNING: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];

const SCRATCHPAD_BUF_SIZE: usize = 4096;

struct EnumReadyGuard {
    controller_id: usize,
}

impl Drop for EnumReadyGuard {
    fn drop(&mut self) {
        ENUM_READY[self.controller_id].store(true, Ordering::Release);
    }
}

fn has_device_on_port(controller_id: usize, port: u8) -> bool {
    DEVICES[controller_id].lock().iter().any(|d| d.port == port)
}

async fn cleanup_disconnected<const N: usize>(
    connected: &Vec<(u8, u32), N>,
    state: &mut UsbControllerState,
) {
    let mut removed: Vec<(u32, DeviceKind), MAX_DEVICES> = Vec::new();
    let controller_id = state.info.controller_id;
    {
        let mut guard = DEVICES[controller_id].lock();
        let mut idx = 0usize;
        while idx < guard.len() {
            let port = guard[idx].port;
            let still_connected = connected.iter().any(|(p, _)| *p == port);
            if still_connected {
                idx += 1;
                continue;
            }
            let entry = guard.remove(idx);
            let _ = removed.push((entry.slot_id, entry.kind));
        }
    }

    for (slot_id, kind) in removed.into_iter() {
        if let Err(()) = disable_slot(state, slot_id).await {
            crate::log!("usb: disable-slot for slot {} failed\n", slot_id);
        }
        if kind == DeviceKind::Hid {
            let _ = hid::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Mass {
            let _ = mass::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Cdc {
            let _ = cdc_acm::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Uac {
            let _ = uac::unregister_runtime(controller_id, slot_id);
        }
        crate::log!("usb: dropped device slot={} (disconnected)\n", slot_id);
    }
}

async fn enumerate_port_recursive(
    state: &mut UsbControllerState,
    target_port: u8,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
    enumerate_port(state, target_port, hub_queue).await;
}

async fn enumerate_hub_ports(
    state: &mut UsbControllerState,
    work: &HubWork,
    hub_queue: &mut heapless::Vec<HubWork, 16>,
) {
    let children = hub::collect_children(
        &state.ctx,
        work.hub_slot_id,
        work.route_string,
        work.depth,
        work.hub_speed_code,
        work.multi_tt,
        work.port_count,
        work.power_on_good_ms,
        work.tt_think_time,
    )
    .await;

    for HubChild {
        port,
        route,
        depth,
        speed_code,
        tt_info,
        tt_think_time,
    } in children.iter().copied()
    {
        crate::log!(
            "usb: hub child enumerate hub_slot={} port={} route=0x{:X} depth={} speed_code={}\n",
            work.hub_slot_id,
            port,
            route,
            depth,
            speed_code,
        );
        let Some(slot_id) = enable_slot(state, port).await else {
            crate::log!(
                "usb: hub child enable-slot failed hub_slot={} port={}\n",
                work.hub_slot_id,
                port
            );
            continue;
        };
        crate::log!(
            "usb: hub child enable-slot ok hub_slot={} port={} slot={}\n",
            work.hub_slot_id,
            port,
            slot_id
        );

        enumerate_with_params(
            state,
            port,
            slot_id,
            work.root_port,
            route,
            depth,
            speed_code,
            None,
            Some((work.hub_slot_id, port)),
            tt_info,
            tt_think_time,
            hub_queue,
        )
        .await;
    }
}

fn init_controller(info: xhci::XhcInfo) -> Result<UsbControllerState, ()> {
    osal::ensure_dma_api_initialized();

    let ctx = unsafe { XhciContext::new(info) };
    let page_size_mask = unsafe { ctx.page_size_mask() };
    if (page_size_mask & 0x1) == 0 {
        crate::log!(
            "usb: xhci lacks 4K page support PAGESIZE=0x{:X}\n",
            page_size_mask
        );
        return Err(());
    }
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
    let ctx_stride_bytes: usize = if csz_64 { 0x40 } else { 0x20 };
    let ctx_stride_words: usize = ctx_stride_bytes / 4;

    crate::log!(
        "usb: xhci hccparams1=0x{:08X} csz_64={} ctx_stride_bytes={}\n",
        ctx.hccparams1,
        csz_64,
        ctx_stride_bytes
    );

    hub::init_topology(&ctx);

    let max_slots = (ctx.hcsparams1 & 0xFF) as usize;
    let supports_64bit = (ctx.hccparams1 & 0x1) != 0;
    let dma_max_exclusive = if supports_64bit { None } else { Some(0x1_0000_0000) };

    let (dcbaa_phys, dcbaa_virt) = match dma::alloc_with_max((max_slots + 1) * 8, 64, dma_max_exclusive) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc dcbaa\n");
            return Err(());
        }
    };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    let scratchpad_count = ctx.max_scratchpad_buffers() as usize;
    let mut scratchpad_array_phys: u64 = 0;
    let mut scratchpad_array_virt: *mut u8 = core::ptr::null_mut();
    if scratchpad_count > 0 {
        let array_bytes = scratchpad_count * core::mem::size_of::<u64>();
        let (sp_array_phys, sp_array_virt) = match dma::alloc_with_max(array_bytes, 64, dma_max_exclusive) {
            Some(pair) => pair,
            None => {
                crate::log!(
                    "usb: failed to alloc scratchpad array count={}\n",
                    scratchpad_count
                );
                return Err(());
            }
        };
        unsafe { write_bytes(sp_array_virt, 0, array_bytes) };

        for idx in 0..scratchpad_count {
            let (buf_phys, buf_virt) = match dma::alloc_with_max(SCRATCHPAD_BUF_SIZE, SCRATCHPAD_BUF_SIZE, dma_max_exclusive) {
                Some(pair) => pair,
                None => {
                    crate::log!(
                        "usb: failed to alloc scratchpad buffer {}/{}\n",
                        idx + 1,
                        scratchpad_count
                    );
                    return Err(());
                }
            };
            unsafe {
                write_bytes(buf_virt, 0, SCRATCHPAD_BUF_SIZE);
                let arr_ptr = sp_array_virt as *mut u64;
                write_volatile(arr_ptr.add(idx), buf_phys);
            }
        }

        unsafe {
            let dcbaa = dcbaa_virt as *mut u64;
            write_volatile(dcbaa, sp_array_phys);
        }

        scratchpad_array_phys = sp_array_phys;
        scratchpad_array_virt = sp_array_virt;
        crate::log!(
            "usb: scratchpads={} array=0x{:X}\n",
            scratchpad_count,
            sp_array_phys
        );
    }

    const CMD_RING_TRBS: usize = 64;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc cmd ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let mut cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 64;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc event ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let event_ring = unsafe { EventRing::new(evt_phys, evt_virt_raw as *mut Trb, EVENT_RING_TRBS) };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb: failed to alloc ERST\n");
            return Err(());
        }
    };
    unsafe {
        write_bytes(erst_virt, 0, size_of::<ErstEntry>());
        let entry = &mut *(erst_virt as *mut ErstEntry);
        entry.seg_base_lo = lo(evt_phys);
        entry.seg_base_hi = hi(evt_phys);
        entry.seg_size = EVENT_RING_TRBS as u32;
    }

    unsafe {
        write_reg64(ctx.op_base, 0x30, dcbaa_phys);
        // CONFIG.MaxSlotsEn: must be >= number of slots we intend to use.
        // Real hardware is much less forgiving than QEMU if this is too small.
        let slots_en = core::cmp::max(1, core::cmp::min(255, max_slots)) as u32;
        write_volatile(ctx.op_base.add(0x38 / 4), slots_en);

        const ERSTSZ: usize = 0x08 / 4;
        const ERSTBA: usize = 0x10 / 4;
        let intr0 = ctx.runtime.add(0x20 / 4);
        write_volatile(intr0.add(ERSTSZ), 1);
        write_volatile(intr0.add(ERSTBA), lo(erst_phys));
        write_volatile(intr0.add(ERSTBA + 1), hi(erst_phys));
        event_ring.update_erdp(intr0);
        xhci::install_event_ring(&ctx, event_ring, intr0);

        write_reg64(ctx.op_base, 0x18, cmd_ring.crcr_value());

        const USBCMD: usize = 0x00 / 4;
        const USBSTS: usize = 0x04 / 4;
        const USBCMD_RS: u32 = 1 << 0;
        const USBSTS_HCH: u32 = 1 << 0;

        // Clear sticky status bits that are RW1C. On some real machines these can be
        // left set by firmware (notably SRE) and the controller may refuse to run.
        // Bits: HSE(2), EINT(3), PCD(4), SRE(10)
        const USBSTS_RW1C_MASK: u32 = (1 << 2) | (1 << 3) | (1 << 4) | (1 << 10);
        let sts0 = read_volatile(ctx.op_base.add(USBSTS));
        let clear = sts0 & USBSTS_RW1C_MASK;
        if clear != 0 {
            write_volatile(ctx.op_base.add(USBSTS), clear);
        }

        write_volatile(ctx.op_base.add(USBCMD), USBCMD_RS);
        let mut spin: u32 = 1_000_000;
        while spin > 0 {
            let sts = read_volatile(ctx.op_base.add(USBSTS));
            if (sts & USBSTS_HCH) == 0 {
                break;
            }
            spin -= 1;
        }
    }

    crate::log!("usb: controller initialized; ready for rescans\n");

    Ok(UsbControllerState {
        info,
        ctx,
        ctx_stride_bytes,
        ctx_stride_words,
        dcbaa_phys,
        dcbaa_virt,
        scratchpad_array_phys,
        scratchpad_array_virt,
        scratchpad_count: scratchpad_count as u32,
        cmd_ring,
        _cmd_phys: cmd_phys,
        _cmd_virt: cmd_virt_raw,
        _evt_phys: evt_phys,
        _evt_virt: evt_virt_raw,
        _erst_phys: erst_phys,
        _erst_virt: erst_virt,
    })
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn usb_scout(info: xhci::XhcInfo) {
    let controller_id = info.controller_id;
    if SCOUT_RUNNING[controller_id]
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!("usb: scout already running; skipping\n");
        return;
    }

    struct ScoutRunGuard {
        controller_id: usize,
    }
    impl Drop for ScoutRunGuard {
        fn drop(&mut self) {
            SCOUT_RUNNING[self.controller_id].store(false, Ordering::Release);
        }
    }

    let _scout_guard = ScoutRunGuard { controller_id };

    // Take controller state out of the mutex so we don't hold a spinlock across `.await`.
    let state = USB_CTRL[controller_id].lock().take();
    let mut state = match state {
        Some(existing) => existing,
        None => {
            // First run: do controller init. Keep ENUM_READY false until the first scan completes.
            ENUM_READY[controller_id].store(false, Ordering::Release);
            let _guard = EnumReadyGuard { controller_id };
            match init_controller(info) {
                Ok(s) => s,
                Err(()) => {
                    return;
                }
            }
        }
    };

    // Always rescan ports; enumerate newly connected devices.
    if USB_LOG_VERBOSE {
        xhci::log_ports_table(&state.ctx);
    }

    let mut connected: Vec<(u8, u32), 64> = Vec::new();
    let mut connected_overflowed = false;
    for port in 0..state.ctx.port_count {
        let status = unsafe { state.ctx.portsc(port as usize) };
        let (connected_flag, _, _) = decode_port_status(status);
        if connected_flag {
            if connected.push(((port + 1) as u8, status)).is_err() {
                connected_overflowed = true;
            }
        }
    }

    // If we couldn't record all connected ports, don't treat missing entries as disconnects.
    if connected_overflowed {
        crate::log!("usb: connected port list overflow; skipping disconnect cleanup this pass\n");
    } else {
        cleanup_disconnected(&connected, &mut state).await;
    }

    let mut hub_queue: heapless::Vec<HubWork, 16> = heapless::Vec::new();

    for (target_port, _port_status) in connected.iter().copied() {
        if has_device_on_port(controller_id, target_port) {
            continue;
        }

        enumerate_port_recursive(&mut state, target_port, &mut hub_queue).await;
    }

    let mut idx = 0usize;
    while idx < hub_queue.len() {
        let work = hub_queue[idx];
        idx += 1;
        enumerate_hub_ports(&mut state, &work, &mut hub_queue).await;
    }

    *USB_CTRL[controller_id].lock() = Some(state);
}