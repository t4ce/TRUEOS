use super::xhci::{
    self, ErstEntry, EventRing, MAX_XHCI_CONTROLLERS, Trb, TrbRing, XhciContext,
    decode_port_status, hi, lo, write_reg64,
};
use super::{
    DEVICES, DeviceKind, ENUM_READY, MAX_DEVICES, USB_LOG_VERBOSE, UsbControllerState, cdc_acm,
    disable_slot, enumerate_port, hid, mass, midi, uac,
};
use crate::pci::dma;
use core::mem::size_of;
use core::ptr::{read_volatile, write_bytes, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;
use spin::Mutex;

static USB_CTRL: [Mutex<Option<UsbControllerState>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];
static PORT_SNAPSHOTS: [Mutex<Vec<ScoutedPort, 64>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];
static SCOUT_RUNNING: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static SCOUT_SERVICE_RUNNING: [AtomicBool; MAX_XHCI_CONTROLLERS] =
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

fn store_port_snapshot(controller_id: usize, snapshot: Vec<ScoutedPort, 64>) {
    let mut snap = PORT_SNAPSHOTS[controller_id].lock();
    // Defensive: if a transient scout pass yields an empty sample, do not clobber
    // an already-populated snapshot that table commands rely on.
    if snapshot.is_empty() && !snap.is_empty() {
        return;
    }
    *snap = snapshot;
}

async fn cleanup_disconnected<const N: usize>(
    connected: &Vec<(u8, u32), N>,
    state: &mut UsbControllerState,
) {
    let mut removed: Vec<(u32, DeviceKind, Option<super::DeviceResources>), MAX_DEVICES> =
        Vec::new();
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
            let _ = removed.push((entry.slot_id, entry.kind, entry.resources));
        }
    }

    for (slot_id, kind, resources) in removed.into_iter() {
        if let Err(()) = disable_slot(state, slot_id).await {
            crate::log!(
                "usb[xHCI {}]: disable-slot for slot {} failed\n",
                controller_id,
                slot_id
            );
        }
        if let Some(res) = resources {
            dma::dealloc(res.ep0_virt_raw as *mut u8, res.ep0_bytes);
            dma::dealloc(res.input_ctx_virt as *mut u8, 4096);
            dma::dealloc(res.dev_ctx_virt as *mut u8, 4096);
        }
        if kind == DeviceKind::Hid {
            let _ = hid::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Mass {
            let _ = mass::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Cdc {
            let _ = cdc_acm::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Uac {
            let _ = uac::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Midi {
            let _ = midi::unregister_runtime(controller_id, slot_id);
        } else if kind == DeviceKind::Leds {
            let _ = super::leds::unregister_runtime(controller_id, slot_id);
        }
        crate::log!(
            "usb[xHCI {}]: dropped device slot={} (disconnected)\n",
            controller_id,
            slot_id
        );
    }
}

fn init_controller(info: xhci::XhcInfo) -> Result<UsbControllerState, ()> {
    let controller_id = info.controller_id;

    let ctx = unsafe { XhciContext::new(info) };
    let page_size_mask = unsafe { ctx.page_size_mask() };
    if (page_size_mask & 0x1) == 0 {
        crate::log!(
            "usb[xHCI {}]: xhci lacks 4K page support PAGESIZE=0x{:X}\n",
            controller_id,
            page_size_mask
        );
        return Err(());
    }
    let csz_64 = (ctx.hccparams1 & (1 << 2)) != 0;
    let ctx_stride_bytes: usize = if csz_64 { 0x40 } else { 0x20 };
    let ctx_stride_words: usize = ctx_stride_bytes / 4;

    crate::log!(
        "usb[xHCI {}]: xhci hccparams1=0x{:08X} csz_64={} ctx_stride_bytes={}\n",
        controller_id,
        ctx.hccparams1,
        csz_64,
        ctx_stride_bytes
    );

    let max_slots = (ctx.hcsparams1 & 0xFF) as usize;
    let supports_64bit = (ctx.hccparams1 & 0x1) != 0;
    let dma_max_exclusive = if supports_64bit {
        None
    } else {
        Some(0x1_0000_0000)
    };

    let (dcbaa_phys, dcbaa_virt) =
        match dma::alloc_with_max((max_slots + 1) * 8, 64, dma_max_exclusive) {
            Some(pair) => pair,
            None => {
                crate::log!("usb[xHCI {}]: failed to alloc dcbaa\n", controller_id);
                return Err(());
            }
        };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    let scratchpad_count = ctx.max_scratchpad_buffers() as usize;
    if scratchpad_count > 0 {
        let array_bytes = scratchpad_count * core::mem::size_of::<u64>();
        let (sp_array_phys, sp_array_virt) =
            match dma::alloc_with_max(array_bytes, 64, dma_max_exclusive) {
                Some(pair) => pair,
                None => {
                    crate::log!(
                        "usb[xHCI {}]: failed to alloc scratchpad array count={}\n",
                        controller_id,
                        scratchpad_count
                    );
                    return Err(());
                }
            };
        unsafe { write_bytes(sp_array_virt, 0, array_bytes) };

        for idx in 0..scratchpad_count {
            let (buf_phys, buf_virt) = match dma::alloc_with_max(
                SCRATCHPAD_BUF_SIZE,
                SCRATCHPAD_BUF_SIZE,
                dma_max_exclusive,
            ) {
                Some(pair) => pair,
                None => {
                    crate::log!(
                        "usb[xHCI {}]: failed to alloc scratchpad buffer {}/{}\n",
                        controller_id,
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

        crate::log!(
            "usb[xHCI {}]: scratchpads={} array=0x{:X}\n",
            controller_id,
            scratchpad_count,
            sp_array_phys
        );
    }

    const CMD_RING_TRBS: usize = 256;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb[xHCI {}]: failed to alloc cmd ring\n", controller_id);
            return Err(());
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 256;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb[xHCI {}]: failed to alloc event ring\n", controller_id);
            return Err(());
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let event_ring = unsafe { EventRing::new(evt_phys, evt_virt_raw as *mut Trb, EVENT_RING_TRBS) };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            crate::log!("usb[xHCI {}]: failed to alloc ERST\n", controller_id);
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

        // DWORD indices into the operational registers.
        const USBCMD: usize = 0;
        const USBSTS: usize = 1;
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

    crate::log!(
        "usb[xHCI {}]: controller initialized; ready for rescans\n",
        controller_id
    );

    Ok(UsbControllerState {
        info,
        ctx,
        ctx_stride_bytes,
        ctx_stride_words,
        dcbaa_virt,
        cmd_ring,
        _cmd_phys: cmd_phys,
        _cmd_virt: cmd_virt_raw,
        _evt_phys: evt_phys,
        _evt_virt: evt_virt_raw,
        _erst_phys: erst_phys,
        _erst_virt: erst_virt,
    })
}

async fn scout_pass(info: xhci::XhcInfo) {
    let controller_id = info.controller_id;

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
        if connected_flag
            && connected.push(((port + 1), status)).is_err() {
                connected_overflowed = true;
            }
    }

    // If we couldn't record all connected ports, don't treat missing entries as disconnects.
    if connected_overflowed {
        crate::log!(
            "usb[xHCI {}]: connected port list overflow; skipping disconnect cleanup this pass\n",
            controller_id
        );
    } else {
        cleanup_disconnected(&connected, &mut state).await;
    }

    for (target_port, _port_status) in connected.iter().copied() {
        if has_device_on_port(controller_id, target_port) {
            continue;
        }

        enumerate_port(&mut state, target_port).await;
    }

    // Publish a stable snapshot for shell/table commands.
    store_port_snapshot(controller_id, collect_ports(controller_id, &state));

    *USB_CTRL[controller_id].lock() = Some(state);
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn usb_scout_service(info: xhci::XhcInfo) {
    let controller_id = info.controller_id;
    if SCOUT_SERVICE_RUNNING[controller_id]
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        crate::log!(
            "usb[xHCI {}]: scout service already running; skipping\n",
            controller_id
        );
        return;
    }

    struct ScoutServiceGuard {
        controller_id: usize,
    }
    impl Drop for ScoutServiceGuard {
        fn drop(&mut self) {
            SCOUT_SERVICE_RUNNING[self.controller_id].store(false, Ordering::Release);
        }
    }
    let _guard = ScoutServiceGuard { controller_id };

    // First scan immediately.
    if SCOUT_RUNNING[controller_id]
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        struct ScoutRunGuard {
            controller_id: usize,
        }
        impl Drop for ScoutRunGuard {
            fn drop(&mut self) {
                SCOUT_RUNNING[self.controller_id].store(false, Ordering::Release);
            }
        }
        let _scout_guard = ScoutRunGuard { controller_id };
        scout_pass(info).await;
    }

    // Thereafter, rescan periodically.
    const RESCAN_PERIOD_MS: u64 = 2_000;
    const POLL_MS: u64 = 250;
    let mut elapsed_ms: u64 = 0;
    loop {
        Timer::after(EmbassyDuration::from_millis(POLL_MS)).await;
        elapsed_ms = elapsed_ms.saturating_add(POLL_MS);

        if elapsed_ms >= RESCAN_PERIOD_MS {
            if SCOUT_RUNNING[controller_id]
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                struct ScoutRunGuard {
                    controller_id: usize,
                }
                impl Drop for ScoutRunGuard {
                    fn drop(&mut self) {
                        SCOUT_RUNNING[self.controller_id].store(false, Ordering::Release);
                    }
                }
                let _scout_guard = ScoutRunGuard { controller_id };
                scout_pass(info).await;
            }

            elapsed_ms = 0;
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScoutedPort {
    pub port_id: u8,
    pub status: u32,
    pub connected: bool,
    pub enabled: bool,
    pub speed: &'static str,
    pub device_kind: Option<&'static str>,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

fn collect_ports(controller_id: usize, state: &UsbControllerState) -> Vec<ScoutedPort, 64> {
    let mut results: Vec<ScoutedPort, 64> = Vec::new();

    for port in 0..state.ctx.port_count {
        let status = unsafe { state.ctx.portsc(port as usize) };
        let (connected, enabled, speed) = decode_port_status(status);

        let mut kind_str: Option<&'static str> = None;
        let mut vid: Option<u16> = None;
        let mut pid: Option<u16> = None;

        // Check registered devices
        {
            let devs = super::DEVICES[controller_id].lock();
            if let Some(dev) = devs.iter().find(|d| d.port == (port + 1)) {
                kind_str = Some(match dev.kind {
                    DeviceKind::Hid => "hid",
                    DeviceKind::Mass => "mass",
                    DeviceKind::Printer => "printer",
                    DeviceKind::Pen => "pen",
                    DeviceKind::Cdc => "cdc",
                    DeviceKind::Uac => "uac",
                    DeviceKind::Midi => "midi",
                    DeviceKind::Leds => "leds",
                    DeviceKind::Unknown => "unknown",
                });

                // Try to get VID/PID from identity cache
                if let Some(ident) = super::identity_for_slot(controller_id, dev.slot_id) {
                    vid = Some(ident.vid);
                    pid = Some(ident.pid);
                    if let Some(name) = super::friendly_name_for_vidpid(ident.vid, ident.pid)
                        && matches!(dev.kind, DeviceKind::Unknown) {
                            kind_str = Some(name);
                        }
                }
            }
        }

        // Fallback or detecting state
        if kind_str.is_none() && connected {
            if let Some((v, p)) = xhci::get_port_vidpid(controller_id, port + 1 ) {
                vid = Some(v);
                pid = Some(p);
                if let Some(name) = super::friendly_name_for_vidpid(v, p) {
                    kind_str = Some(name);
                } else if v != 0 || p != 0 {
                    kind_str = Some("detecting");
                }
            } else {
                kind_str = Some("present");
            }
        }

        let _ = results.push(ScoutedPort {
            port_id: (port + 1),
            status,
            connected,
            enabled,
            speed,
            device_kind: kind_str,
            vid,
            pid,
        });
    }
    results
}

pub fn port_snapshot(controller_id: usize) -> Vec<ScoutedPort, 64> {
    PORT_SNAPSHOTS[controller_id].lock().clone()
}
