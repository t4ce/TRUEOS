mod lib;

pub use crab_usb as crabusb;
pub use self::lib::*;

#[embassy_executor::task]
pub async fn usb_controller_service_task() {
    let Some((mmio, kernel)) = lib::known_xhci_host_inputs() else { return };
    let mut host = crabusb::USBHost::new_xhci(mmio, kernel).expect("crabusb xhci host");
    let event_handler = host.create_event_handler();
    let spawner: embassy_executor::Spawner =
        unsafe { embassy_executor::Spawner::for_current_executor().await };
    spawner.spawn(usb_event_pump_task(event_handler).expect("crabusb event pump token"));
    crate::log!("crabusb: event pump started\n");

    host.init().await.expect("crabusb xhci init");
    let devices = host.probe_devices().await.expect("crabusb probe devices");
    crate::log!("crabusb: probe_devices count={}\n", devices.len());
    for dev in devices {
        match dev {
            crabusb::ProbedDevice::Device(info) => {
                let device = host.open_device(&info).await.expect("crabusb open device");
                crate::log!("Normal USB Device.");
            }

            crabusb::ProbedDevice::Hub(hub) => {
                crate::log!("Hub USB Device.");
                // inspect hub.as_device_info() 
            }
        }
    }
}

#[embassy_executor::task]
pub async fn usb_event_pump_task(handler: crabusb::EventHandler) {
    loop {
        match handler.handle_event() {
            crabusb::Event::Nothing => {
                embassy_time::Timer::after(embassy_time::Duration::from_millis(1)).await;
            }
            crabusb::Event::PortChange { port } => {
                crate::log!("crabusb: event port-change port={}\n", port);
            }
            crabusb::Event::TransferActivity { count } => {
                crate::log!("crabusb: event transfer-activity count={}\n", count);
            }
            crabusb::Event::Stopped => {
                crate::log!("crabusb: event pump stopped\n");
                return;
            }
        }
    }
}
