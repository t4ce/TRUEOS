mod api;
mod cdc;
pub mod class;
mod descriptor;
mod dev_gears;
pub(crate) mod heal_protocol;
mod heal_service;
pub mod hid;
pub(crate) mod lab;
mod lib;
mod mass;
mod net;
mod pen;
mod runtime;
mod scsi;
mod skhynix;

pub use self::heal_service::XhciBackendSelection;
pub use self::hid::midi;
pub use self::lib::*;
pub use crab_usb as crabusb;

const CRABUSB_CONTROLLER_ID: u32 = 3;
const MAX_CRABUSB_CONTROLLERS: usize = 2;
const HOT_RESCAN_DEBOUNCE_MS: u64 = 100;
const HOT_RESCAN_HANDOFF_SETTLE_MS: u64 = 500;
const PROBE_WATCHDOG_MS: u64 = 2_000;
const PROBE_RETRY_MS: u64 = 1_000;
static USB_PORT_CHANGE_SEQ: [core::sync::atomic::AtomicU32; MAX_CRABUSB_CONTROLLERS] =
    [const { core::sync::atomic::AtomicU32::new(0) }; MAX_CRABUSB_CONTROLLERS];
// Emergency BSP isolation switch. Keep this runtime-visible so the complete
// USB path remains linked when temporarily taking the controller offline.
static BSP_HEADLESS_SKIP_CRABUSB_XHCI_CLAIM: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[trueos_executor::task]
pub async fn usb_controller_service_task() {
    // Keep shared HID/storage ABI types available, but never claim an xHCI
    // controller when USB was deselected for this build.
    if !cfg!(feature = "subsystem-usb") {
        crate::log!("crabusb: disabled by build selection; xhci remains unclaimed\n");
        return;
    }
    if BSP_HEADLESS_SKIP_CRABUSB_XHCI_CLAIM.load(core::sync::atomic::Ordering::Acquire) {
        crate::log!(
            "crabusb: BSP headless boot experiment enabled; xhci remains enumerated and unclaimed\n"
        );
        return;
    }
    let controllers = crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .filter(|dev| dev.class == 0x0c && dev.subclass == 0x03 && dev.prog_if == 0x30)
            .take(MAX_CRABUSB_CONTROLLERS)
            .collect::<alloc::vec::Vec<_>>()
    });
    let Some(&first) = controllers.first() else {
        return;
    };
    let spawner: trueos_executor::Spawner =
        unsafe { trueos_executor::Spawner::for_current_executor().await };
    start_device_pool_worker(&spawner);
    for (index, &device) in controllers.iter().enumerate().skip(1) {
        match usb_additional_controller_task(index, device) {
            Ok(token) => spawner.spawn(token),
            Err(err) => {
                controller_failure(
                    index,
                    "task-allocation",
                    alloc::format!("controller task allocation failed: {err:?}"),
                );
                crate::log!(
                    "crabusb: controller task allocation failed pci={:02x}:{:02x}.{} error={:?}\n",
                    device.bus,
                    device.slot,
                    device.function,
                    err
                );
            }
        }
    }
    run_usb_controller(0, first).await;
}

#[trueos_executor::task]
async fn usb_additional_controller_task(index: usize, device: crate::pci::PciDevice) {
    run_usb_controller(index, device).await;
}

fn start_device_pool_worker(spawner: &trueos_executor::Spawner) {
    let device_pool_token = match dev_gears::usb_device_pool_worker_task() {
        Ok(token) => token,
        Err(err) => {
            crate::log!(
                "crabusb: device pool worker task allocation failed error={:?}\n",
                err
            );
            return;
        }
    };
    if let Some((worker_slot, core_kind, worker_spawner)) =
        crate::workers::pick_eff_background_spawner_with_slot()
    {
        worker_spawner.spawn(device_pool_token);
        crate::log!(
            "crabusb: device pool worker started placement=ecore slot={} core_kind={}\n",
            worker_slot,
            core_kind
        );
    } else {
        spawner.spawn(device_pool_token);
        crate::log!(
            "crabusb: device pool worker started placement=bsp-fallback reason=no-eff-worker\n"
        );
    }
}

async fn run_usb_controller(index: usize, xhci_device: crate::pci::PciDevice) {
    controller_stage(index, "admission", "unclaimed");
    let selection = match heal_service::claim_backend(xhci_device, index == 0) {
        Ok(selection) => selection,
        Err(reason) => {
            controller_failure(index, "claim-failed", reason);
            return;
        }
    };
    let controller_id = CRABUSB_CONTROLLER_ID + index as u32;
    crate::log!(
        "crabusb: controller index={} id={} pci={:02x}:{:02x}.{} claimed\n",
        index,
        controller_id,
        xhci_device.bus,
        xhci_device.slot,
        xhci_device.function
    );
    controller_stage(index, "mapping", "claimed");
    let (mmio, mmio_len, kernel) = match lib::map_xhci_host_inputs(&xhci_device) {
        Ok(inputs) => inputs,
        Err(error) => {
            controller_failure(index, "map-failed", error);
            return;
        }
    };
    let root_hub_policy = match selection {
        XhciBackendSelection::KnownIntel(profile) => {
            crate::log!(
                "crabusb: xhci backend=known-intel profile={} normal-init=admitted\n",
                profile.label()
            );
            if matches!(xhci_device.device_id, 0x9a13 | 0xa0ed) {
                crabusb::XhciRootHubInitPolicy::FullAllPorts
            } else {
                crabusb::XhciRootHubInitPolicy::SelectivePorts3And4Skip11
            }
        }
        XhciBackendSelection::KnownQemu(profile) => {
            crate::log!(
                "crabusb: xhci backend=known-qemu profile={} normal-init=admitted\n",
                profile.label()
            );
            crabusb::XhciRootHubInitPolicy::FullAllPorts
        }
        XhciBackendSelection::HealRequired(seed) => {
            controller_stage(index, "heal-quarantined", "quarantined");
            heal_service::run_quarantined(index, mmio, mmio_len, kernel, seed).await
        }
    };
    controller_stage(index, "constructing", "claimed");
    let mut host = match crabusb::USBHost::new_xhci_with_root_hub_init_policy_and_mmio_len(
        mmio,
        mmio_len,
        kernel,
        root_hub_policy,
    ) {
        Ok(host) => host,
        Err(err) => {
            controller_failure(
                index,
                "construction-failed",
                alloc::format!("controller construction failed: {err:?}"),
            );
            crate::log!(
                "crabusb: controller index={} construction failed error={:?}\n",
                index,
                err
            );
            return;
        }
    };
    let _ = lab::refresh_snapshot(index, &mut host).await;
    controller_stage(index, "initializing", "initializing");
    let _ =
        heal_service::register_pending_controller(index, xhci_device, &mut host, "initializing")
            .await;
    if let Err(err) = await_controller_operation(index, "initializing", host.init()).await {
        crate::log!(
            "crabusb: controller index={} init failed error={:?}\n",
            index,
            err
        );
        let reason = alloc::format!("controller initialization failed: {err:?}");
        controller_failure(index, "init-failed", reason.clone());
        park_failed_controller(index, xhci_device, &mut host, reason).await;
    }

    // Keep an owner reference to the event ring even if task allocation fails
    // or the consumer stops; hardware may still hold its DMA address.
    let event_handler = alloc::sync::Arc::new(host.create_event_handler());
    let spawner: trueos_executor::Spawner =
        unsafe { trueos_executor::Spawner::for_current_executor().await };
    let event_pump_token = match usb_event_pump_task(index, event_handler.clone()) {
        Ok(token) => token,
        Err(err) => {
            crate::log!(
                "crabusb: controller index={} event pump task allocation failed error={:?}\n",
                index,
                err
            );
            let reason = alloc::format!("event pump task allocation failed: {err:?}");
            controller_failure(index, "event-allocation-failed", reason.clone());
            park_failed_controller(index, xhci_device, &mut host, reason).await;
        }
    };
    spawner.spawn(event_pump_token);
    crate::log!("crabusb: controller index={} event pump started\n", index);
    controller_stage(index, "discovering", "active");
    if let Err(reason) = lab::refresh_snapshot(index, &mut host).await {
        crate::log_important!(target: "usb"; "crabusb: controller={} initial snapshot failed reason={}\n", index, reason);
    }
    let mut heal_report =
        heal_service::register_live_controller(index, xhci_device, &mut host).await;
    let mut retry_needed = false;
    if let Some(news) = probe_devices_with_log(&mut host, "initial", index).await {
        await_controller_operation(
            index,
            "handoff",
            open_and_handoff_devices(&mut host, news, &spawner, controller_id),
        )
        .await;
    } else {
        retry_needed = true;
    }

    // The first root-hub pass can only *start* a USB2 port reset. Its
    // completion may arrive during device handoff, before we establish the
    // maintenance event baseline below. Finish those boot-time transitions
    // explicitly instead of waiting for another physical hotplug edge.
    for _ in 0..3 {
        trueos_time::Timer::after(trueos_time::Duration::from_millis(150)).await;
        if let Some(news) = probe_devices_with_log(&mut host, "initial-settle", index).await {
            await_controller_operation(
                index,
                "handoff",
                open_and_handoff_devices(&mut host, news, &spawner, controller_id),
            )
            .await;
            retry_needed = false;
        } else {
            retry_needed = true;
        }
    }

    // Keep boot reset/change events pending for the maintenance pass too.
    let mut observed_port_change_seq = 0;
    let mut next_snapshot = trueos_time::Instant::now()
        + trueos_time::Duration::from_millis(crate::allcaps::usb::CONTROLLER_SNAPSHOT_CADENCE_MS);
    let mut next_retry =
        trueos_time::Instant::now() + trueos_time::Duration::from_millis(PROBE_RETRY_MS);
    loop {
        if index == 0 {
            USB_LEGENDARY_LEGACY_SAVEWRAPPER_LMAO(&mut host).await;
        } else {
            trueos_time::Timer::after(trueos_time::Duration::from_millis(
                crate::allcaps::usb::CONTROLLER_MAINTENANCE_CADENCE_MS,
            ))
            .await;
        }
        let _ = heal_protocol::service_one(&mut host, &mut heal_report).await;
        let heal_rescan = heal_service::take_rescan_requested(index);
        retry_needed |= heal_rescan;
        if trueos_time::Instant::now() >= next_snapshot {
            next_snapshot = trueos_time::Instant::now()
                + trueos_time::Duration::from_millis(
                    crate::allcaps::usb::CONTROLLER_SNAPSHOT_CADENCE_MS,
                );
            if let Err(reason) = lab::refresh_snapshot(index, &mut host).await {
                crate::log_trace!(
                    target: "usb";
                    "crabusb: periodic xhci wisdom snapshot failed reason={}\n",
                    reason
                );
            }
        }
        let next_port_change_seq =
            USB_PORT_CHANGE_SEQ[index].load(core::sync::atomic::Ordering::Acquire);
        let retry_due = retry_needed && trueos_time::Instant::now() >= next_retry;
        if next_port_change_seq == observed_port_change_seq && !retry_due && !heal_rescan {
            continue;
        }
        if retry_needed && !retry_due {
            continue;
        }
        if !runtime::snapshot(index).event_ready {
            // Do not submit commands after the sole event consumer stopped.
            // Keep the host's DMA allocations and diagnostic/Heal owner alive.
            continue;
        }
        let quarantine = match lab::enter_controller_quarantine().await {
            Ok(guard) => guard,
            Err(reason) => {
                runtime::update(index, |runtime| {
                    runtime.failure("rescan-quarantine", reason.clone())
                });
                retry_needed = true;
                next_retry = trueos_time::Instant::now()
                    + trueos_time::Duration::from_millis(PROBE_RETRY_MS);
                crate::log!(
                    "crabusb: probe_devices trigger=port-change seq={} quarantine-error={}\n",
                    observed_port_change_seq,
                    reason
                );
                continue;
            }
        };
        // Only consume the sequence after normal probing has actually been
        // admitted. A failed quarantine must leave the rescan pending.
        trueos_time::Timer::after(trueos_time::Duration::from_millis(HOT_RESCAN_DEBOUNCE_MS)).await;
        // Keep events arriving during this probe pending. Consuming the final
        // global sequence used to lose reset completions and real hotplugs.
        let probing_port_change_seq =
            USB_PORT_CHANGE_SEQ[index].load(core::sync::atomic::Ordering::Acquire);
        crate::log!(
            "crabusb: probe_devices trigger=port-change seq={} quarantine=active\n",
            probing_port_change_seq
        );
        retry_needed = true;
        if let Some(news) = probe_devices_with_log(&mut host, "rescan", index).await {
            retry_needed = false;
            observed_port_change_seq = probing_port_change_seq;
            if !news.is_empty() {
                await_controller_operation(
                    index,
                    "handoff",
                    open_and_handoff_devices(&mut host, news, &spawner, controller_id),
                )
                .await;
                trueos_time::Timer::after(trueos_time::Duration::from_millis(
                    HOT_RESCAN_HANDOFF_SETTLE_MS,
                ))
                .await;
            }
        }
        next_retry =
            trueos_time::Instant::now() + trueos_time::Duration::from_millis(PROBE_RETRY_MS);
        drop(quarantine);
        crate::log!(
            "crabusb: probe_devices trigger=port-change seq={} quarantine=released\n",
            observed_port_change_seq
        );
    }
}

fn controller_stage(index: usize, phase: &'static str, lifecycle: &'static str) {
    runtime::update(index, |runtime| runtime.stage(phase, lifecycle));
    crate::log_important!(target: "usb"; "crabusb: controller={} phase={} lifecycle={}\n", index, phase, lifecycle);
}

fn controller_failure(index: usize, stage: &'static str, reason: alloc::string::String) {
    crate::log_important!(target: "usb"; "crabusb: controller={} stage={} failure={}\n", index, stage, reason);
    runtime::update(index, |runtime| {
        runtime.stage(stage, "failed");
        runtime.failure(stage, reason);
    });
}

async fn park_failed_controller(
    index: usize,
    device: crate::pci::PciDevice,
    host: &mut crabusb::USBHost,
    reason: alloc::string::String,
) -> ! {
    // A partial initialization may already have exposed DMA addresses to the
    // controller. Retain its owner and rings instead of dropping a live host.
    let mut report = heal_service::register_failed_controller(index, device, host, reason).await;
    loop {
        let _ = lab::refresh_snapshot(index, host).await;
        let _ = heal_protocol::service_one(host, &mut report).await;
        trueos_time::Timer::after(trueos_time::Duration::from_millis(250)).await;
    }
}

#[allow(non_snake_case)]
async fn USB_LEGENDARY_LEGACY_SAVEWRAPPER_LMAO(_host: &mut crabusb::USBHost) {
    trueos_time::Timer::after(trueos_time::Duration::from_millis(
        crate::allcaps::usb::CONTROLLER_MAINTENANCE_CADENCE_MS,
    ))
    .await;
}

async fn probe_devices_with_log(
    host: &mut crabusb::USBHost,
    label: &'static str,
    controller_index: usize,
) -> Option<alloc::vec::Vec<crabusb::ProbedDevice>> {
    runtime::update(controller_index, |runtime| {
        runtime.probe_started(label, trueos_time::Instant::now().as_millis())
    });
    crate::log_important!(target: "usb"; "crabusb: controller={} probe label={} state=started\n", controller_index, label);
    let result = await_probe_without_cancel(controller_index, host.probe_devices()).await;
    let now_ms = trueos_time::Instant::now().as_millis();
    let news = match result {
        Ok(news) => {
            runtime::update(controller_index, |runtime| {
                runtime.probe_finished(Ok(news.len()), now_ms)
            });
            lib::observe_probed_devices(controller_index, label, &news);
            let runtime = runtime::snapshot(controller_index);
            crate::log_important!(target: "usb";
                "crabusb: controller={} probe label={} state={} count={} elapsed_ms={} attempts={} successes={} failures={} watchdogs={}\n",
                controller_index, label, runtime.last_probe_state, news.len(), runtime.last_probe_elapsed_ms,
                runtime.probe_attempts, runtime.probe_successes, runtime.probe_failures, runtime.probe_watchdog_timeouts);
            Some(news)
        }
        Err(error) => {
            let reason = alloc::format!("probe label={label} failed: {error:?}");
            runtime::update(controller_index, |runtime| {
                runtime.probe_finished(Err(reason.clone()), now_ms)
            });
            crate::log_important!(target: "usb"; "crabusb: controller={} {} owner=retained event-pump=retained\n", controller_index, reason);
            None
        }
    };
    let _ = lab::refresh_snapshot(controller_index, host).await;
    news
}

async fn await_probe_without_cancel<F: core::future::Future>(
    controller_index: usize,
    probe: F,
) -> F::Output {
    // Enumeration can already have submitted commands/transfers and exposed
    // DMA buffers. A watchdog may drop its borrowed projection, never the
    // operation itself; retain the same future until completion.
    let mut probe = core::pin::pin!(probe);
    loop {
        match trueos_time::with_timeout(
            trueos_time::Duration::from_millis(PROBE_WATCHDOG_MS),
            probe.as_mut(),
        )
        .await
        {
            Ok(result) => return result,
            Err(_) => {
                runtime::update(controller_index, |runtime| {
                    runtime.probe_watchdog(trueos_time::Instant::now().as_millis())
                });
                let runtime = runtime::snapshot(controller_index);
                if runtime.probe_watchdog_timeouts == 1
                    || runtime.probe_watchdog_timeouts.is_multiple_of(5)
                {
                    crate::log_important!(target: "usb";
                        "crabusb: controller={} probe state=pending elapsed_ms={} watchdogs={} event_ready={} action=retain-operation-and-dma\n",
                        controller_index, runtime.last_probe_elapsed_ms, runtime.probe_watchdog_timeouts, runtime.event_ready);
                }
            }
        }
    }
}

async fn await_controller_operation<F: core::future::Future>(
    controller_index: usize,
    stage: &'static str,
    operation: F,
) -> F::Output {
    runtime::update(controller_index, |runtime| {
        runtime.operation_started(stage, trueos_time::Instant::now().as_millis())
    });
    let mut operation = core::pin::pin!(operation);
    loop {
        match trueos_time::with_timeout(
            trueos_time::Duration::from_millis(PROBE_WATCHDOG_MS),
            operation.as_mut(),
        )
        .await
        {
            Ok(result) => {
                runtime::update(controller_index, |runtime| {
                    runtime.operation_finished(trueos_time::Instant::now().as_millis())
                });
                return result;
            }
            Err(_) => {
                runtime::update(controller_index, |runtime| {
                    runtime.operation_watchdog(trueos_time::Instant::now().as_millis())
                });
                let runtime = runtime::snapshot(controller_index);
                if runtime.operation_watchdog_timeouts == 1
                    || runtime.operation_watchdog_timeouts.is_multiple_of(5)
                {
                    crate::log_important!(target: "usb";
                        "crabusb: controller={} operation={} state=pending elapsed_ms={} watchdogs={} action=retain-operation-and-dma\n",
                        controller_index, stage, runtime.operation_elapsed_ms, runtime.operation_watchdog_timeouts);
                }
            }
        }
    }
}

async fn open_and_handoff_devices(
    host: &mut crabusb::USBHost,
    news: alloc::vec::Vec<crabusb::ProbedDevice>,
    spawner: &trueos_executor::Spawner,
    controller_id: u32,
) {
    for new in news {
        log_probed_device("probed", &new);
        match new {
            crabusb::ProbedDevice::Device(info) => {
                let desc = info.descriptor();
                let vendor_id = desc.vendor_id;
                let product_id = desc.product_id;
                if net::maybe_start(host, &info, spawner, controller_id - CRABUSB_CONTROLLER_ID)
                    .await
                {
                    continue;
                }
                if hid::boot::maybe_start_hid_boot_streams(
                    host,
                    &info,
                    spawner,
                    controller_id,
                    false,
                )
                .await
                {
                    continue;
                }

                if hid::midi::maybe_start_midi(host, &info, spawner, controller_id).await {
                    continue;
                }

                if (vendor_id != 0x152e || product_id != 0x7001)
                    && pen::maybe_start_mass_storage(host, &info, spawner, controller_id).await
                {
                    continue;
                }

                if vendor_id != 0x152e || product_id != 0x7001 {
                    crate::log!(
                        "crabusb: device id={} ignored reason=no-usb3-driver vid={:04x} pid={:04x}\n",
                        info.id(),
                        vendor_id,
                        product_id
                    );
                    continue;
                }

                let device = match host.open_device(&info).await {
                    Ok(device) => device,
                    Err(err) => {
                        crate::log!(
                            "crabusb: normal device open failed id={} vid={:04x} pid={:04x} error={:?}\n",
                            info.id(),
                            vendor_id,
                            product_id,
                            err
                        );
                        continue;
                    }
                };
                let id = device.slot_id() as usize;
                match dev_gears::handoff_opened_device(device) {
                    Ok(pool_len) => {
                        crate::log!(
                            "crabusb: normal device opened id={} handed_to_pool pool_len={}\n",
                            id,
                            pool_len
                        );
                    }
                    Err(device) => {
                        crate::log!(
                            "crabusb: normal device opened id={} dropped reason=device_pool_full cap={}\n",
                            device.slot_id(),
                            dev_gears::USB_DEVICE_POOL_CAP
                        );
                    }
                }
            }

            crabusb::ProbedDevice::Hub(hub) => {
                log_hub_device_info(&hub);
            }
        }
    }
}

fn log_probed_device(label: &str, probed: &crabusb::ProbedDevice) {
    let desc = probed.descriptor();
    crate::log!(
        "crabusb: {} id={} vid={:04x} pid={:04x} class={:02x}:{:02x}:{:02x} configs={}\n",
        label,
        probed.id(),
        desc.vendor_id,
        desc.product_id,
        desc.class,
        desc.subclass,
        desc.protocol,
        probed.configurations().len()
    );
}

fn log_hub_device_info(hub: &crabusb::HubDeviceInfo) {
    let desc = hub.descriptor();
    crate::log!(
        "crabusb: hub device id={} vid={:04x} pid={:04x} class={:02x}:{:02x}:{:02x} configs={}\n",
        hub.id(),
        desc.vendor_id,
        desc.product_id,
        desc.class,
        desc.subclass,
        desc.protocol,
        hub.configurations().len()
    );
}

// Each controller retains its pump for its lifetime; a single task slot keeps
// the second controller from reaching device discovery.
#[trueos_executor::task(pool_size = MAX_CRABUSB_CONTROLLERS)]
pub async fn usb_event_pump_task(index: usize, handler: alloc::sync::Arc<crabusb::EventHandler>) {
    runtime::update(index, |runtime| runtime.event_ready = true);
    crate::log_important!(target: "usb"; "crabusb: controller={} event pump state=running\n", index);
    let mut last_transfer_activity_count = None;
    loop {
        let mut active = false;
        for _ in 0..64 {
            match handler.handle_event() {
                crabusb::Event::Nothing => break,
                crabusb::Event::PortChange { port } => {
                    active = true;
                    USB_PORT_CHANGE_SEQ[index].fetch_add(1, core::sync::atomic::Ordering::AcqRel);
                    runtime::update(index, |runtime| runtime.root_port_change_seen = true);
                    crate::log!(
                        "crabusb: event controller={} port-change port={}\n",
                        index,
                        port
                    );
                }
                crabusb::Event::TransferActivity { count } => {
                    active = true;
                    if crate::log_os::flags::USB_MASS_UAS_TRACE_LOGS
                        && last_transfer_activity_count != Some(count)
                    {
                        last_transfer_activity_count = Some(count);
                        crate::log_trace!(target: "usb";
                            "crabusb: event transfer-activity count={}\n",
                            count
                        );
                    }
                }
                crabusb::Event::Stopped => {
                    runtime::update(index, |runtime| runtime.pump_stopped());
                    crate::log_important!(target: "usb"; "crabusb: controller={} event pump state=stopped\n", index);
                    return;
                }
            }
        }

        if active {
            trueos_time::Timer::after(trueos_time::Duration::from_micros(0)).await;
        } else {
            trueos_time::Timer::after(trueos_time::Duration::from_micros(50)).await;
        }
    }
}
