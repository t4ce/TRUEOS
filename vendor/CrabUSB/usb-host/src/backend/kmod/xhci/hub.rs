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

use crate::backend::kmod::{
    XhciRootHubInitPolicy,
    hub::{HubInfo, HubOp, PortChangeInfo, PortState},
};

use super::{reg::XhciRegisters, root_hub_profile::XhciRootHubPhysics};

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
    physics: XhciRootHubPhysics,
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
            match self.physics {
                XhciRootHubPhysics::BaremetalSelective => {
                    self.init_baremetal_selective_ports()?;
                }
                XhciRootHubPhysics::EmulatedFullReset => {
                    self.init_emulated_full_reset_ports()?;
                }
            }

            self.log_status_mut("root-hub-init-end");

            Ok(info)
        }
        .boxed()
    }

    fn slot_id(&self) -> u8 {
        0
    }

    fn request_port_reset<'a>(&'a mut self, port_id: u8) -> BoxFuture<'a, Result<(), USBError>> {
        async move {
            self.request_root_port_reset(port_id)?;
            Ok(())
        }
        .boxed()
    }
}

impl XhciRootHub {
    /// 创建新的 xHCI Root Hub
    pub fn new(
        reg: XhciRegisters,
        init_policy: XhciRootHubInitPolicy,
    ) -> Result<Self, USBError> {
        let port_num = reg.port_register_set.len();
        let ports = PortChangeWaker::new(port_num as _).ports.clone();

        Ok(Self {
            reg,
            ports,
            physics: XhciRootHubPhysics::from_init_policy(init_policy),
        })
    }

    pub fn waker(&self) -> PortChangeWaker {
        PortChangeWaker {
            ports: self.ports.clone(),
        }
    }

    fn root_port_ignored_by_physics(&self, port_id: u8) -> bool {
        self.physics.ignores_root_port(port_id)
    }

    fn request_root_port_reset(&mut self, port_id: u8) -> Result<(), USBError> {
        let Some(idx) = port_id.checked_sub(1).map(usize::from) else {
            return Err(USBError::Other(anyhow::anyhow!(
                "invalid root port reset request port=0"
            )));
        };
        if self.root_port_ignored_by_physics(port_id) {
            info!(
                "xhci: root port {} reset request ignored by baremetal selective physics",
                port_id
            );
            return Ok(());
        }
        if idx >= self.reg.port_register_set.len() {
            return Err(USBError::Other(anyhow::anyhow!(
                "root port reset request out of range port={} ports={}",
                port_id,
                self.reg.port_register_set.len()
            )));
        }

        self.log_port_status("root-hub-port-reset-request-begin", port_id);
        self.log_portsc("root-hub-port-reset-request-begin", port_id);
        self.fail_if_halted("root-hub-port-reset-request-begin")?;

        let before = self.reg.port_register_set.read_volatile_at(idx).portsc;
        if !before.port_power() {
            self.reg.port_register_set.update_volatile_at(idx, |reg| {
                reg.portsc.set_port_power();
            });
            self.log_port_status("root-hub-port-reset-request-after-power", port_id);
            self.log_portsc("root-hub-port-reset-request-after-power", port_id);
            self.fail_if_halted("root-hub-port-reset-request-after-power")?;
        } else {
            info!("xhci: root port {} power already set", port_id);
        }

        let before_ped = self.reg.port_register_set.read_volatile_at(idx).portsc;
        if before_ped.port_enabled_disabled() {
            self.reg.port_register_set.update_volatile_at(idx, |reg| {
                reg.portsc.set_0_port_enabled_disabled();
            });
            self.log_port_status("root-hub-port-reset-request-after-clear-ped", port_id);
            self.log_portsc("root-hub-port-reset-request-after-clear-ped", port_id);
            self.fail_if_halted("root-hub-port-reset-request-after-clear-ped")?;
        } else {
            info!("xhci: root port {} PED already clear; skipping PED write", port_id);
        }

        self.reg.port_register_set.update_volatile_at(idx, |reg| {
            reg.portsc.set_port_reset();
        });
        self.ports_mut()[idx].state = PortState::Uninit;
        self.log_port_status("root-hub-port-reset-request-after-set-reset", port_id);
        self.log_portsc("root-hub-port-reset-request-after-set-reset", port_id);
        self.fail_if_halted("root-hub-port-reset-request-after-set-reset")?;
        Ok(())
    }

    fn init_baremetal_selective_ports(&mut self) -> Result<(), USBError> {
        debug!("Observing selected xHCI Root Hub ports");
        self.log_status_mut("root-hub-init-begin");
        self.fail_if_halted("root-hub-init-begin")?;

        info!("xhci: root hub baremetal init observes ports=3,4 skip=11 reset_deferred=true");
        self.log_portsc("root-hub-selective-before", 3);
        self.log_portsc("root-hub-selective-before", 4);
        self.log_portsc("root-hub-selective-skip", 11);
        info!("xhci: root port 11 selective reset skipped reason=known-problematic-leds");

        Ok(())
    }

    fn init_emulated_full_reset_ports(&mut self) -> Result<(), USBError> {
        debug!("Resetting all ports of xHCI Root Hub");
        self.log_status_mut("root-hub-init-begin");
        self.fail_if_halted("root-hub-init-begin")?;

        info!("xhci: root hub qemu/full reset all ports");
        for idx in 0..self.reg.port_register_set.len() {
            self.reg.port_register_set.update_volatile_at(idx, |reg| {
                if !reg.portsc.port_power() {
                    trace!("Powering on port {}", idx + 1);
                    reg.portsc.set_port_power();
                }
            });
        }
        self.log_status_mut("root-hub-after-port-power");
        self.fail_if_halted("root-hub-after-port-power")?;

        for idx in 0..self.reg.port_register_set.len() {
            self.reg.port_register_set.update_volatile_at(idx, |reg| {
                reg.portsc.set_0_port_enabled_disabled();
                reg.portsc.set_port_reset();
            });
        }
        self.log_status_mut("root-hub-after-all-port-reset");
        self.fail_if_halted("root-hub-after-all-port-reset")?;
        Ok(())
    }

    async fn _changed_ports(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        self.log_status("root-hub-changed-ports-begin");
        self.fail_if_halted("root-hub-changed-ports-begin")?;
        self.handle_uninit().await?;
        self.log_status("root-hub-after-uninit");
        self.fail_if_halted("root-hub-after-uninit")?;
        let out = self.handle_reseted().await;
        self.log_status("root-hub-after-reseted");
        out
    }

    async fn handle_uninit(&mut self) -> Result<(), USBError> {
        let uninited = self
            .ports()
            .iter()
            .filter(|port| matches!(port.state, PortState::Uninit))
            .filter(|port| !self.root_port_ignored_by_physics(port.port_id))
            .map(|p| p.port_id)
            .collect::<Vec<_>>();

        for &id in &uninited {
            debug!("Waiting for port {id} reset ...");
            self.log_port_status("root-hub-before-port-reset-check", id);
            self.fail_if_halted("root-hub-before-port-reset-check")?;
            let i = (id - 1) as usize;

            let port = self.reg.port_register_set.read_volatile_at(i).portsc;

            if port.port_reset() {
                continue;
            }

            debug!(
                "Port {} reset complete, enable={}, connect={}",
                id,
                port.port_enabled_disabled(),
                port.current_connect_status()
            );
            self.log_port_status("root-hub-port-reset-complete", id);
            self.fail_if_halted("root-hub-port-reset-complete")?;

            self.ports_mut()[i].state = PortState::Reseted;
        }

        Ok(())
    }

    async fn handle_reseted(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        let reseted = self
            .ports()
            .iter()
            .filter(|port| matches!(port.state, PortState::Reseted))
            .filter(|port| !self.root_port_ignored_by_physics(port.port_id))
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
            debug!("Port {} device connected at speed {:?}", id, speed);
            debug!("Port {} : \r\n {:?}", id, port_reg.portsc);
            self.log_port_status("root-hub-before-probed-port", id);
            self.fail_if_halted("root-hub-before-probed-port")?;
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

    fn log_status(&self, stage: &'static str) {
        self.log_status_inner(stage, None);
    }

    fn log_status_mut(&mut self, stage: &'static str) {
        self.log_status_inner(stage, None);
    }

    fn log_port_status(&self, stage: &'static str, port_id: u8) {
        self.log_status_inner(stage, Some(port_id));
    }

    fn log_portsc(&self, stage: &'static str, port_id: u8) {
        let Some(idx) = port_id.checked_sub(1).map(usize::from) else {
            return;
        };
        if idx >= self.reg.port_register_set.len() {
            return;
        }
        let portsc = self.reg.port_register_set.read_volatile_at(idx).portsc;
        info!("xhci: portsc stage={} port={} {:?}", stage, port_id, portsc);
    }

    fn log_status_inner(&self, stage: &'static str, port_id: Option<u8>) {
        let usbsts = self.reg.operational.usbsts.read_volatile();
        let crcr = self.reg.operational.crcr.read_volatile();
        info!(
            "xhci: status stage={} port={} halted={} cnr={} eint={} hse={} hce={} sre={} crr={}",
            stage,
            port_id.unwrap_or(0),
            usbsts.hc_halted(),
            usbsts.controller_not_ready(),
            usbsts.event_interrupt(),
            usbsts.host_system_error(),
            usbsts.host_controller_error(),
            usbsts.save_restore_error(),
            crcr.command_ring_running()
        );
    }

    fn fail_if_halted(&self, stage: &'static str) -> Result<(), USBError> {
        let usbsts = self.reg.operational.usbsts.read_volatile();
        if usbsts.hc_halted() || usbsts.host_system_error() {
            return Err(USBError::Other(anyhow::anyhow!(
                "xHCI halted during root hub scan at {stage}: halted={} hse={} hce={}",
                usbsts.hc_halted(),
                usbsts.host_system_error(),
                usbsts.host_controller_error()
            )));
        }
        Ok(())
    }
}
