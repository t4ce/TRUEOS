mod api;
pub mod class;
mod descriptor;
mod dev_gears;
pub mod hid;
mod lib;
mod skhynix;

pub use self::hid::{hut, input, midi};
pub use self::lib::*;
pub use crab_usb as crabusb;

const CRABUSB_CONTROLLER_ID: u32 = 3;
static USB_PORT_CHANGE_SEQ: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

#[embassy_executor::task]
pub async fn usb_controller_service_task() {
    let Some((mmio, kernel, root_hub_policy)) = lib::known_xhci_host_inputs() else {
        return;
    };
    let mut host =
        crabusb::USBHost::new_xhci_with_root_hub_init_policy(mmio, kernel, root_hub_policy)
            .expect("crabusb xhci host");
    host.init().await.expect("crabusb xhci init");

    let event_handler = host.create_event_handler();
    let spawner: embassy_executor::Spawner =
        unsafe { embassy_executor::Spawner::for_current_executor().await };
    spawner.spawn(usb_event_pump_task(event_handler).expect("crabusb event pump token"));
    crate::log!("crabusb: event pump started\n");
    spawner
        .spawn(dev_gears::usb_device_pool_worker_task().expect("crabusb device pool worker token"));
    crate::log!("crabusb: device pool worker started\n");

    let Some(news) = probe_devices_with_log(&mut host, "initial").await else {
        return;
    };
    open_and_handoff_devices(&mut host, news, &spawner).await;

    let mut observed_port_change_seq =
        USB_PORT_CHANGE_SEQ.load(core::sync::atomic::Ordering::Acquire);
    loop {
        embassy_time::Timer::after(embassy_time::Duration::from_millis(25)).await;
        let next_port_change_seq = USB_PORT_CHANGE_SEQ.load(core::sync::atomic::Ordering::Acquire);
        if next_port_change_seq == observed_port_change_seq {
            continue;
        }
        observed_port_change_seq = next_port_change_seq;
        embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
        crate::log!(
            "crabusb: probe_devices trigger=port-change seq={}\n",
            observed_port_change_seq
        );
        let Some(news) = probe_devices_with_log(&mut host, "rescan").await else {
            continue;
        };
        if news.is_empty() {
            continue;
        }
        open_and_handoff_devices(&mut host, news, &spawner).await;
    }
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

                if vendor_id != 0x152e || product_id != 0x7001 {
                    crate::log!(
                        "crabusb: device id={} ignored reason=no-usb3-driver vid={:04x} pid={:04x}\n",
                        info.id(),
                        vendor_id,
                        product_id
                    );
                    continue;
                }

                let device = host.open_device(&info).await.expect("crabusb open device");
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
                    if crate::logflag::USB_MASS_UAS_TRACE_LOGS {
                        crate::log!("crabusb: event transfer-activity count={}\n", count);
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
