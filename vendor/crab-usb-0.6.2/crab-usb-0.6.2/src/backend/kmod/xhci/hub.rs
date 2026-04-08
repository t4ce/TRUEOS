//! xHCI Root Hub 实现
//!
//! 实现 xHCI 控制器的 Root Hub 功能，遵循 xHCI 规范第 4.19 章。

use alloc::{sync::Arc, vec::Vec};
use core::{
    cell::UnsafeCell,
    time::Duration,
    sync::atomic::{AtomicBool, Ordering},
};

use futures::{FutureExt, future::BoxFuture, task::AtomicWaker};
use usb_if::{err::USBError, host::hub::Speed};
use xhci::accessor::single;
use xhci::registers::{PortRegisterSet, operational::PortStatusAndControlRegister};

use crate::backend::kmod::hub::{HubInfo, HubOp, PortChangeInfo, PortState};
use crate::backend::kmod::osal::Kernel;

use super::reg::{MemMapper, XhciRegisters};

const HUB_DEBOUNCE_TIMEOUT_MS: u64 = 2000;
const HUB_DEBOUNCE_STEP_MS: u64 = 25;
const HUB_DEBOUNCE_STABLE_MS: u64 = 100;
const ROOT_PORT_BOOTSTRAP_SETTLE_MS: u64 = 150;
const ROOT_PORT_ENABLE_WAIT_MS: u64 = 1500;
const ROOT_PORT_ENABLE_CHECK_MS: u64 = 10;

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
    kernel: Kernel,

    ports: Arc<UnsafeCell<Vec<Port>>>,
}

unsafe impl Send for XhciRootHub {}

impl XhciRootHub {
    fn portsc_accessor(
        &mut self,
        port_id: u8,
    ) -> single::ReadWrite<PortStatusAndControlRegister, MemMapper> {
        let caplength = self.reg.capability.caplength.read_volatile().get();
        let base = self.reg.mmio_base + usize::from(caplength) + 0x400;
        let stride = core::mem::size_of::<PortRegisterSet>();
        let offset = (usize::from(port_id) - 1) * stride;
        unsafe { single::ReadWrite::new(base + offset, MemMapper) }
    }

    fn update_portsc<U>(&mut self, port_id: u8, f: U)
    where
        U: FnOnce(&mut PortStatusAndControlRegister),
    {
        let mut portsc = self.portsc_accessor(port_id);
        portsc.update_volatile(f);
    }

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
        let idx = (port_id.saturating_sub(1)) as usize;
        if let Some(port) = self.ports_mut().get_mut(idx) {
            port.state = PortState::Uninit;
            port.changed.store(true, Ordering::Release);
        }
    }

    fn init(&mut self, info: HubInfo) -> BoxFuture<'_, Result<HubInfo, USBError>> {
        async {
            let mut info = info;
            info.speed = Speed::SuperSpeedPlus;
            debug!("Resetting all ports of xHCI Root Hub");

            for idx in 0..self.reg.port_register_set.len() {
                let port_id = (idx + 1) as u8;
                self.update_portsc(port_id, |portsc| {
                    if !portsc.port_power() {
                        trace!("Powering on port {}", idx + 1);
                        portsc.set_port_power();
                    }
                });
            }

            self.kernel
                .delay(Duration::from_millis(ROOT_PORT_BOOTSTRAP_SETTLE_MS));

            for idx in 0..self.reg.port_register_set.len() {
                let port_id = (idx + 1) as u8;
                let status = self.reg.port_register_set.read_volatile_at(idx).portsc;
                if !status.current_connect_status() {
                    continue;
                }

                self.update_portsc(port_id, |portsc| {
                    portsc.set_0_port_enabled_disabled();
                    portsc.set_port_reset();
                });
            }

            for idx in 0..self.reg.port_register_set.len() {
                let port_id = (idx + 1) as u8;
                let status = self.reg.port_register_set.read_volatile_at(idx).portsc;
                if !status.current_connect_status() {
                    continue;
                }

                let mut elapsed_ms = 0;
                while elapsed_ms < ROOT_PORT_ENABLE_WAIT_MS {
                    let poll = self.reg.port_register_set.read_volatile_at(idx).portsc;
                    let reset_cleared = !poll.port_reset();
                    let enabled = poll.port_enabled_disabled();
                    if enabled || reset_cleared {
                        break;
                    }
                    self.kernel
                        .delay(Duration::from_millis(ROOT_PORT_ENABLE_CHECK_MS));
                    elapsed_ms += ROOT_PORT_ENABLE_CHECK_MS;
                }

                let final_port = self.reg.port_register_set.read_volatile_at(idx).portsc;
                info!(
                    "crabusb/xhci/hub: bootstrap port={} connect={} enabled={} reset={} speed={}",
                    port_id,
                    final_port.current_connect_status(),
                    final_port.port_enabled_disabled(),
                    final_port.port_reset(),
                    final_port.port_speed()
                );
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
    pub fn new(reg: XhciRegisters, kernel: Kernel) -> Result<Self, USBError> {
        let port_num = reg.port_register_set.len();
        let ports = PortChangeWaker::new(port_num as _).ports.clone();

        Ok(Self { reg, kernel, ports })
    }

    pub fn waker(&self) -> PortChangeWaker {
        PortChangeWaker {
            ports: self.ports.clone(),
        }
    }

    async fn _changed_ports(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        info!("crabusb/xhci/hub: changed_ports begin");
        self.handle_uninit().await?;
        let out = self.handle_reseted().await?;
        info!("crabusb/xhci/hub: changed_ports end emitted={}", out.len());
        Ok(out)
    }

    async fn debounce_port(&mut self, port_id: u8, must_be_connected: bool) -> Result<(), USBError> {
        let required_stable = (HUB_DEBOUNCE_STABLE_MS / HUB_DEBOUNCE_STEP_MS) as u8;
        let max_attempts = (HUB_DEBOUNCE_TIMEOUT_MS / HUB_DEBOUNCE_STEP_MS) as u8;
        let mut stable_count = 0u8;

        for _ in 0..max_attempts {
            self.kernel.delay(Duration::from_millis(HUB_DEBOUNCE_STEP_MS));
            let port = self.reg.port_register_set.read_volatile_at((port_id - 1) as usize).portsc;
            if port.current_connect_status() == must_be_connected {
                stable_count = stable_count.saturating_add(1);
                if stable_count >= required_stable {
                    return Ok(());
                }
            } else {
                stable_count = 0;
            }
        }

        Err(USBError::Timeout)
    }

    async fn reset_port(&mut self, port_id: u8, speed_raw: u8) -> Result<(), USBError> {
        let idx = (port_id - 1) as usize;
        self.update_portsc(port_id, |portsc| {
            portsc.set_0_port_enabled_disabled();
            portsc.set_port_reset();
        });

        let reset_delay_ms = if matches!(Speed::from_xhci_portsc(speed_raw), Speed::Low) {
            100
        } else {
            50
        };
        self.kernel.delay(Duration::from_millis(reset_delay_ms));

        for _ in 0..10 {
            let port = self.reg.port_register_set.read_volatile_at(idx).portsc;
            if port.port_reset_change() || !port.port_reset() {
                self.update_portsc(port_id, |portsc| {
                    if portsc.port_reset_change() {
                        portsc.clear_port_reset_change();
                    }
                });
                return Ok(());
            }
            self.kernel.delay(Duration::from_millis(10));
        }

        Err(USBError::Timeout)
    }

    async fn wait_for_port_enabled(&mut self, port_id: u8) -> Result<(), USBError> {
        let attempts = ROOT_PORT_ENABLE_WAIT_MS / ROOT_PORT_ENABLE_CHECK_MS;
        let idx = (port_id - 1) as usize;

        for _ in 0..attempts {
            let port = self.reg.port_register_set.read_volatile_at(idx).portsc;
            if port.current_connect_status() && port.port_enabled_disabled() {
                return Ok(());
            }
            self.kernel
                .delay(Duration::from_millis(ROOT_PORT_ENABLE_CHECK_MS));
        }

        Err(USBError::Timeout)
    }

    async fn settle_port_enabled(&mut self, port_id: u8, reason: &'static str) -> bool {
        match self.wait_for_port_enabled(port_id).await {
            Ok(()) => true,
            Err(USBError::Timeout) => {
                let idx = (port_id - 1) as usize;
                let port = self.reg.port_register_set.read_volatile_at(idx).portsc;
                info!(
                    "crabusb/xhci/hub: port={} {} still settling connect={} enabled={} reset={} speed={}",
                    port_id,
                    reason,
                    port.current_connect_status(),
                    port.port_enabled_disabled(),
                    port.port_reset(),
                    port.port_speed()
                );
                false
            }
            Err(err) => {
                info!(
                    "crabusb/xhci/hub: port={} {} failed while waiting for enable: {:?}",
                    port_id, reason, err
                );
                false
            }
        }
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

            self.update_portsc(id, |portsc| {
                if connect_changed {
                    portsc.clear_connect_status_change();
                }
                if enabled_changed {
                    portsc.clear_port_enabled_disabled_change();
                }
                if warm_reset_changed {
                    portsc.clear_warm_port_reset_change();
                }
                if over_current_changed {
                    portsc.clear_over_current_change();
                }
                if reset_changed {
                    portsc.clear_port_reset_change();
                }
                if link_changed {
                    portsc.clear_port_link_state_change();
                }
                if config_error_changed {
                    portsc.clear_port_config_error_change();
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
                self.debounce_port(id, true).await?;
                info!(
                    "crabusb/xhci/hub: changed port={} issuing connect-time reset",
                    id
                );
                self.reset_port(id, port.port_speed()).await?;
                if self
                    .settle_port_enabled(id, "connect-time reset")
                    .await
                {
                    self.ports_mut()[i].state = PortState::Reseted;
                } else {
                    self.ports_mut()[i].state = PortState::Uninit;
                }
                continue;
            }

            if reset_changed || (enabled_changed && port.port_enabled_disabled()) {
                info!(
                    "crabusb/xhci/hub: changed port={} reset/enable complete -> ready to probe",
                    id
                );
                if self
                    .settle_port_enabled(id, "post-reset change")
                    .await
                {
                    self.ports_mut()[i].state = PortState::Reseted;
                } else {
                    self.ports_mut()[i].state = PortState::Uninit;
                }
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

            if port.current_connect_status() || port.port_enabled_disabled() {
                info!(
                    "crabusb/xhci/hub: uninit port={} reset complete enable={} connect={} speed={}",
                    id,
                    port.port_enabled_disabled(),
                    port.current_connect_status(),
                    port.port_speed()
                );
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
            if port_reg.portsc.current_connect_status() || port_reg.portsc.port_enabled_disabled() {
                info!(
                    "crabusb/xhci/hub: reseted port={} connect={} enabled={} speed={}",
                    id,
                    port_reg.portsc.current_connect_status(),
                    port_reg.portsc.port_enabled_disabled(),
                    port_reg.portsc.port_speed()
                );
            }
            if !port_reg.portsc.current_connect_status() || !port_reg.portsc.port_enabled_disabled()
            {
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
