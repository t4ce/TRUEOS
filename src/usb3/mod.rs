mod api;
pub mod class;
mod descriptor;
mod dev_gears;
pub mod hid;
pub(crate) mod lab;
mod lib;
mod mass;
mod pen;
mod scsi;
mod skhynix;

pub use self::hid::midi;
pub use self::lib::*;
pub use crab_usb as crabusb;

const CRABUSB_CONTROLLER_ID: u32 = 3;
const HOT_RESCAN_DEBOUNCE_MS: u64 = 100;
const HOT_RESCAN_HANDOFF_SETTLE_MS: u64 = 500;
const TEMPORARY_SKHYNIX_FS_RESCAN_READY: u32 =
    crate::r::readiness::TRUEOSFS_ROOT_MOUNTED | crate::r::readiness::TRUEOSFS_INDEX_READY;
static USB_PORT_CHANGE_SEQ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
// Emergency BSP isolation switch. Keep this runtime-visible so the complete
// USB path remains linked when temporarily taking the controller offline.
static BSP_HEADLESS_SKIP_CRABUSB_XHCI_CLAIM: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

#[embassy_executor::task]
pub async fn usb_controller_service_task() {
    if BSP_HEADLESS_SKIP_CRABUSB_XHCI_CLAIM.load(core::sync::atomic::Ordering::Acquire) {
        crate::log!(
            "crabusb: BSP headless boot experiment enabled; xhci remains enumerated and unclaimed\n"
        );
        return;
    }
    let Some((mmio, mmio_len, kernel, root_hub_policy)) = lib::known_xhci_host_inputs() else {
        return;
    };
    let mut host = match crabusb::USBHost::new_xhci_with_root_hub_init_policy_and_mmio_len(
        mmio,
        mmio_len,
        kernel,
        root_hub_policy,
    ) {
        Ok(host) => host,
        Err(err) => {
            crate::log!("crabusb: controller construction failed error={:?}\n", err);
            return;
        }
    };
    if let Err(err) = host.init().await {
        crate::log!("crabusb: controller init failed error={:?}\n", err);
        return;
    }

    let event_handler = host.create_event_handler();
    let spawner: embassy_executor::Spawner =
        unsafe { embassy_executor::Spawner::for_current_executor().await };
    let event_pump_token = match usb_event_pump_task(event_handler) {
        Ok(token) => token,
        Err(err) => {
            crate::log!("crabusb: event pump task allocation failed error={:?}\n", err);
            return;
        }
    };
    spawner.spawn(event_pump_token);
    crate::log!("crabusb: event pump started\n");
    if let Err(reason) = lab::refresh_snapshot(&mut host).await {
        crate::log!("crabusb: initial xhci wisdom snapshot failed reason={}\n", reason);
    }
    let device_pool_token = match dev_gears::usb_device_pool_worker_task() {
        Ok(token) => token,
        Err(err) => {
            crate::log!("crabusb: device pool worker task allocation failed error={:?}\n", err);
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

    let Some(news) = probe_devices_with_log(&mut host, "initial").await else {
        return;
    };
    open_and_handoff_devices(&mut host, news, &spawner).await;

    let mut observed_port_change_seq =
        USB_PORT_CHANGE_SEQ.load(core::sync::atomic::Ordering::Acquire);
    let mut next_snapshot = embassy_time::Instant::now()
        + embassy_time::Duration::from_millis(crate::allcaps::usb::CONTROLLER_SNAPSHOT_CADENCE_MS);
    loop {
        USB_LEGENDARY_LEGACY_SAVEWRAPPER_LMAO(&mut host).await;
        if embassy_time::Instant::now() >= next_snapshot {
            next_snapshot = embassy_time::Instant::now()
                + embassy_time::Duration::from_millis(
                    crate::allcaps::usb::CONTROLLER_SNAPSHOT_CADENCE_MS,
                );
            if let Err(reason) = lab::refresh_snapshot(&mut host).await {
                crate::log_trace!(
                    target: "usb";
                    "crabusb: periodic xhci wisdom snapshot failed reason={}\n",
                    reason
                );
            }
        }
        let next_port_change_seq = USB_PORT_CHANGE_SEQ.load(core::sync::atomic::Ordering::Acquire);
        if next_port_change_seq == observed_port_change_seq {
            continue;
        }

        // TEMPORARY boot-ordering bridge, not a USB device dependency: the
        // current SKHynix filesystem replay can outlive the normal quarantine
        // timeout. Retain the pending port change until that replay publishes
        // its root and index, then let every waiting USB device probe normally.
        if !crate::r::readiness::is_set(TEMPORARY_SKHYNIX_FS_RESCAN_READY) {
            crate::log_warn!(target: "usb";
                "crabusb: temporary rescan gate waiting for SKHynix-backed TRUEOSFS root+index readiness; HID/other USB devices are not functionally dependent on SKHynix action=retain-pending-port-change\n"
            );
            crate::r::readiness::wait_for(TEMPORARY_SKHYNIX_FS_RESCAN_READY).await;
            crate::log!(
                "crabusb: temporary rescan gate released by TRUEOSFS root+index readiness action=resume-normal-usb-probe\n"
            );
        }
        let quarantine = match lab::enter_controller_quarantine().await {
            Ok(guard) => guard,
            Err(reason) => {
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
        observed_port_change_seq = next_port_change_seq;
        embassy_time::Timer::after(embassy_time::Duration::from_millis(HOT_RESCAN_DEBOUNCE_MS))
            .await;
        crate::log!(
            "crabusb: probe_devices trigger=port-change seq={} quarantine=active\n",
            observed_port_change_seq
        );
        if let Some(news) = probe_devices_with_log(&mut host, "rescan").await {
            if !news.is_empty() {
                open_and_handoff_devices(&mut host, news, &spawner).await;
                embassy_time::Timer::after(embassy_time::Duration::from_millis(
                    HOT_RESCAN_HANDOFF_SETTLE_MS,
                ))
                .await;
            }
        }
        drop(quarantine);
        // Port reset/probe work emits its own xHCI change events. Consume that
        // resulting sequence here so maintenance cannot recursively rescan itself.
        observed_port_change_seq = USB_PORT_CHANGE_SEQ.load(core::sync::atomic::Ordering::Acquire);
        crate::log!(
            "crabusb: probe_devices trigger=port-change seq={} quarantine=released\n",
            observed_port_change_seq
        );
    }
}

#[allow(non_snake_case)]
async fn USB_LEGENDARY_LEGACY_SAVEWRAPPER_LMAO(host: &mut crabusb::USBHost) {
    let started = embassy_time::Instant::now();
    let budget =
        embassy_time::Duration::from_millis(crate::allcaps::usb::CONTROLLER_MAINTENANCE_BUDGET_MS);
    while lab::service_one(host).await {
        if embassy_time::Instant::now().duration_since(started) >= budget {
            break;
        }
    }
    embassy_time::Timer::after(embassy_time::Duration::from_millis(
        crate::allcaps::usb::CONTROLLER_MAINTENANCE_CADENCE_MS,
    ))
    .await;
}

async fn probe_devices_with_log(
    host: &mut crabusb::USBHost,
    label: &'static str,
) -> Option<alloc::vec::Vec<crabusb::ProbedDevice>> {
    let news = match embassy_time::with_timeout(
        embassy_time::Duration::from_secs(2),
        host.probe_devices(),
    )
    .await
    {
        Ok(Ok(news)) => news,
        Ok(Err(err)) => {
            crate::log!("crabusb: probe_devices label={} error={:?}\n", label, err);
            return None;
        }
        Err(_) => {
            crate::log!(
                "crabusb: probe_devices label={} timeout waiting for xhci completion\n",
                label
            );
            return None;
        }
    };
    if label == "initial" || !news.is_empty() {
        crate::log!("crabusb: probe_devices label={} count={}\n", label, news.len());
    }
    lib::observe_probed_devices(label, &news);
    Some(news)
}

async fn open_and_handoff_devices(
    host: &mut crabusb::USBHost,
    news: alloc::vec::Vec<crabusb::ProbedDevice>,
    spawner: &embassy_executor::Spawner,
) {
    for new in news {
        log_probed_device("probed", &new);
        match new {
            crabusb::ProbedDevice::Device(info) => {
                let desc = info.descriptor();
                let vendor_id = desc.vendor_id;
                let product_id = desc.product_id;
                if hid::boot::maybe_start_hid_boot_streams(
                    host,
                    &info,
                    spawner,
                    CRABUSB_CONTROLLER_ID,
                    false,
                )
                .await
                {
                    continue;
                }

                if hid::midi::maybe_start_midi(host, &info, spawner, CRABUSB_CONTROLLER_ID).await {
                    continue;
                }

                if (vendor_id != 0x152e || product_id != 0x7001)
                    && pen::maybe_start_mass_storage(host, &info, spawner, CRABUSB_CONTROLLER_ID)
                        .await
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

#[embassy_executor::task]
pub async fn usb_event_pump_task(handler: crabusb::EventHandler) {
    let mut last_transfer_activity_count = None;
    loop {
        let mut active = false;
        for _ in 0..64 {
            match handler.handle_event() {
                crabusb::Event::Nothing => break,
                crabusb::Event::PortChange { port } => {
                    active = true;
                    USB_PORT_CHANGE_SEQ.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
                    crate::log!("crabusb: event port-change port={}\n", port);
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
                    crate::log!("crabusb: event pump stopped\n");
                    return;
                }
            }
        }

        if active {
            embassy_time::Timer::after(embassy_time::Duration::from_micros(0)).await;
        } else {
            embassy_time::Timer::after(embassy_time::Duration::from_micros(50)).await;
        }
    }
}
