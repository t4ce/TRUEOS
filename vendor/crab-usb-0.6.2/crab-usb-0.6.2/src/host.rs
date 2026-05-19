use alloc::boxed::Box;
use alloc::vec::Vec;

use crate::backend::BackendOp;
use crate::backend::ty::*;
use crate::err::Result;
use crate::topology::{DeviceHandle, DeviceTree};
use crate::DeviceId;

#[cfg(kmod)]
pub use super::backend::kmod::*;

#[cfg(all(umod, not(any(target_os = "trueos", target_os = "zkvm"))))]
pub use super::backend::umod::*;

pub use crate::device::{Device, DeviceInfo};

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

    pub async fn probe_devices(&mut self) -> Result<Vec<DeviceInfo>> {
        let device_infos = self.backend.device_list().await?;
        let mut devices = Vec::new();
        for dev in device_infos {
            let dev_info = DeviceInfo { inner: dev };
            devices.push(dev_info);
        }
        Ok(devices)
    }

    pub async fn topology(&mut self) -> Result<DeviceTree> {
        self.backend.topology().await
    }

    pub async fn device(&mut self, id: DeviceId) -> Result<Option<DeviceHandle>> {
        Ok(self.topology().await?.device(id))
    }

    #[cfg(kmod)]
    pub fn create_event_handler(&mut self) -> EventHandler {
        let handler = self.backend.create_event_handler();
        EventHandler { handler }
    }

    pub async fn open_device(&mut self, dev: &DeviceInfo) -> Result<Device> {
        let device = self.backend.open_device(dev.inner.as_ref()).await?;
        let mut device: Device = device.into();
        device.init().await?;
        Ok(device)
    }

    pub async fn open_device_id(&mut self, id: DeviceId) -> Result<Device> {
        let device = self.backend.open_device_by_id(id).await?;
        let mut device: Device = device.into();
        device.init().await?;
        Ok(device)
    }

    pub async fn open_handle(&mut self, handle: &DeviceHandle) -> Result<Device> {
        self.open_device_id(handle.id()).await
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
