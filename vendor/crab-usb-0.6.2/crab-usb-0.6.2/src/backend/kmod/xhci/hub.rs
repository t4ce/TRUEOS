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

use crate::backend::kmod::hub::{HubInfo, HubOp, PortChangeInfo, PortState};

use super::reg::XhciRegisters;

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

    fn init(&mut self, info: HubInfo) -> BoxFuture<'_, Result<HubInfo, USBError>> {
        async {
            let mut info = info;
            info.speed = Speed::SuperSpeedPlus;
            debug!("Resetting all ports of xHCI Root Hub");

            for idx in 0..self.reg.port_register_set.len() {
                self.reg.port_register_set.update_volatile_at(idx, |reg| {
                    if !reg.portsc.port_power() {
                        trace!("Powering on port {}", idx + 1);
                        reg.portsc.set_port_power();
                    }
                });
            }

            for idx in 0..self.reg.port_register_set.len() {
                self.reg.port_register_set.update_volatile_at(idx, |reg| {
                    reg.portsc.set_0_port_enabled_disabled();
                    reg.portsc.set_port_reset();
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
        info!("crabusb/xhci/hub: changed_ports begin");
        self.handle_changed().await?;
        self.handle_uninit().await?;
        let out = self.handle_reseted().await?;
        info!("crabusb/xhci/hub: changed_ports end emitted={}", out.len());
        Ok(out)
    }

    async fn handle_changed(&mut self) -> Result<(), USBError> {
        let changed = self
            .ports()
            .iter()
            .filter(|port| port.changed.swap(false, Ordering::AcqRel))
            .map(|port| port.port_id)
            .collect::<Vec<_>>();

        for &id in &changed {
            let i = (id - 1) as usize;
            let port = self.reg.port_register_set.read_volatile_at(i).portsc;
            let connect_changed = port.connect_status_change();
            let enabled_changed = port.port_enabled_disabled_change();
            let warm_reset_changed = port.warm_port_reset_change();
            let over_current_changed = port.over_current_change();
            let reset_changed = port.port_reset_change();
            let link_changed = port.port_link_state_change();
            let config_error_changed = port.port_config_error_change();
            info!(
                "crabusb/xhci/hub: changed port={} connect={} enabled={} reset={} speed={} csc={} pedc={} wrc={} occ={} prc={} plc={} cec={} state={:?}",
                id,
                port.current_connect_status(),
                port.port_enabled_disabled(),
                port.port_reset(),
                port.port_speed(),
                connect_changed,
                enabled_changed,
                warm_reset_changed,
                over_current_changed,
                reset_changed,
                link_changed,
                config_error_changed,
                self.ports()[i].state,
            );

            self.reg.port_register_set.update_volatile_at(i, |reg| {
                if connect_changed {
                    reg.portsc.clear_connect_status_change();
                }
                if enabled_changed {
                    reg.portsc.clear_port_enabled_disabled_change();
                }
                if warm_reset_changed {
                    reg.portsc.clear_warm_port_reset_change();
                }
                if over_current_changed {
                    reg.portsc.clear_over_current_change();
                }
                if reset_changed {
                    reg.portsc.clear_port_reset_change();
                }
                if link_changed {
                    reg.portsc.clear_port_link_state_change();
                }
                if config_error_changed {
                    reg.portsc.clear_port_config_error_change();
                }
            });

            if port.port_reset() {
                self.ports_mut()[i].state = PortState::Uninit;
                continue;
            }

            if connect_changed && !port.current_connect_status() {
                info!("crabusb/xhci/hub: changed port={} disconnect observed", id);
                self.ports_mut()[i].state = PortState::Reseted;
                continue;
            }

            if connect_changed && port.current_connect_status() && !port.port_enabled_disabled() {
                info!(
                    "crabusb/xhci/hub: changed port={} issuing connect-time reset",
                    id
                );
                self.reg.port_register_set.update_volatile_at(i, |reg| {
                    reg.portsc.set_0_port_enabled_disabled();
                    reg.portsc.set_port_reset();
                });
                self.ports_mut()[i].state = PortState::Uninit;
                continue;
            }

            if reset_changed || (enabled_changed && port.port_enabled_disabled()) {
                info!(
                    "crabusb/xhci/hub: changed port={} reset/enable complete -> ready to probe",
                    id
                );
                self.ports_mut()[i].state = PortState::Reseted;
                continue;
            }

            info!(
                "crabusb/xhci/hub: changed port={} no actionable connect/reset transition",
                id
            );
        }

        Ok(())
    }

    async fn handle_uninit(&mut self) -> Result<(), USBError> {
        let uninited = self
            .ports()
            .iter()
            .filter(|port| matches!(port.state, PortState::Uninit))
            .map(|p| p.port_id)
            .collect::<Vec<_>>();

        for &id in &uninited {
            info!("crabusb/xhci/hub: uninit port={} waiting for reset", id);
            let i = (id - 1) as usize;

            let port = self.reg.port_register_set.read_volatile_at(i).portsc;

            if port.port_reset() {
                info!(
                    "crabusb/xhci/hub: uninit port={} still resetting connect={} enabled={} speed={}",
                    id,
                    port.current_connect_status(),
                    port.port_enabled_disabled(),
                    port.port_speed()
                );
                continue;
            }

            info!(
                "crabusb/xhci/hub: uninit port={} reset complete enable={} connect={} speed={}",
                id,
                port.port_enabled_disabled(),
                port.current_connect_status(),
                port.port_speed()
            );

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
            info!(
                "crabusb/xhci/hub: reseted port={} connect={} enabled={} speed={}",
                id,
                port_reg.portsc.current_connect_status(),
                port_reg.portsc.port_enabled_disabled(),
                port_reg.portsc.port_speed()
            );
            if !port_reg.portsc.current_connect_status() || !port_reg.portsc.port_enabled_disabled()
            {
                info!(
                    "crabusb/xhci/hub: reseted port={} skipped (not connect+enabled)",
                    id
                );
                continue;
            }
            let speed_raw = port_reg.portsc.port_speed();
            let speed = Speed::from_xhci_portsc(speed_raw);
            info!(
                "crabusb/xhci/hub: reseted port={} emitting change speed={:?}",
                id, speed
            );
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
