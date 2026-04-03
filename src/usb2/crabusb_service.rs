use alloc::string::String;
use core::fmt::Write;
use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;

use crab_usb::{Event, EventHandler, KernelOp, USBHost};
use dma_api::{DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;
use core::sync::atomic::{AtomicBool, Ordering};

pub(super) struct TrueosCrabUsbKernel;

pub(super) static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;

use super::xhci::MAX_XHCI_CONTROLLERS;

static EVENT_HANDLER_READY: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static PROBE_REQUESTED: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static EVENT_HANDLER: [Mutex<Option<EventHandler>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];

#[inline]
fn usb_log_all_enabled() -> bool {
    crate::logflag::USB_LOG_ALL.load(Ordering::Relaxed)
}

#[derive(Clone, Copy)]
struct BounceMapping {
    orig_virt: usize,
    bounce_virt: usize,
    size: usize,
    direction: DmaDirection,
}

static BOUNCE_MAPPINGS: Mutex<alloc::vec::Vec<BounceMapping>> = Mutex::new(alloc::vec::Vec::new());

#[inline]
fn crabusb_dma_cache_flush(addr: NonNull<u8>, size: usize) {
    #[cfg(target_arch = "x86_64")]
    unsafe {
        use core::arch::x86_64::{_mm_clflush, _mm_mfence};

        if size == 0 {
            return;
        }

        let line = 64usize;
        let start = (addr.as_ptr() as usize) & !(line - 1);
        let end = (addr.as_ptr() as usize).saturating_add(size);
        let mut ptr = start;
        while ptr < end {
            _mm_clflush(ptr as *const _);
            ptr = ptr.saturating_add(line);
        }
        _mm_mfence();
    }
}

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    fn flush(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    fn invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    fn flush_invalidate(&self, addr: NonNull<u8>, size: usize) {
        crabusb_dma_cache_flush(addr, size);
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let required_align = align.max(1);
        let layout =
            Layout::from_size_align(size.get(), required_align).map_err(DmaError::LayoutError)?;
        let phys = crate::phys::virt_to_phys_checked(addr.as_ptr()).ok_or(DmaError::NoMemory)?;

        let aligned = phys.is_multiple_of(required_align as u64);
        let in_mask = phys
            .checked_add(size.get().saturating_sub(1) as u64)
            .map(|end| end <= dma_mask)
            .unwrap_or(false);

        if aligned && in_mask {
            return Ok(unsafe { DmaMapHandle::new(addr, DmaAddr::from(phys), layout, None) });
        }

        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (bounce_phys, bounce_virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        if matches!(direction, DmaDirection::ToDevice | DmaDirection::Bidirectional) {
            unsafe {
                core::ptr::copy_nonoverlapping(addr.as_ptr(), bounce_virt.as_ptr(), layout.size())
            };
        }

        BOUNCE_MAPPINGS.lock().push(BounceMapping {
            orig_virt: addr.as_ptr() as usize,
            bounce_virt: bounce_virt.as_ptr() as usize,
            size: layout.size(),
            direction,
        });

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            let mapping = {
                let mut mappings = BOUNCE_MAPPINGS.lock();
                mappings
                    .iter()
                    .position(|entry| entry.bounce_virt == alloc_virt.as_ptr() as usize)
                    .map(|idx| mappings.swap_remove(idx))
            };

            if let Some(mapping) = mapping
                && matches!(
                    mapping.direction,
                    DmaDirection::FromDevice | DmaDirection::Bidirectional
                )
            {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        mapping.bounce_virt as *const u8,
                        mapping.orig_virt as *mut u8,
                        mapping.size,
                    );
                }
            }

            crate::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
    }
}

impl KernelOp for TrueosCrabUsbKernel {
    fn delay(&self, duration: Duration) {
        let millis = duration.as_millis();
        if millis == 0 {
            return;
        }
        let timeout_ms = millis.min(u128::from(u64::MAX)) as u64;
        let _ = crate::wait::spin_until_timeout(timeout_ms, || false);
    }
}

fn connected_ports_summary(controller_id: usize) -> String {
    let mut out = String::new();
    let mut any = false;

    if let Some(diag) = super::controller_mmio_diag(controller_id) {
        for port in diag.ports {
            if (port.portsc & 0x1) == 0 {
                continue;
            }
            if any {
                let _ = out.write_str(",");
            }
            let _ = write!(out, "{}", port.port_id);
            any = true;
        }
    }

    if !any {
        out.push('-');
    }

    out
}

fn install_event_handler(controller_id: usize, handler: EventHandler) {
    *EVENT_HANDLER[controller_id].lock() = Some(handler);
    EVENT_HANDLER_READY[controller_id].store(true, Ordering::Release);
}

fn uninstall_event_handler(controller_id: usize) {
    EVENT_HANDLER_READY[controller_id].store(false, Ordering::Release);
    *EVENT_HANDLER[controller_id].lock() = None;
}

async fn probe_and_bind(
    host: &mut USBHost,
    info: super::TlbUsbController,
    spawner: &Spawner,
) {
    if let Ok(devices) = host.probe_devices().await {
        if usb_log_all_enabled() {
            crate::log!(
                "crabusb: probe ctrl={} devices={}\n",
                info.index,
                devices.len()
            );
        }

        if devices.is_empty() && usb_log_all_enabled() {
            crate::log!("crabusb: descriptor check ctrl={} none\n", info.index);
        }

        for dev in devices.iter() {
            let desc = dev.descriptor();
            let topo = dev.topology();
            let if_count: usize = dev
                .configurations()
                .iter()
                .map(|cfg| cfg.interfaces.len())
                .sum();
            let ep_count: usize = dev
                .configurations()
                .iter()
                .flat_map(|cfg| cfg.interfaces.iter())
                .flat_map(|interface| interface.alt_settings.iter())
                .map(|alt| alt.endpoints.len())
                .sum();

            if usb_log_all_enabled() {
                crate::log!(
                    "crabusb: dev ctrl={} root_port={} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} ifs={} eps={}\n",
                    info.index,
                    topo.root_port_id,
                    desc.vendor_id,
                    desc.product_id,
                    desc.class,
                    desc.subclass,
                    desc.protocol,
                    if_count,
                    ep_count
                );
                crate::log!(
                    "crabusb: descriptor check ctrl={} root_port={} ok cfgs={}\n",
                    info.index,
                    topo.root_port_id,
                    dev.configurations().len()
                );
            }

            let controller_id = info.index as u32;
            let mut bound_any = false;
            if super::hid::boot::maybe_start_hid_boot_streams(host, dev, spawner, controller_id).await
            {
                bound_any = true;
            }
            if super::hid::mediacontrol::maybe_start_media_control(host, dev, spawner, controller_id)
                .await
            {
                bound_any = true;
            }
            if super::hid::leds::maybe_start_led_controller(host, dev, spawner, controller_id).await
            {
                bound_any = true;
            }
            if super::midi::maybe_start_midi(host, dev, spawner, controller_id).await {
                bound_any = true;
            }
            if super::pen::maybe_start_mass_storage(host, dev, spawner, controller_id).await {
                bound_any = true;
            }
            if bound_any && usb_log_all_enabled() {
                crate::log!(
                    "crabusb: bind ctrl={} root_port={} vid={:04X} pid={:04X} handoff=true\n",
                    info.index,
                    topo.root_port_id,
                    desc.vendor_id,
                    desc.product_id
                );
            }
        }
    }
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn event_pump_task(controller_id: usize) {
    loop {
        if !EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(10)).await;
            continue;
        }

        let event = {
            let guard = EVENT_HANDLER[controller_id].lock();
            guard.as_ref().map(|handler| handler.handle_event())
        };

        match event {
            Some(Event::Nothing) | None => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::PortChange { port }) => {
                PROBE_REQUESTED[controller_id].store(true, Ordering::Release);
                if usb_log_all_enabled() {
                    crate::log!(
                        "crabusb: pump port change ctrl={} root_port={}\n",
                        controller_id,
                        port
                    );
                }
            }
            Some(Event::Stopped) => {
                if usb_log_all_enabled() {
                    crate::log!("crabusb: pump stopped ctrl={}\n", controller_id);
                }
                uninstall_event_handler(controller_id);
            }
        }
    }
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn bsp_service(controller_index: usize) {
    const RETRY_MS: u64 = 1000;
    const INTEL_PROBE_SETTLE_MS: u64 = 1500;
    const INTEL_REPROBE_MS: u64 = 1500;
    let spawner: Spawner = unsafe { Spawner::for_current_executor().await };

    loop {
        let Some(info) = super::controller_by_index(controller_index) else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(_) => {
                Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
                continue;
            }
        };

        if host.init().await.is_ok() {
            let connected_ports = connected_ports_summary(info.index);
            let intel_settle_probe = info.vendor_id == 0x8086;
            crate::log!(
                "crabusb: init successful ctrl={} bdf={:02X}:{:02X}.{} vid={:04X} pid={:04X} mmio={:p} ports={}\n",
                info.index,
                info.bus,
                info.slot,
                info.function,
                info.vendor_id,
                info.device_id,
                info.mmio_base,
                connected_ports
            );

            PROBE_REQUESTED[info.index].store(false, Ordering::Release);
            let mut quiet_probe_until = None;
            let mut reprobe_until = None;
            if intel_settle_probe {
                if usb_log_all_enabled() {
                    crate::log!(
                        "crabusb: controller {} intel deferred probe before event pump; waiting {}ms\n",
                        info.index,
                        INTEL_PROBE_SETTLE_MS
                    );
                }
                quiet_probe_until =
                    Some(Instant::now() + EmbassyDuration::from_millis(INTEL_PROBE_SETTLE_MS));
            } else {
                install_event_handler(info.index, host.create_event_handler());
                let _ = spawner.spawn(event_pump_task(info.index));
                probe_and_bind(&mut host, info, &spawner).await;
            }

            loop {
                if PROBE_REQUESTED[info.index].swap(false, Ordering::AcqRel) {
                    if intel_settle_probe {
                        quiet_probe_until =
                            Some(Instant::now() + EmbassyDuration::from_millis(INTEL_PROBE_SETTLE_MS));
                    } else {
                        probe_and_bind(&mut host, info, &spawner).await;
                    }
                }
                if let Some(deadline) = quiet_probe_until {
                    if Instant::now() >= deadline {
                        if usb_log_all_enabled() {
                            crate::log!(
                                "crabusb: servicing settled probe on controller {}\n",
                                info.index
                            );
                        }
                        if intel_settle_probe
                            && !EVENT_HANDLER_READY[info.index].load(Ordering::Acquire)
                        {
                            if usb_log_all_enabled() {
                                crate::log!(
                                    "crabusb: controller {} installing event pump before settled probe\n",
                                    info.index
                                );
                            }
                            install_event_handler(info.index, host.create_event_handler());
                            let _ = spawner.spawn(event_pump_task(info.index));
                        }
                        probe_and_bind(&mut host, info, &spawner).await;
                        if !intel_settle_probe {
                            install_event_handler(info.index, host.create_event_handler());
                            let _ = spawner.spawn(event_pump_task(info.index));
                        }
                        quiet_probe_until = None;
                    }
                }
                if let Some(deadline) = reprobe_until {
                    if Instant::now() >= deadline {
                        if usb_log_all_enabled() {
                            crate::log!(
                                "crabusb: controller {} intel periodic reprobe\n",
                                info.index
                            );
                        }
                        probe_and_bind(&mut host, info, &spawner).await;
                        reprobe_until =
                            Some(Instant::now() + EmbassyDuration::from_millis(INTEL_REPROBE_MS));
                    }
                }
                if !EVENT_HANDLER_READY[info.index].load(Ordering::Acquire) {
                    if intel_settle_probe {
                        Timer::after(EmbassyDuration::from_millis(20)).await;
                        continue;
                    }
                    break;
                }
                Timer::after(EmbassyDuration::from_millis(20)).await;
            }
        }

        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    }
}
