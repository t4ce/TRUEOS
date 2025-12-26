pub mod hid;
pub mod input;
pub mod pen;
pub mod print;

use crate::debugconf;
use crate::pci::{dma, osal, xhci};
use crate::pci::xhci::{
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
static ENUM_READY: AtomicBool = AtomicBool::new(false);
static SCOUT_RUNNING: AtomicBool = AtomicBool::new(false);

struct UsbControllerState {
    info: xhci::ControllerInfo,
    ctx: XhciContext,
    ctx_stride_bytes: usize,
    ctx_stride_words: usize,
    dcbaa_phys: u64,
    dcbaa_virt: *mut u8,
    cmd_ring: TrbRing,
    _cmd_phys: u64,
    _cmd_virt: *mut u8,
    _evt_phys: u64,
    _evt_virt: *mut u8,
    _erst_phys: u64,
    _erst_virt: *mut u8,
}

unsafe impl Send for UsbControllerState {}
unsafe impl Sync for UsbControllerState {}

static USB_CTRL: Mutex<Option<UsbControllerState>> = Mutex::new(None);

fn init_controller(info: xhci::ControllerInfo) -> Result<UsbControllerState, ()> {
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
            return Err(());
        }
    };
    unsafe { write_bytes(dcbaa_virt, 0, (max_slots + 1) * 8) };

    const CMD_RING_TRBS: usize = 64;
    let (cmd_phys, cmd_virt_raw) = match dma::alloc(CMD_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc cmd ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(cmd_virt_raw, 0, CMD_RING_TRBS * size_of::<Trb>()) };
    let mut cmd_ring = unsafe { TrbRing::new(cmd_phys, cmd_virt_raw as *mut Trb, CMD_RING_TRBS) };

    const EVENT_RING_TRBS: usize = 64;
    let (evt_phys, evt_virt_raw) = match dma::alloc(EVENT_RING_TRBS * size_of::<Trb>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc event ring\n");
            return Err(());
        }
    };
    unsafe { write_bytes(evt_virt_raw, 0, EVENT_RING_TRBS * size_of::<Trb>()) };
    let mut event_ring = unsafe { EventRing::new(evt_phys, evt_virt_raw as *mut Trb, EVENT_RING_TRBS) };

    let (erst_phys, erst_virt) = match dma::alloc(size_of::<ErstEntry>(), 64) {
        Some(pair) => pair,
        None => {
            debugconf!("usb: failed to alloc ERST\n");
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

    debugconf!("usb: controller initialized; ready for rescans\n");

    Ok(UsbControllerState {
        info,
        ctx,
        ctx_stride_bytes,
        ctx_stride_words,
        dcbaa_phys,
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

fn has_device_on_port(port: u8) -> bool {
    DEVICES.lock().iter().any(|d| d.port == port)
}

fn cleanup_disconnected<const N: usize>(connected: &Vec<(u8, u32), N>) {
    let mut removed: Vec<(u32, DeviceKind), MAX_DEVICES> = Vec::new();
    {
        let mut guard = DEVICES.lock();
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
        if kind == DeviceKind::Hid {
            let _ = hid::unregister_runtime(slot_id);
        }
        debugconf!("usb: dropped device slot={} (disconnected)\n", slot_id);
    }
}

struct EnumReadyGuard;

impl Drop for EnumReadyGuard {
    fn drop(&mut self) {
        ENUM_READY.store(true, Ordering::Release);
        debugconf!("usb: ENUM_READY set by scout completion\n");
    }
}

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
    // Signal that at least one device is enumerated so poll_task can start.
    ENUM_READY.store(true, Ordering::Release);
    debugconf!(
        "usb: slot {} registered as {:?} on port {}\n",
        slot_id,
        kind,
        port
    );
}

fn device_kind_for_slot(slot_id: u32) -> Option<DeviceKind> {
    DEVICES.lock().iter().find(|d| d.slot_id == slot_id).map(|d| d.kind)
}

fn any_hid_registered() -> bool {
    DEVICES.lock().iter().any(|d| d.kind == DeviceKind::Hid)
}

#[embassy_executor::task]
pub async fn usb_scout(info: xhci::ControllerInfo) {
    if SCOUT_RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        debugconf!("usb: scout already running; skipping\n");
        return;
    }

    struct ScoutRunGuard;
    impl Drop for ScoutRunGuard {
        fn drop(&mut self) {
            SCOUT_RUNNING.store(false, Ordering::Release);
        }
    }

    let _scout_guard = ScoutRunGuard;

    // Take controller state out of the mutex so we don't hold a spinlock across `.await`.
    let mut state = USB_CTRL.lock().take();
    let mut state = match state {
        Some(existing) => existing,
        None => {
            // First run: do controller init. Keep ENUM_READY false until the first scan completes.
            ENUM_READY.store(false, Ordering::Release);
            let _guard = EnumReadyGuard;
            match init_controller(info) {
                Ok(s) => s,
                Err(()) => {
                    return;
                }
            }
        }
    };

    // Always rescan ports; enumerate newly connected devices.
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
        debugconf!("usb: connected port list overflow; skipping disconnect cleanup this pass\n");
    } else {
        cleanup_disconnected(&connected);
    }

    for (target_port, port_status) in connected.iter().copied() {
        if has_device_on_port(target_port) {
            continue;
        }

        // Enumerate this port.
        let ctx = &state.ctx;
        let dcbaa_virt = state.dcbaa_virt;
        let ctx_stride_bytes = state.ctx_stride_bytes;
        let ctx_stride_words = state.ctx_stride_words;
        let cmd_ring = &mut state.cmd_ring;
        let mut port_status = port_status;


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
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) & 0xFF;
                evt_slot == slot_id
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
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) & 0xFF;
                evt_slot == slot_id as u32
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
            cmd_ring,
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
            // Hand off this slot to the poller; stop consuming its events here.
            continue;
        } else if pen::try_handle(cfg_slice, target_port) {
            register_device(slot_id as u32, target_port, DeviceKind::Pen);
            handled = true;
            continue;
        } else if print::try_handle(cfg_slice, target_port) {
            register_device(slot_id as u32, target_port, DeviceKind::Printer);
            handled = true;
            continue;
        }

        if !handled {
            debugconf!("usb: device on port {} not claimed\n", target_port);
        }
    }

    *USB_CTRL.lock() = Some(state);
}

#[embassy_executor::task]
pub async fn poll_task(info: xhci::ControllerInfo) {
    let ctx = unsafe { XhciContext::new(info) };
    let mut heartbeat: u32 = 0;
    let mut idle_timeouts: u32 = 0;

    loop {
        if !ENUM_READY.load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(5)).await;
            continue;
        }

        heartbeat = heartbeat.wrapping_add(1);
        if heartbeat % 500 == 0 {
            debugconf!("usb: poll heartbeat ready loops={}\n", heartbeat);
        }

        let evt_opt = xhci::wait_for_event(
            |evt| {
                let evt_type = (evt.d3 >> 10) & 0x3F;
                if evt_type != 32 {
                    return false;
                }
                let evt_slot = (evt.d3 >> 24) as u32;
                device_kind_for_slot(evt_slot).is_some()
            },
            400,
            EmbassyDuration::from_millis(5)
        )
        .await;

        let Some(evt) = evt_opt else {
            idle_timeouts = idle_timeouts.wrapping_add(1);
            continue;
        };

        idle_timeouts = 0;

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

                    let before = runtime.ep_ring.state_snapshot();
                    if !runtime.ep_ring.push(normal) {
                        debugconf!("usb: failed to requeue HID interrupt IN transfer\n");
                    } else {
                        let after = runtime.ep_ring.state_snapshot();
                        debugconf!(
                            "[hid] requeue slot={} target={} ring_before=({}, {}) ring_after=({}, {})\n",
                            runtime.slot_id,
                            runtime.ep_target,
                            before.0,
                            before.1 as u8,
                            after.0,
                            after.1 as u8
                        );
                        unsafe {
                            write_volatile(
                                ctx.doorbell.add(runtime.slot_id as usize),
                                runtime.ep_target
                            );
                        }
                        debugconf!(
                            "[hid] doorbell slot={} target={} rung\n",
                            runtime.slot_id,
                            runtime.ep_target
                        );
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
