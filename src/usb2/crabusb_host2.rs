use core::time::Duration;

use crab_usb_host2::{KernelOp, USBHost};
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::crabusb_service::CRABUSB_KERNEL;
use super::xhci::MAX_XHCI_CONTROLLERS;

impl KernelOp for super::crabusb_service::TrueosCrabUsbKernel {
    fn delay(&self, duration: Duration) {
        let millis = duration.as_millis();
        if millis == 0 {
            return;
        }
        let timeout_ms = millis.min(u128::from(u64::MAX)) as u64;
        let _ = crate::wait::spin_until_timeout(timeout_ms, || false);
    }
}

#[embassy_executor::task(pool_size = MAX_XHCI_CONTROLLERS)]
pub async fn bsp_service2(controller_index: usize) {
    const RETRY_MS: u64 = 1000;
    const HOLD_MS: u64 = 60_000;

    loop {
        let Some(info) = super::controller_by_index(controller_index) else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        crate::pci::enable_mem_and_bus_master(info.bus, info.slot, info.function);

        let mut host = match USBHost::new_xhci(info.mmio_base, &CRABUSB_KERNEL) {
            Ok(host) => host,
            Err(err) => {
                crate::log_info!(
                    target: "usb";
                    "crabusb2: new_xhci failed ctrl={} bdf={:02X}:{:02X}.{} vid={:04X} pid={:04X} err={:?}\n",
                    info.index,
                    info.bus,
                    info.slot,
                    info.function,
                    info.vendor_id,
                    info.device_id,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
                continue;
            }
        };

        match host.init().await {
            Ok(()) => {
                crate::log_info!(
                    target: "usb";
                    "crabusb2: init successful ctrl={} bdf={:02X}:{:02X}.{} vid={:04X} pid={:04X} mmio={:p} probe=disabled\n",
                    info.index,
                    info.bus,
                    info.slot,
                    info.function,
                    info.vendor_id,
                    info.device_id,
                    info.mmio_base
                );
                loop {
                    Timer::after(EmbassyDuration::from_millis(HOLD_MS)).await;
                }
            }
            Err(err) => {
                crate::log_info!(
                    target: "usb";
                    "crabusb2: init failed ctrl={} bdf={:02X}:{:02X}.{} vid={:04X} pid={:04X} err={:?}\n",
                    info.index,
                    info.bus,
                    info.slot,
                    info.function,
                    info.vendor_id,
                    info.device_id,
                    err
                );
                Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            }
        }
    }
}
