//! xHCI Root Hub 实现
//!
//! 实现 xHCI 控制器的 Root Hub 功能，遵循 xHCI 规范第 4.19 章。

use alloc::{sync::Arc, vec::Vec};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};

use futures::{FutureExt, future::BoxFuture, task::AtomicWaker};
use usb_if::{err::USBError, host::hub::Speed};

use super::reg::XhciRegisters;
use crate::backend::kmod::hub::{HubInfo, HubOp, PortChangeInfo, PortState};

pub struct PortChangeWaker {
    ports: Arc<UnsafeCell<Vec<Port>>>,
}

unsafe impl Send for PortChangeWaker {}
unsafe impl Sync for PortChangeWaker {}

impl PortChangeWaker {
    #[allow(clippy::arc_with_non_send_sync)]
    pub fn new(port_num: u8) -> Self {
        let mut ports = Vec::with_capacity(port_num as usize);
        for i in 0..port_num {
            ports.push(Port {
                port_id: i + 1,
                change_waker: AtomicWaker::new(),
                changed: AtomicBool::new(false),
                state: PortState::Uninit,
            });
        }
        Self {
            ports: Arc::new(UnsafeCell::new(ports)),
        }
    }

    pub fn set_port_changed(&self, port_id: u8) {
        let ports = unsafe { &*self.ports.get() };
        let idx = (port_id - 1) as usize;
        debug!("Setting port {} changed", port_id);
        ports[idx].changed.store(true, Ordering::Release);
        ports[idx].change_waker.wake();
    }
}

pub struct Port {
    port_id: u8,
    change_waker: AtomicWaker,
    changed: AtomicBool,
    state: PortState,
}

/// xHCI Root Hub
///
/// Root Hub 是集成在 xHCI 控制器中的虚拟 Hub。
pub struct XhciRootHub {
    /// 寄存器访问
    reg: XhciRegisters,

    ports: Arc<UnsafeCell<Vec<Port>>>,
}

unsafe impl Send for XhciRootHub {}

impl XhciRootHub {
    fn ports(&self) -> &[Port] {
        unsafe { &*self.ports.get() }
    }

    fn ports_mut(&mut self) -> &mut [Port] {
        unsafe { &mut *self.ports.get() }
    }
}

impl HubOp for XhciRootHub {
    fn changed_ports(&mut self) -> BoxFuture<'_, Result<Vec<PortChangeInfo>, USBError>> {
        self._changed_ports().boxed()
    }

    fn rearm_port(&mut self, port_id: u8) {
        let idx = port_id.saturating_sub(1) as usize;
        if let Some(port) = self.ports_mut().get_mut(idx) {
            info!("xhci/root-hub: rearm port {}", port_id);
            port.state = PortState::Uninit;
            port.changed.store(true, Ordering::Release);
        }
    }

    fn init(&mut self, info: HubInfo) -> BoxFuture<'_, Result<HubInfo, USBError>> {
        async {
            let mut info = info;
            info.speed = Speed::SuperSpeedPlus;
            info!("xhci/root-hub: power ports without blanket reset");

            for idx in 0..self.reg.port_register_set.len() {
                self.reg.port_register_set.update_volatile_at(idx, |reg| {
                    if !reg.portsc.port_power() {
                        trace!("Powering on port {}", idx + 1);
                        reg.portsc.set_port_power();
                    }
                });
            }

            Ok(info)
        }
        .boxed()
    }

    fn slot_id(&self) -> u8 {
        0
    }
}

impl XhciRootHub {
    /// 创建新的 xHCI Root Hub
    pub fn new(reg: XhciRegisters) -> Result<Self, USBError> {
        let port_num = reg.port_register_set.len();
        let ports = PortChangeWaker::new(port_num as _).ports.clone();

        Ok(Self { reg, ports })
    }

    pub fn waker(&self) -> PortChangeWaker {
        PortChangeWaker {
            ports: self.ports.clone(),
        }
    }

    async fn _changed_ports(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        info!("xhci/root-hub: changed_ports begin");
        self.handle_uninit().await?;
        let ports = self.handle_reseted().await?;
        info!("xhci/root-hub: changed_ports done count={}", ports.len());
        Ok(ports)
    }

    async fn handle_uninit(&mut self) -> Result<(), USBError> {
        let uninited = self
            .ports()
            .iter()
            .filter(|port| matches!(port.state, PortState::Uninit))
            .map(|p| p.port_id)
            .collect::<Vec<_>>();

        for &id in &uninited {
            let i = (id - 1) as usize;

            let port = self.reg.port_register_set.read_volatile_at(i).portsc;

            let connect = port.current_connect_status();
            let enable = port.port_enabled_disabled();
            let speed_raw = port.port_speed();

            info!(
                "xhci/root-hub: port {} initial state enable={} connect={} reset={} speed_raw={}",
                id,
                enable,
                connect,
                port.port_reset(),
                speed_raw
            );

            if connect && !enable {
                info!("xhci/root-hub: port {} connected but not enabled; leaving untouched", id);
                self.ports_mut()[i].state = PortState::Probed;
                continue;
            }

            self.ports_mut()[i].state = PortState::Reseted;
        }

        Ok(())
    }

    async fn handle_reseted(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        let reseted = self
            .ports()
            .iter()
            .filter(|port| matches!(port.state, PortState::Reseted))
            .map(|p| p.port_id)
            .collect::<Vec<_>>();

        let mut out = Vec::new();

        for &id in &reseted {
            let i = (id - 1) as usize;
            let port_reg = self.reg.port_register_set.read_volatile_at(i);
            if !port_reg.portsc.current_connect_status() || !port_reg.portsc.port_enabled_disabled()
            {
                continue;
            }
            let speed_raw = port_reg.portsc.port_speed();
            let speed = Speed::from_xhci_portsc(speed_raw);
            info!(
                "xhci/root-hub: port {} device connected speed={:?} raw={}",
                id,
                speed,
                speed_raw
            );
            debug!("Port {} : \r\n {:?}", id, port_reg.portsc);
            self.ports_mut()[i].state = PortState::Probed;

            out.push(PortChangeInfo {
                root_port_id: id,
                port_id: id,
                port_speed: speed,
                // Root Hub 不需要 TT
                tt_port_on_hub: None,
            });
        }

        Ok(out)
    }
}
