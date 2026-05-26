mod api;
pub mod class;
mod dev_gears;
mod descriptor;
pub mod hid;
mod lib;

pub use self::hid::{hut, input, midi};
pub use self::lib::*;
pub use crab_usb as crabusb;

const CRABUSB_CONTROLLER_ID: u32 = 3;

#[embassy_executor::task]
pub async fn usb_controller_service_task() {
    let Some((mmio, kernel)) = lib::known_xhci_host_inputs() else {
        return;
    };
    let mut host = crabusb::USBHost::new_xhci(mmio, kernel).expect("crabusb xhci host");
    host.init().await.expect("crabusb xhci init");

    let event_handler = host.create_event_handler();
    let spawner: embassy_executor::Spawner =
        unsafe { embassy_executor::Spawner::for_current_executor().await };
    spawner.spawn(usb_event_pump_task(event_handler).expect("crabusb event pump token"));
    crate::log!("crabusb: event pump started\n");
    spawner
        .spawn(dev_gears::usb_device_pool_worker_task().expect("crabusb device pool worker token"));
    crate::log!("crabusb: device pool worker started\n");
    spawner
        .spawn(dev_gears::usb_boot_mouse_worker_task().expect("crabusb boot mouse worker token"));
    crate::log!("crabusb: boot mouse worker started\n");

    let Some(news) = probe_devices_with_log(&mut host, "initial").await else {
        return;
    };
    open_and_handoff_devices(&mut host, news, &spawner).await;
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
    crate::log!("crabusb: probe_devices label={} count={}\n", label, news.len());
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
                let handoff_to_gears =
                    dev_gears::has_boot_mouse_transport(info.configurations())
                        || !hid::boot::maybe_start_hid_boot_streams(
                            host,
                            &info,
                            spawner,
                            CRABUSB_CONTROLLER_ID,
                            true,
                        )
                        .await;
                if !handoff_to_gears {
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
