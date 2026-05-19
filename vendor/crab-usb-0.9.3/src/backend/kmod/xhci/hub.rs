//! xHCI Root Hub 实现
//!
//! 实现 xHCI 控制器的 Root Hub 功能，遵循 xHCI 规范第 4.19 章。

use alloc::{sync::Arc, vec::Vec};
use core::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use futures::{FutureExt, future::BoxFuture, task::AtomicWaker};
use usb_if::{err::USBError, host::hub::Speed};
use xhci::accessor::single;
use xhci::registers::{PortRegisterSet, operational::PortStatusAndControlRegister};

use super::reg::{MemMapper, XhciRegisters};
use crate::backend::kmod::hub::{HubInfo, HubOp, PortChangeInfo, PortState};
use crate::osal::Kernel;

const ROOT_PORT_BOOTSTRAP_SETTLE_MS: u64 = 150;
const ROOT_PORT_ENABLE_WAIT_MS: u64 = 1500;
const ROOT_PORT_ENABLE_CHECK_MS: u64 = 10;
const WARM_RESET_SETTLE_MS: u64 = 100;
const WARM_RESET_WAIT_MS: u64 = 1500;

fn speed_name(spd: u8) -> &'static str {
    match spd {
        0 => "-",
        1 => "FS",
        2 => "LS",
        3 => "HS",
        4 => "SS",
        5 => "SS+",
        _ => "?",
    }
}

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
    kernel: Kernel,
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

    async fn warm_reset_port(&mut self, port_id: u8) -> Result<bool, USBError> {
        let idx = (port_id - 1) as usize;

        self.update_portsc(port_id, |portsc| {
            portsc.set_warm_port_reset();
        });

        self.kernel.delay(Duration::from_millis(WARM_RESET_SETTLE_MS));

        let attempts = WARM_RESET_WAIT_MS / ROOT_PORT_ENABLE_CHECK_MS;
        for _ in 0..attempts {
            let port = self.reg.port_register_set.read_volatile_at(idx).portsc;
            if !port.warm_port_reset() || port.warm_port_reset_change() {
                self.update_portsc(port_id, |portsc| {
                    if portsc.warm_port_reset_change() {
                        portsc.clear_warm_port_reset_change();
                    }
                    if portsc.port_reset_change() {
                        portsc.clear_port_reset_change();
                    }
                });
                let final_port = self.reg.port_register_set.read_volatile_at(idx).portsc;
                return Ok(final_port.current_connect_status()
                    && final_port.port_enabled_disabled());
            }
            self.kernel
                .delay(Duration::from_millis(ROOT_PORT_ENABLE_CHECK_MS));
        }

        Ok(false)
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
            let total_ports = self.reg.port_register_set.len();

            for idx in 0..total_ports {
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

            for idx in 0..total_ports {
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

            for idx in 0..total_ports {
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
            }

            let mut kicked_ss = false;
            for idx in 0..total_ports {
                let port_id = (idx + 1) as u8;
                let status = self.reg.port_register_set.read_volatile_at(idx).portsc;
                if port_id >= 17
                    && status.port_power()
                    && !status.current_connect_status()
                    && status.port_link_state() == 4
                {
                    self.update_portsc(port_id, |portsc| {
                        portsc.set_port_link_state(5);
                        portsc.set_port_link_state_write_strobe();
                    });
                    kicked_ss = true;
                }
            }
            if kicked_ss {
                self.kernel.delay(Duration::from_millis(500));
                for idx in 0..total_ports {
                    let port_id = (idx + 1) as u8;
                    if port_id < 17 {
                        continue;
                    }
                    let p = self.reg.port_register_set.read_volatile_at(idx).portsc;
                    if p.current_connect_status() && !p.port_enabled_disabled() {
                        let _ = self.warm_reset_port(port_id).await;
                    }
                }
            }

            for idx in 0..total_ports {
                let port_id = (idx + 1) as u8;
                let status = self.reg.port_register_set.read_volatile_at(idx).portsc;
                if status.current_connect_status()
                    && !status.port_enabled_disabled()
                    && !status.port_reset()
                {
                    let _ = self.reset_port(port_id, status.port_speed()).await;
                    let after_hot = self.reg.port_register_set.read_volatile_at(idx).portsc;
                    if after_hot.current_connect_status()
                        && !after_hot.port_enabled_disabled()
                        && !after_hot.port_reset()
                    {
                        let _ = self.warm_reset_port(port_id).await;
                    }
                }
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

        Ok(Self { reg, ports, kernel })
    }

    pub fn waker(&self) -> PortChangeWaker {
        PortChangeWaker {
            ports: self.ports.clone(),
        }
    }

    async fn _changed_ports(&mut self) -> Result<Vec<PortChangeInfo>, USBError> {
        debug!("xhci/root-hub: changed_ports begin");
        self.handle_uninit().await?;
        let ports = self.handle_reseted().await?;
        if ports.is_empty() {
            debug!("xhci/root-hub: changed_ports done count=0");
        } else {
            info!("xhci/root-hub: changed_ports done count={}", ports.len());
        }
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

            if port.port_reset() {
                continue;
            }

            if port.current_connect_status() && !port.port_enabled_disabled() {
                let _ = self.reset_port(id, port.port_speed()).await;
                let after_hot = self.reg.port_register_set.read_volatile_at(i).portsc;
                if after_hot.current_connect_status()
                    && !after_hot.port_enabled_disabled()
                    && !after_hot.port_reset()
                {
                    let _ = self.warm_reset_port(id).await;
                }
            }

            let final_port = self.reg.port_register_set.read_volatile_at(i).portsc;
            if final_port.current_connect_status() || final_port.port_enabled_disabled() {
                info!(
                    "xhci/root-hub: port {} reseted ccs={} ped={} speed={}",
                    id,
                    final_port.current_connect_status() as u8,
                    final_port.port_enabled_disabled() as u8,
                    speed_name(final_port.port_speed())
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
