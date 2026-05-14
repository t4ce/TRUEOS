use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::fmt::Write;
use core::num::NonZeroUsize;
use core::ptr::NonNull;
use core::time::Duration;

use core::sync::atomic::{AtomicBool, Ordering};
use crab_usb::{Event, EventHandler, KernelOp, USBHost, usb_if};
use dma_api::{DmaAddr, DmaDirection, DmaError, DmaHandle, DmaMapHandle, DmaOp};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use spin::Mutex;

pub(super) struct TrueosCrabUsbKernel;

pub(super) static CRABUSB_KERNEL: TrueosCrabUsbKernel = TrueosCrabUsbKernel;

use super::xhci::MAX_XHCI_CONTROLLERS;

static EVENT_HANDLER_READY: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static PROBE_REQUESTED: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static BOOT_DEFER_DONE: [AtomicBool; MAX_XHCI_CONTROLLERS] =
    [const { AtomicBool::new(false) }; MAX_XHCI_CONTROLLERS];
static EVENT_HANDLER: [Mutex<Option<EventHandler>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(None) }; MAX_XHCI_CONTROLLERS];
static OBSERVED_DEVICES: [Mutex<Vec<ObservedUsbDevice>>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(Vec::new()) }; MAX_XHCI_CONTROLLERS];
static CONTROLLER_RUNTIME_DIAG: [Mutex<super::UsbControllerRuntimeDiag>; MAX_XHCI_CONTROLLERS] =
    [const { Mutex::new(super::UsbControllerRuntimeDiag::new()) }; MAX_XHCI_CONTROLLERS];

const EVENT_PUMP_NOT_READY_SLEEP_MS: u64 = 10;
const EVENT_PUMP_HOT_IDLE_YIELDS: u8 = 64;
const EVENT_PUMP_IDLE_SLEEP_US: u64 = 100;

#[inline]
fn usb_log_all_enabled() -> bool {
    crate::logflag::USB_LOG_ALL.load(Ordering::Relaxed)
}

fn speed_label(speed: usb_if::Speed) -> &'static str {
    match speed {
        usb_if::Speed::Low => "LS",
        usb_if::Speed::Full => "FS",
        usb_if::Speed::High => "HS",
        usb_if::Speed::SuperSpeed => "SS",
        usb_if::Speed::SuperSpeedPlus => "SS+",
        _ => "?",
    }
}

fn classify_device_kind(dev: &ObservedUsbDevice) -> &'static str {
    let triple = super::class::UsbClassTriple::from_codes(dev.class, dev.subclass, dev.protocol);
    match triple {
        super::class::UsbClassTriple::PerInterface => "per-if",
        super::class::UsbClassTriple::Unclassified { base, .. } => match base {
            super::class::UsbBaseClass::PerInterface => "per-if",
            _ => base.short_name(),
        },
        _ => triple.short_name(),
    }
}

#[derive(Clone)]
struct ObservedUsbDevice {
    backend_id: usize,
    slot_id: u32,
    stable_id: u32,
    root_port_id: u8,
    port_id: u8,
    route_string: u32,
    speed: &'static str,
    vendor_id: u16,
    product_id: u16,
    class: u8,
    subclass: u8,
    protocol: u8,
    num_configurations: u8,
    max_packet_size_0: u8,
    manufacturer: Option<String>,
    product: Option<String>,
    serial: Option<String>,
    configurations: Vec<super::TlbUsbConfiguration>,
}

#[inline]
fn update_runtime_diag(
    controller_id: usize,
    update: impl FnOnce(&mut super::UsbControllerRuntimeDiag),
) {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return;
    }
    let mut guard = CONTROLLER_RUNTIME_DIAG[controller_id].lock();
    update(&mut guard);
}

pub(crate) fn runtime_diag(controller_id: usize) -> super::UsbControllerRuntimeDiag {
    if controller_id >= MAX_XHCI_CONTROLLERS {
        return super::UsbControllerRuntimeDiag::new();
    }
    *CONTROLLER_RUNTIME_DIAG[controller_id].lock()
}

#[inline]
fn endpoint_transfer_type_label(transfer_type: usb_if::descriptor::EndpointType) -> &'static str {
    super::descriptor::endpoint_transfer_type_label(transfer_type)
}

fn collect_tlb_usb_configurations(
    configs: &[usb_if::descriptor::ConfigurationDescriptor],
) -> Vec<super::TlbUsbConfiguration> {
    configs
        .iter()
        .map(|cfg| super::TlbUsbConfiguration {
            configuration_value: cfg.configuration_value,
            attributes: cfg.attributes,
            max_power: cfg.max_power,
            interfaces: cfg
                .interfaces
                .iter()
                .flat_map(|interface| {
                    interface
                        .alt_settings
                        .iter()
                        .map(|alt| super::TlbUsbInterface {
                            interface_number: alt.interface_number,
                            alternate_setting: alt.alternate_setting,
                            class: alt.class,
                            subclass: alt.subclass,
                            protocol: alt.protocol,
                            endpoints: alt
                                .endpoints
                                .iter()
                                .map(|ep| super::TlbUsbEndpoint {
                                    address: ep.address,
                                    transfer_type: endpoint_transfer_type_label(ep.transfer_type),
                                    max_packet_size: ep.max_packet_size,
                                    interval: ep.interval,
                                })
                                .collect(),
                        })
                })
                .collect(),
        })
        .collect()
}

fn preserve_first_enumeration(
    previous: &ObservedUsbDevice,
    mut current: ObservedUsbDevice,
) -> ObservedUsbDevice {
    if !previous.configurations.is_empty() {
        current.num_configurations = previous.num_configurations;
        current.max_packet_size_0 = previous.max_packet_size_0;
        current.configurations = previous.configurations.clone();
    }
    current
}

#[inline]
fn same_physical_device(a: &ObservedUsbDevice, b: &ObservedUsbDevice) -> bool {
    a.stable_id == b.stable_id
        || (a.root_port_id == b.root_port_id && a.route_string == b.route_string)
}

fn sync_observed_devices(
    controller_id: usize,
    current: &[ObservedUsbDevice],
) -> (Vec<ObservedUsbDevice>, Vec<ObservedUsbDevice>) {
    let mut seen = OBSERVED_DEVICES[controller_id].lock();
    let previous = seen.clone();

    let mut merged_current: Vec<ObservedUsbDevice> = current
        .iter()
        .cloned()
        .map(|candidate| {
            previous
                .iter()
                .find(|known| same_physical_device(known, &candidate))
                .map(|known| preserve_first_enumeration(known, candidate.clone()))
                .unwrap_or(candidate)
        })
        .collect();

    if merged_current.is_empty() && !previous.is_empty() {
        if let Some(diag) = super::controller_mmio_diag(controller_id) {
            merged_current = previous
                .iter()
                .filter(|dev| {
                    diag.ports.iter().any(|port| {
                        port.port_id == dev.root_port_id && (port.portsc & (1 << 0)) != 0
                    })
                })
                .cloned()
                .collect();
        }
    }

    let connected = merged_current
        .iter()
        .cloned()
        .filter(|candidate| {
            !previous
                .iter()
                .any(|known| same_physical_device(known, candidate))
        })
        .collect();

    let disconnected = previous
        .iter()
        .cloned()
        .filter(|known| {
            !merged_current
                .iter()
                .any(|candidate| same_physical_device(known, candidate))
        })
        .collect();

    *seen = merged_current;
    (connected, disconnected)
}

fn clear_observed_devices(controller_id: usize) {
    OBSERVED_DEVICES[controller_id].lock().clear();
}

fn note_disconnected_device(controller_id: usize, dev: ObservedUsbDevice) {
    crate::log_info!(
        target: "usb";
        "crabusb: hotplug disconnect ctrl={} root_port={} dev={} stable_id={} vid={:04X} pid={:04X} class={:02X}/{:02X}/{:02X}\n",
        controller_id,
        dev.root_port_id,
        dev.backend_id,
        dev.stable_id,
        dev.vendor_id,
        dev.product_id,
        dev.class,
        dev.subclass,
        dev.protocol
    );
}

fn note_connected_device(controller_id: usize, dev: ObservedUsbDevice) {
    crate::log_info!(
        target: "usb";
        "crabusb: hotplug connect ctrl={} root_port={} dev={} stable_id={} vid={:04X} pid={:04X} class={:02X}/{:02X}/{:02X}\n",
        controller_id,
        dev.root_port_id,
        dev.backend_id,
        dev.stable_id,
        dev.vendor_id,
        dev.product_id,
        dev.class,
        dev.subclass,
        dev.protocol
    );
}

#[derive(Clone, Copy)]
struct BounceMapping {
    orig_virt: usize,
    bounce_virt: usize,
    size: usize,
    direction: DmaDirection,
}

static BOUNCE_MAPPINGS: Mutex<alloc::vec::Vec<BounceMapping>> = Mutex::new(alloc::vec::Vec::new());
const XHCI_NORMAL_TRB_BYTES: usize = 64 * 1024;

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
        let needs_contiguous_bounce = size.get() > XHCI_NORMAL_TRB_BYTES
            && matches!(direction, DmaDirection::ToDevice | DmaDirection::Bidirectional);

        if aligned && in_mask && !needs_contiguous_bounce {
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
            let raw_speed = (port.portsc >> 10) & 0xF;
            let spd = speed_label(usb_if::Speed::from_xhci_portsc(raw_speed as u8));
            let _ = write!(out, "{}({})", port.port_id, spd);
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
    PROBE_REQUESTED[controller_id].store(false, Ordering::Release);
    update_runtime_diag(controller_id, |diag| {
        diag.event_handler_ready = true;
        diag.probe_requested = false;
        diag.controller_phase = "event-ready";
        diag.root_hub_lifecycle = "running";
    });
}

fn uninstall_event_handler(controller_id: usize) {
    EVENT_HANDLER_READY[controller_id].store(false, Ordering::Release);
    *EVENT_HANDLER[controller_id].lock() = None;
    update_runtime_diag(controller_id, |diag| {
        diag.event_handler_ready = false;
        diag.controller_phase = "event-stopped";
        diag.root_hub_lifecycle = "stopped";
    });
}

async fn probe_and_bind(host: &mut USBHost, info: super::TlbUsbController, spawner: &Spawner) {
    update_runtime_diag(info.index, |diag| {
        diag.controller_phase = "probing";
        diag.probe_requested = PROBE_REQUESTED[info.index].load(Ordering::Acquire);
    });
    let devices = match host.probe_devices().await {
        Ok(devices) => devices,
        Err(_) => {
            update_runtime_diag(info.index, |diag| {
                diag.controller_phase = "probe-error";
                diag.last_probe_state = "error";
                diag.probe_fail_streak = diag.probe_fail_streak.saturating_add(1);
            });
            return;
        }
    };

    update_runtime_diag(info.index, |diag| {
        diag.controller_phase = "probe-ok";
        diag.root_hub_lifecycle = "enumerated";
        diag.last_probe_state = "ok";
        diag.last_probe_device_count = devices.len() as u32;
        diag.probe_fail_streak = 0;
        if devices.is_empty() {
            diag.empty_probe_streak = diag.empty_probe_streak.saturating_add(1);
        } else {
            diag.empty_probe_streak = 0;
        }
    });

    if usb_log_all_enabled() && !devices.is_empty() {
        crate::log!("crabusb: probe ctrl={} devices={}\n", info.index, devices.len());
    }

    let mut current_devices: Vec<ObservedUsbDevice> = Vec::new();
    for dev in devices.iter() {
        let desc = dev.descriptor();
        // Keep the observation pass non-consuming. The kmod crabusb backend
        // transfers leaf ownership on open_device(); doing that here just to
        // decorate the table with strings forces the later class bind path to
        // re-address an already-addressed physical device.
        let topo = dev.topology();
        let location = dev.location();
        current_devices.push(ObservedUsbDevice {
            backend_id: dev.id(),
            slot_id: dev.id() as u32,
            stable_id: dev.stable_id().raw(),
            root_port_id: topo.root_port_id,
            port_id: topo.port_id,
            route_string: location.route_string,
            speed: speed_label(topo.port_speed),
            vendor_id: desc.vendor_id,
            product_id: desc.product_id,
            class: desc.class,
            subclass: desc.subclass,
            protocol: desc.protocol,
            num_configurations: desc.num_configurations,
            max_packet_size_0: desc.max_packet_size_0,
            manufacturer: None,
            product: None,
            serial: None,
            configurations: collect_tlb_usb_configurations(dev.configurations()),
        });
    }
    let (connected, disconnected) = sync_observed_devices(info.index, current_devices.as_slice());
    for dev in disconnected {
        note_disconnected_device(info.index, dev);
    }
    for dev in connected {
        note_connected_device(info.index, dev);
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
                "crabusb: dev ctrl={} root_port={} vid={:04X} pid={:04X} class={:02X} subclass={:02X} proto={:02X} speed={} ifs={} eps={}\n",
                info.index,
                topo.root_port_id,
                desc.vendor_id,
                desc.product_id,
                desc.class,
                desc.subclass,
                desc.protocol,
                speed_label(topo.port_speed),
                if_count,
                ep_count
            );
            crate::log!(
                "crabusb: descriptor check ctrl={} root_port={} ok cfgs={}\n",
                info.index,
                topo.root_port_id,
                dev.configurations().len()
            );

            // Log all interfaces for mass-storage-class devices so we can
            // tell whether BOT or UAS (or neither) is available.
            for cfg in dev.configurations().iter() {
                for iface in cfg.interfaces.iter() {
                    for alt in iface.alt_settings.iter() {
                        if alt.class == 0x08 {
                            crate::log!(
                                "crabusb:   if#{} alt={} class={:02X} sub={:02X} proto={:02X} eps={}\n",
                                iface.interface_number,
                                alt.alternate_setting,
                                alt.class,
                                alt.subclass,
                                alt.protocol,
                                alt.endpoints.len()
                            );
                        }
                    }
                }
            }
        }

        let controller_id = info.index as u32;
        let mut bound_any = false;
        let mut shared_led_device = None;
        if usb_log_all_enabled()
            && desc.class == 0x03
            && super::descriptor::hid_optional_descriptor_skip_reason(
                desc.vendor_id,
                desc.product_id,
            )
            .is_none()
        {
            super::descriptor::log_hid_report_descriptors(host, dev).await;
        }
        if super::hid::leds::should_share_probe_device(dev) {
            match host.open_device(dev).await {
                Ok(mut device) => {
                    super::descriptor::log_hid_report_descriptors_on_device(&mut device, dev).await;
                    shared_led_device = Some(device);
                }
                Err(err) => {
                    crate::log!(
                        "crabusb: hid+led {:04X}:{:04X} shared open failed: {:?}\n",
                        desc.vendor_id,
                        desc.product_id,
                        err
                    );
                }
            }
        }
        if super::hid::boot::maybe_start_hid_boot_streams(
            host,
            dev,
            spawner,
            controller_id,
            shared_led_device.is_none(),
        )
        .await
        {
            bound_any = true;
        }
        if let Some(device) = shared_led_device {
            if super::hid::leds::maybe_start_led_controller_with_device(
                device,
                dev,
                spawner,
                controller_id,
            )
            .await
            {
                bound_any = true;
            }
        } else if super::hid::leds::maybe_start_led_controller(host, dev, spawner, controller_id)
            .await
        {
            bound_any = true;
        }
        if super::midi::maybe_start_midi(host, dev, spawner, controller_id).await {
            bound_any = true;
        }
        if super::video::cam::maybe_start_camera(host, dev, spawner, controller_id).await {
            bound_any = true;
        }
        let audio_started = super::sound::maybe_start_target_audio(host, dev, spawner).await;
        if audio_started {
            bound_any = true;
        }
        if !audio_started
            && super::hid::mediacontrol::maybe_start_media_control(
                host,
                dev,
                spawner,
                controller_id,
            )
            .await
        {
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

pub(crate) fn observed_device_summaries(
    controller_index: usize,
) -> Result<Vec<super::UsbDeviceSummary>, &'static str> {
    if super::controller_by_index(controller_index).is_none() {
        return Err("controller not found");
    }

    let observed = OBSERVED_DEVICES[controller_index].lock().clone();
    Ok(observed
        .into_iter()
        .map(|dev| super::UsbDeviceSummary {
            stable_id: dev.stable_id,
            slot_id: dev.slot_id,
            port: dev.port_id,
            root_port_id: dev.root_port_id,
            route_string: dev.route_string,
            kind: classify_device_kind(&dev),
            vid: Some(dev.vendor_id),
            pid: Some(dev.product_id),
            class: Some(dev.class),
            subclass: Some(dev.subclass),
            protocol: Some(dev.protocol),
            product: dev.product,
        })
        .collect())
}

pub(crate) fn observed_devices(
    controller_index: usize,
) -> Result<Vec<super::TlbUsbDevice>, &'static str> {
    if super::controller_by_index(controller_index).is_none() {
        return Err("controller not found");
    }

    let observed = OBSERVED_DEVICES[controller_index].lock().clone();
    Ok(observed
        .into_iter()
        .map(|dev| super::TlbUsbDevice {
            controller_index,
            stable_id: dev.stable_id,
            slot_id: dev.slot_id,
            root_port_id: dev.root_port_id,
            route_string: dev.route_string,
            path: Vec::new(),
            port_id: dev.port_id,
            speed: dev.speed,
            parent_hub_slot_id: None,
            hub_path: Vec::new(),
            vendor_id: dev.vendor_id,
            product_id: dev.product_id,
            class: dev.class,
            subclass: dev.subclass,
            protocol: dev.protocol,
            num_configurations: dev.num_configurations,
            max_packet_size_0: dev.max_packet_size_0,
            manufacturer: dev.manufacturer,
            product: dev.product,
            serial: dev.serial,
            configurations: dev.configurations,
        })
        .collect())
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn event_pump_task(controller_id: usize) {
    let mut idle_yields = 0u8;
    loop {
        if !EVENT_HANDLER_READY[controller_id].load(Ordering::Acquire) {
            idle_yields = 0;
            Timer::after(EmbassyDuration::from_millis(EVENT_PUMP_NOT_READY_SLEEP_MS)).await;
            continue;
        }

        let event = {
            let guard = EVENT_HANDLER[controller_id].lock();
            guard.as_ref().map(|handler| handler.handle_event())
        };

        match event {
            Some(Event::Nothing) | None => {
                if idle_yields < EVENT_PUMP_HOT_IDLE_YIELDS {
                    idle_yields = idle_yields.saturating_add(1);
                    Timer::after(EmbassyDuration::from_micros(0)).await;
                } else {
                    Timer::after(EmbassyDuration::from_micros(EVENT_PUMP_IDLE_SLEEP_US)).await;
                }
            }
            Some(Event::PortChange { port }) => {
                idle_yields = 0;
                let already_pending = PROBE_REQUESTED[controller_id].swap(true, Ordering::AcqRel);
                update_runtime_diag(controller_id, |diag| {
                    diag.probe_requested = true;
                    diag.root_port_change_seen = true;
                    diag.controller_phase = "port-change";
                    diag.root_hub_lifecycle = "changed";
                });
                if usb_log_all_enabled() && !already_pending {
                    crate::log!(
                        "crabusb: pump port change ctrl={} root_port={}\n",
                        controller_id,
                        port
                    );
                }
            }
            Some(Event::Stopped) => {
                idle_yields = 0;
                update_runtime_diag(controller_id, |diag| {
                    diag.event_handler_ready = false;
                    diag.controller_phase = "event-stopped";
                    diag.root_hub_lifecycle = "stopped";
                });
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
    const USB_BRINGUP_ENABLED: bool = true;
    const BOOT_USB_DEFER_MS: u64 = 0;
    const RETRY_MS: u64 = 1000;
    const HOTPLUG_POLL_MS: u64 = 1000;
    const INTEL_PROBE_SETTLE_MS: u64 = 0;
    const INTEL_REPROBE_MS: u64 = 1500;
    let spawner: Spawner = unsafe { Spawner::for_current_executor().await };

    loop {
        if !USB_BRINGUP_ENABLED {
            clear_observed_devices(controller_index);
            if !BOOT_DEFER_DONE[controller_index].swap(true, Ordering::AcqRel) {
                crate::log_info!(
                    target: "usb";
                    "crabusb: controller {} USB bring-up disabled; skipping host init\n",
                    controller_index
                );
            }
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        }

        if !BOOT_DEFER_DONE[controller_index].swap(true, Ordering::AcqRel) {
            crate::log_info!(
                target: "usb";
                "crabusb: controller {} boot defer before USB bring-up; waiting {}ms\n",
                controller_index,
                BOOT_USB_DEFER_MS
            );
            Timer::after(EmbassyDuration::from_millis(BOOT_USB_DEFER_MS)).await;
        }

        let Some(info) = super::controller_by_index(controller_index) else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mut host = match USBHost::new_xhci_with_pci_ids(
            info.mmio_base,
            &CRABUSB_KERNEL,
            info.vendor_id,
            info.device_id,
        ) {
            Ok(host) => host,
            Err(_) => {
                Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
                continue;
            }
        };

        if host.init().await.is_ok() {
            let connected_ports = connected_ports_summary(info.index);
            let intel_settle_probe = info.vendor_id == 0x8086;
            let mut hotplug_poll_deadline =
                Instant::now() + EmbassyDuration::from_millis(HOTPLUG_POLL_MS);
            crate::log_info!(
                target: "usb";
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
                if let Ok(token) = event_pump_task(info.index) {
                    spawner.spawn(token);
                }
                probe_and_bind(&mut host, info, &spawner).await;
            }

            loop {
                if PROBE_REQUESTED[info.index].swap(false, Ordering::AcqRel) {
                    if intel_settle_probe {
                        quiet_probe_until = Some(
                            Instant::now() + EmbassyDuration::from_millis(INTEL_PROBE_SETTLE_MS),
                        );
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
                            if let Ok(token) = event_pump_task(info.index) {
                                spawner.spawn(token);
                            }
                        }
                        probe_and_bind(&mut host, info, &spawner).await;
                        hotplug_poll_deadline =
                            Instant::now() + EmbassyDuration::from_millis(HOTPLUG_POLL_MS);
                        if !intel_settle_probe {
                            install_event_handler(info.index, host.create_event_handler());
                            if let Ok(token) = event_pump_task(info.index) {
                                spawner.spawn(token);
                            }
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
                        hotplug_poll_deadline =
                            Instant::now() + EmbassyDuration::from_millis(HOTPLUG_POLL_MS);
                    }
                }
                if quiet_probe_until.is_none() && Instant::now() >= hotplug_poll_deadline {
                    probe_and_bind(&mut host, info, &spawner).await;
                    hotplug_poll_deadline =
                        Instant::now() + EmbassyDuration::from_millis(HOTPLUG_POLL_MS);
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
        } else {
            update_runtime_diag(info.index, |diag| {
                diag.controller_phase = "init-error";
                diag.root_hub_lifecycle = "retry";
                diag.probe_fail_streak = diag.probe_fail_streak.saturating_add(1);
            });
        }

        clear_observed_devices(controller_index);
        update_runtime_diag(controller_index, |diag| {
            diag.event_handler_ready =
                EVENT_HANDLER_READY[controller_index].load(Ordering::Acquire);
            diag.probe_requested = PROBE_REQUESTED[controller_index].load(Ordering::Acquire);
            if diag.controller_phase != "init-error" {
                diag.controller_phase = "retry";
            }
            diag.root_hub_lifecycle = "retry";
        });

        Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
    }
}
