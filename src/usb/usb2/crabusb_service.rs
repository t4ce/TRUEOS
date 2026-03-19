use core::alloc::Layout;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::sync::atomic::{AtomicBool, Ordering};
use core::time::Duration;

use crab_usb::{
    DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp, EndpointKind, Event,
    EventHandler, KernelOp, USBHost, usb_if,
};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

struct TrueosCrabUsbKernel;

static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;
static INITIAL_SNAPSHOT_LOGGED: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER_READY: AtomicBool = AtomicBool::new(false);
static EVENT_HANDLER: Mutex<Option<EventHandler>> = Mutex::new(None);

impl DmaOp for TrueosCrabUsbKernel {
    fn page_size(&self) -> usize {
        4096
    }

    unsafe fn map_single(
        &self,
        dma_mask: u64,
        addr: NonNull<u8>,
        size: NonZeroUsize,
        align: usize,
        _direction: DmaDirection,
    ) -> Result<DmaMapHandle, DmaError> {
        let required_align = align.max(1);
        let layout = Layout::from_size_align(size.get(), required_align)
            .map_err(DmaError::LayoutError)?;
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
            crate::pci::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)
                .ok_or(DmaError::NoMemory)?;
        let bounce_virt = NonNull::new(bounce_virt).ok_or(DmaError::NoMemory)?;

        Ok(unsafe {
            DmaMapHandle::new(addr, DmaAddr::from(bounce_phys), layout, Some(bounce_virt))
        })
    }

    unsafe fn unmap_single(&self, handle: DmaMapHandle) {
        if let Some(alloc_virt) = handle.alloc_virt() {
            crate::pci::dma::dealloc(alloc_virt.as_ptr(), handle.size());
        }
    }

    unsafe fn alloc_coherent(&self, dma_mask: u64, layout: Layout) -> Option<DmaHandle> {
        let max_phys_exclusive = if dma_mask == u64::MAX {
            None
        } else {
            dma_mask.checked_add(1)
        };
        let (phys, virt) =
            crate::pci::dma::alloc_with_max(layout.size(), layout.align(), max_phys_exclusive)?;
        let virt = NonNull::new(virt)?;
        Some(unsafe { DmaHandle::new(virt, DmaAddr::from(phys), layout) })
    }

    unsafe fn dealloc_coherent(&self, handle: DmaHandle) {
        crate::pci::dma::dealloc(handle.as_ptr().as_ptr(), handle.size());
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

fn endpoint_kind_name(kind: &EndpointKind) -> &'static str {
    match kind {
        EndpointKind::Control(_) => "control",
        EndpointKind::IsochronousIn(_) => "iso-in",
        EndpointKind::IsochronousOut(_) => "iso-out",
        EndpointKind::BulkIn(_) => "bulk-in",
        EndpointKind::BulkOut(_) => "bulk-out",
        EndpointKind::InterruptIn(_) => "intr-in",
        EndpointKind::InterruptOut(_) => "intr-out",
    }
}

const HYPERX_VENDOR_ID: u16 = 0x0951;
const HYPERX_PRODUCT_ID: u16 = 0x16A4;

#[derive(Copy, Clone)]
struct PreferredAlt {
    interface_number: u8,
    alternate_setting: u8,
    class: u8,
    subclass: u8,
    protocol: u8,
    has_iso_out: bool,
    endpoint_count: usize,
}

fn pick_preferred_alt(configs: &[usb_if::descriptor::ConfigurationDescriptor]) -> Option<PreferredAlt> {
    let mut best: Option<PreferredAlt> = None;

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                let has_iso_out = alt.endpoints.iter().any(|ep| {
                    ep.transfer_type == usb_if::descriptor::EndpointType::Isochronous
                        && ep.direction == usb_if::transfer::Direction::Out
                });

                let score = if has_iso_out {
                    100
                } else if alt.class == 0x01 && alt.subclass == 0x02 {
                    50
                } else if alt.class == 0x01 {
                    25
                } else {
                    0
                };

                let candidate = PreferredAlt {
                    interface_number: alt.interface_number,
                    alternate_setting: alt.alternate_setting,
                    class: alt.class,
                    subclass: alt.subclass,
                    protocol: alt.protocol,
                    has_iso_out,
                    endpoint_count: alt.endpoints.len(),
                };

                let replace = match best {
                    None => true,
                    Some(current) => {
                        let current_score = if current.has_iso_out {
                            100
                        } else if current.class == 0x01 && current.subclass == 0x02 {
                            50
                        } else if current.class == 0x01 {
                            25
                        } else {
                            0
                        };

                        score > current_score
                            || (score == current_score
                                && candidate.endpoint_count > current.endpoint_count)
                            || (score == current_score
                                && candidate.endpoint_count == current.endpoint_count
                                && candidate.alternate_setting > current.alternate_setting)
                    }
                };

                if replace {
                    best = Some(candidate);
                }
            }
        }
    }

    best
}

async fn log_opened_device_graph(host: &mut USBHost, dev_idx: usize, dev_info: &crab_usb::DeviceInfo) {
    let mut device = match host.open_device(dev_info).await {
        Ok(device) => device,
        Err(err) => {
            let desc = dev_info.descriptor();
            crate::log!(
                "crabusb: open dev#{} {:04X}:{:04X} failed: {:?}\n",
                dev_idx,
                desc.vendor_id,
                desc.product_id,
                err
            );
            return;
        }
    };

    let desc = device.descriptor();
    let vendor_id = desc.vendor_id;
    let product_id = desc.product_id;
    crate::log!(
        "crabusb: open dev#{} slot={} vid={:04X} pid={:04X} mfg={}\n",
        dev_idx,
        device.slot_id(),
        vendor_id,
        product_id,
        device.manufacturer().unwrap_or("-")
    );

    let configs = device.configurations().to_vec();
    if vendor_id == HYPERX_VENDOR_ID
        && product_id == HYPERX_PRODUCT_ID
        && let Some(preferred) = pick_preferred_alt(&configs)
    {
        crate::log!(
            "crabusb: target {:04X}:{:04X} preferred if#{} alt={} class={:02X} subclass={:02X} proto={:02X} iso_out={}\n",
            vendor_id,
            product_id,
            preferred.interface_number,
            preferred.alternate_setting,
            preferred.class,
            preferred.subclass,
            preferred.protocol,
            preferred.has_iso_out
        );

        match device
            .claim_interface(preferred.interface_number, preferred.alternate_setting)
            .await
        {
            Ok(()) => crate::log!(
                "crabusb: target {:04X}:{:04X} selected if#{} alt={}\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting
            ),
            Err(err) => crate::log!(
                "crabusb: target {:04X}:{:04X} preferred if#{} alt={} claim failed: {:?}\n",
                vendor_id,
                product_id,
                preferred.interface_number,
                preferred.alternate_setting,
                err
            ),
        }
    }

    for config in configs.iter() {
        for interface in config.interfaces.iter() {
            for alt in interface.alt_settings.iter() {
                match device
                    .claim_interface(interface.interface_number, alt.alternate_setting)
                    .await
                {
                    Ok(()) => {
                        crate::log!(
                            "crabusb: open dev#{} if#{} alt={} claim ok class={:02X} subclass={:02X} proto={:02X}\n",
                            dev_idx,
                            alt.interface_number,
                            alt.alternate_setting,
                            alt.class,
                            alt.subclass,
                            alt.protocol
                        );
                    }
                    Err(err) => {
                        crate::log!(
                            "crabusb: open dev#{} if#{} alt={} claim failed: {:?}\n",
                            dev_idx,
                            alt.interface_number,
                            alt.alternate_setting,
                            err
                        );
                        continue;
                    }
                }

                for ep in alt.endpoints.iter() {
                    let ep_num = ep.address & 0x0F;
                    match device.get_endpoint(ep.address).await {
                        Ok(kind) => {
                            crate::log!(
                                "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} num={} kind={} mps={} interval={}\n",
                                dev_idx,
                                alt.interface_number,
                                alt.alternate_setting,
                                ep.address,
                                ep_num,
                                endpoint_kind_name(&kind),
                                ep.max_packet_size,
                                ep.interval
                            );
                        }
                        Err(err) => {
                            crate::log!(
                                "crabusb: open dev#{} if#{} alt={} ep=0x{:02X} get failed: {:?}\n",
                                dev_idx,
                                alt.interface_number,
                                alt.alternate_setting,
                                ep.address,
                                err
                            );
                        }
                    }
                }
            }
        }
    }
}

async fn probe_and_log(host: &mut USBHost) {
    crate::log!("crabusb: periodic probe begin\n");
    match host.probe_devices().await {
        Ok(devices) => {
            if devices.is_empty() {
                crate::log!("crabusb: no newly discovered devices\n");
            } else {
                crate::log!("crabusb: discovered {} new device(s)\n", devices.len());
                for dev in devices.iter() {
                    let desc = dev.descriptor();
                    crate::log!(
                        "crabusb: dev {:04X}:{:04X} class={:02X} subclass={:02X} proto={:02X}\n",
                        desc.vendor_id,
                        desc.product_id,
                        desc.class,
                        desc.subclass,
                        desc.protocol
                    );
                }
            }
        }
        Err(err) => crate::log!("crabusb: probe failed: {:?}\n", err),
    }
    crate::log!("crabusb: periodic probe end\n");
}

async fn crab_scout_once(host: &mut USBHost, info: super::super::xhci::XhcInfo) {
    if INITIAL_SNAPSHOT_LOGGED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!(
        "crabusb: one-time snapshot controller={} bdf={:02X}:{:02X}.{}\n",
        info.controller_id,
        info.bus,
        info.slot,
        info.function
    );

    crate::log!("crabusb: scout begin\n");
    match host.probe_devices().await {
        Ok(devices) => {
            crate::log!("crabusb: scout devices={}\n", devices.len());
            for (dev_idx, dev) in devices.iter().enumerate() {
                let desc = dev.descriptor();
                crate::log!(
                    "crabusb: scout dev#{} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} cfgs={}\n",
                    dev_idx,
                    desc.vendor_id,
                    desc.product_id,
                    desc.class,
                    desc.subclass,
                    desc.protocol,
                    dev.configurations().len()
                );
                for iface in dev.interface_descriptors() {
                    crate::log!(
                        "crabusb: scout dev#{} if#{} alt={} class={:02X} subclass={:02X} proto={:02X} eps={}\n",
                        dev_idx,
                        iface.interface_number,
                        iface.alternate_setting,
                        iface.class,
                        iface.subclass,
                        iface.protocol,
                        iface.endpoints.len()
                    );
                }
            }
            for (dev_idx, dev) in devices.iter().enumerate() {
                log_opened_device_graph(host, dev_idx, dev).await;
            }
        }
        Err(err) => crate::log!("crabusb: scout probe failed: {:?}\n", err),
    }
    crate::log!("crabusb: scout end\n");
}

fn discover_first_controller() -> Option<super::super::xhci::XhcInfo> {
    crate::pci::enumerate_impl();
    super::super::xhci::init_once();
    super::super::xhci::xhc_list().iter().copied().next()
}

fn install_event_handler(handler: EventHandler) {
    *EVENT_HANDLER.lock() = Some(handler);
    EVENT_HANDLER_READY.store(true, Ordering::Release);
}

fn uninstall_event_handler() {
    EVENT_HANDLER_READY.store(false, Ordering::Release);
    *EVENT_HANDLER.lock() = None;
}

#[embassy_executor::task]
pub async fn event_pump_task() {
    loop {
        if !EVENT_HANDLER_READY.load(Ordering::Acquire) {
            Timer::after(EmbassyDuration::from_millis(10)).await;
            continue;
        }

        let event = {
            let guard = EVENT_HANDLER.lock();
            guard.as_ref().map(|handler| handler.handle_event())
        };

        match event {
            Some(Event::Nothing) | None => {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::PortChange { port }) => {
                crate::log!("crabusb: pump port change on root port {}\n", port);
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
            Some(Event::Stopped) => {
                crate::log!("crabusb: pump observed stopped event\n");
                uninstall_event_handler();
                Timer::after(EmbassyDuration::from_millis(10)).await;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn bsp_service() {
    const OFFLINE_RETRY_MS: u64 = 1000;

    loop {
        let Some(info) = discover_first_controller() else {
            crate::log!("crabusb: no xhci controller available yet; retrying on BSP\n");
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        };

        crate::log!(
            "crabusb: BSP service binding controller {} at {:02X}:{:02X}.{}\n",
            info.controller_id,
            info.bus,
            info.slot,
            info.function
        );

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log!(
                    "crabusb: failed to create host for controller {}: {:?}\n",
                    info.controller_id,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
                continue;
            }
        };

        install_event_handler(host.create_event_handler());

        if let Err(err) = host.init().await {
            crate::log!(
                "crabusb: host init failed for controller {}: {:?}\n",
                info.controller_id,
                err
            );
            uninstall_event_handler();
            Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
            continue;
        }

        crate::log!(
            "crabusb: host init complete for controller {}\n",
            info.controller_id
        );
        crab_scout_once(&mut host, info).await;
        probe_and_log(&mut host).await;

        let mut idle_ticks = 0u32;
        loop {
            if !EVENT_HANDLER_READY.load(Ordering::Acquire) {
                crate::log!("crabusb: event handler stopped; rediscovering controller\n");
                break;
            }

            idle_ticks = idle_ticks.wrapping_add(1);
            if idle_ticks >= 200 {
                idle_ticks = 0;
                probe_and_log(&mut host).await;
            }
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
        uninstall_event_handler();
        Timer::after(EmbassyDuration::from_millis(OFFLINE_RETRY_MS)).await;
    }
}
