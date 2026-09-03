use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::backend::BackendOp;
use crate::backend::ty::*;
#[cfg(kmod)]
use crate::diag::{XhciDirectRequest, XhciDirectResponse};
use crate::err::Result;
#[cfg(kmod)]
use crate::recover::{XhciRecoveryRequest, XhciRecoveryResponse};

#[cfg(kmod)]
pub use super::backend::kmod::*;

#[cfg(umod)]
pub use super::backend::umod::*;

pub use crate::device::{Device, DeviceInfo, HubDeviceInfo, ProbedDevice};

/// USB 主机控制器
pub struct USBHost {
    pub(crate) backend: Box<dyn BackendOp>,
}

impl USBHost {
    /// 初始化主机控制器
    pub async fn init(&mut self) -> Result<()> {
        self.backend.init().await?;
        Ok(())
    }

    pub async fn probe_devices(&mut self) -> Result<Vec<ProbedDevice>> {
        let device_infos = self.backend.device_list().await?;
        let mut devices = Vec::new();
        for dev in device_infos {
            let dev_info = match dev {
                ProbedDeviceInfoOp::Device(inner) => ProbedDevice::Device(DeviceInfo { inner }),
                ProbedDeviceInfoOp::Hub(inner) => ProbedDevice::Hub(HubDeviceInfo { inner }),
            };
            devices.push(dev_info);
        }
        Ok(devices)
    }

    #[cfg(kmod)]
    pub fn create_event_handler(&mut self) -> EventHandler {
        let handler = self.backend.create_event_handler();
        EventHandler { handler }
    }

    #[cfg(kmod)]
    pub async fn request_root_port_reset(&mut self, port_id: u8) -> Result<()> {
        self.backend.request_root_port_reset(port_id).await
    }

    /// Execute one register-level xHCI diagnostic operation through the live
    /// backend owner.  Callers are responsible for controller quiescence and
    /// for applying any policy around mutating requests.
    #[cfg(kmod)]
    pub async fn xhci_direct(&mut self, request: XhciDirectRequest) -> Result<XhciDirectResponse> {
        self.backend.xhci_direct(request).await
    }

    /// Execute one bounded semantic recovery operation through the exclusive
    /// xHCI backend owner.
    #[cfg(kmod)]
    pub async fn xhci_recover(
        &mut self,
        request: XhciRecoveryRequest,
    ) -> Result<XhciRecoveryResponse> {
        self.backend.xhci_recover(request).await
    }

    pub async fn open_device(&mut self, dev: &DeviceInfo) -> Result<Device> {
        let device = self.backend.open_device(dev.inner.as_ref()).await?;
        let mut device: Device = device.into();
        device.init().await?;
        Ok(device)
    }
}

pub struct EventHandler {
    handler: Box<dyn EventHandlerOp>,
}

impl EventHandler {
    /// 处理事件
    pub fn handle_event(&self) -> Event {
        self.handler.handle_event()
    }
}
