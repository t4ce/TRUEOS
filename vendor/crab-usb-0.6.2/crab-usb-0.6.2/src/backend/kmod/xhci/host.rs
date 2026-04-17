use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::{cell::UnsafeCell, time::Duration};

use ::xhci::{
    ExtendedCapability,
    extended_capabilities::{List, usb_legacy_support_capability::UsbLegacySupport},
    ring::trb::{command, event::CommandCompletion},
};
use dma_api::DmaDirection;
use futures::{FutureExt, future::BoxFuture};
use mbarrier::mb;
use spin::RwLock;
use usb_if::err::{TransferError, USBError};

use super::{
    Device, SlotId,
    cmd::CommandRing,
    context::{DeviceContextList, ScratchpadBufferArray},
    event::{EventRing, EventRingInfo},
    hub::{PortChangeWaker, XhciRootHub},
    reg::{MemMapper, XhciRegisters},
    transfer::TransferResultHandler,
};
use crate::{
    DeviceAddressInfo, KernelOp, Mmio,
    backend::{
        kmod::{hub::HubOp, kcore::CoreOp, xhci::reg::SlotBell},
        ty::{DeviceOp, Event, EventHandlerOp},
    },
    debug_record_event,
    err::{ConvertXhciError, Result},
    osal::Kernel,
    queue::Finished,
};

pub struct Xhci {
    pub(crate) reg: Arc<RwLock<XhciRegisters>>,
    pub(crate) kernel: Kernel,
    pub(crate) cmd: CommandRing,
    dev_ctx: Option<DeviceContextList>,
    event_handler: Option<EventHandler>,
    irq_ready: bool,
    event_ring_info: EventRingInfo,
    scratchpad_buf_arr: Option<ScratchpadBufferArray>,
    pci_vendor_id: Option<u16>,
    pci_device_id: Option<u16>,
    disable_staged_run_experiments: bool,
    log_first_command_diagnostics: bool,
    first_command_logged: bool,
    pub(crate) transfer_result_handler: TransferResultHandler,
    root_hub: Option<XhciRootHub>,
}

unsafe impl Send for Xhci {}
unsafe impl Sync for Xhci {}

impl CoreOp for Xhci {
    fn root_hub(&mut self) -> Box<dyn HubOp> {
        Box::new(
            self.root_hub
                .take()
                .expect("Root hub can only be taken once"),
        )
    }

    fn init<'a>(&'a mut self) -> BoxFuture<'a, core::result::Result<(), USBError>> {
        self._init().boxed()
    }

    fn new_addressed_device<'a>(
        &'a mut self,
        addr: DeviceAddressInfo,
    ) -> BoxFuture<'a, Result<Box<dyn DeviceOp>>> {
        self.new_device(addr).boxed()
    }

    fn create_event_handler(&mut self) -> Box<dyn EventHandlerOp> {
        if !self.irq_ready {
            if Self::POLL_ONLY_EVENT_HANDLER_SMOKE_VALID {
                info!("crabusb/xhci: polling-only event handler mode (interrupt signaling disabled)");
            } else {
                self.arm_irq();
                self.enable_irq();
            }
            self.irq_ready = true;
        }
        Box::new(
            self.event_handler
                .take()
                .expect("Event handler can only be created once"),
        )
    }

    fn kernel(&self) -> &Kernel {
        &self.kernel
    }
}

impl Xhci {
    const STATUS_POLL_DELAY_MS: u64 = 1;
    const STATUS_POLL_LIMIT: usize = 2_000;
    const SKIP_SCRATCHPADS_EXPERIMENT: bool = false;
    const PROGRAM_DCBAAP_BEFORE_RUN_EXPERIMENT: bool = false;
    const PROGRAM_CRCR_BEFORE_RUN_EXPERIMENT: bool = true;
    const PROGRAM_RUNTIME_RING_BEFORE_RUN_EXPERIMENT: bool = true;
    const ARM_WRAP_EVENT_EXPERIMENT: bool = false;
    const POLL_ONLY_EVENT_HANDLER_SMOKE_VALID: bool = true;
    const INTEL_VENDOR_ID: u16 = 0x8086;
    const INTEL_RPL_PCH_XHCI_DEVICE_ID: u16 = 0x7A60;

    fn flush_controller_write(&self) {
        let _ = self.reg.read().operational.usbsts.read_volatile();
        mb();
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
        #[cfg(target_arch = "x86")]
        unsafe {
            core::arch::x86::_mm_mfence();
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::x86_64::_mm_mfence();
        }
    }

    fn status_snapshot(&self) -> XhciStatusBits {
        let sts = self.reg.read().operational.usbsts.read_volatile();
        let cmd = self.reg.read().operational.usbcmd.read_volatile();

        XhciStatusBits {
            halted: sts.hc_halted(),
            cnr: sts.controller_not_ready(),
            reset: cmd.host_controller_reset(),
            hse: sts.host_system_error(),
            hce: sts.host_controller_error(),
        }
    }

    fn ensure_controller_state(
        &mut self,
        stage: &'static str,
        expected_halted: bool,
        expected_cnr: bool,
        expected_reset: bool,
    ) -> Result {
        let status = self.status_snapshot();
        if status.hse
            || status.hce
            || status.halted != expected_halted
            || status.cnr != expected_cnr
            || status.reset != expected_reset
        {
            return Err(USBError::Other(anyhow!(
                "xHCI controller in unexpected state after {stage}: halted={} cnr={} reset={} hse={} hce={} expected_halted={expected_halted} expected_cnr={expected_cnr} expected_reset={expected_reset}",
                status.halted,
                status.cnr,
                status.reset,
                status.hse,
                status.hce,
            )));
        }
        Ok(())
    }

    async fn wait_for_status<F>(&self, stage: &'static str, mut ready: F) -> Result
    where
        F: FnMut(&XhciStatusBits) -> bool,
    {
        for _ in 0..Self::STATUS_POLL_LIMIT {
            let status = self.status_snapshot();
            if status.hse || status.hce {
                return Err(USBError::Other(anyhow!(
                    "xHCI controller became unhealthy while waiting for {stage}: halted={} cnr={} reset={} hse={} hce={}",
                    status.halted,
                    status.cnr,
                    status.reset,
                    status.hse,
                    status.hce,
                )));
            }
            if ready(&status) {
                return Ok(());
            }
            self.kernel
                .delay(Duration::from_millis(Self::STATUS_POLL_DELAY_MS));
        }

        let status = self.status_snapshot();
        Err(USBError::Other(anyhow!(
            "xHCI controller timed out while waiting for {stage}: halted={} cnr={} reset={} hse={} hce={}",
            status.halted,
            status.cnr,
            status.reset,
            status.hse,
            status.hce,
        )))
    }

    fn poll_command_completion(
        &mut self,
        stage: &'static str,
        addr: crate::BusAddr,
    ) -> Result<CommandCompletion> {
        for _ in 0..Self::STATUS_POLL_LIMIT {
            let status = self.status_snapshot();
            if status.hse || status.hce || status.halted || status.cnr || status.reset {
                return Err(USBError::Other(anyhow!(
                    "xHCI controller became unhealthy during {stage}: halted={} cnr={} reset={} hse={} hce={}",
                    status.halted,
                    status.cnr,
                    status.reset,
                    status.hse,
                    status.hce,
                )));
            }

            if let Some(handler) = self.event_handler.as_ref() {
                let _ = handler.handle_event();
            }

            if let Some(cpl) = self.cmd.poll_finished(addr) {
                return Ok(cpl);
            }

            self.kernel
                .delay(Duration::from_millis(Self::STATUS_POLL_DELAY_MS));
        }

        let status = self.status_snapshot();
        Err(USBError::Other(anyhow!(
            "xHCI command timed out during {stage}: halted={} cnr={} reset={} hse={} hce={}",
            status.halted,
            status.cnr,
            status.reset,
            status.hse,
            status.hce,
        )))
    }

    fn command_ring_self_test(&mut self) -> Result {
        let noop = command::Allowed::Noop(command::Noop::default());
        let addr = self.cmd.submit_for_poll(noop);
        let completion = self.poll_command_completion("post-run noop self-test", addr)?;
        match completion.completion_code() {
            Ok(code) => code.to_result()?,
            Err(err) => Err(USBError::Other(anyhow!(
                "xHCI post-run noop self-test completion decode failed: {err:?}"
            )))?,
        }
        self.ensure_controller_state("post-run noop self-test", false, false, false)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct XhciStatusBits {
    halted: bool,
    cnr: bool,
    reset: bool,
    hse: bool,
    hce: bool,
}

impl Xhci {
    pub fn new(mmio: Mmio, kernel: &'static dyn KernelOp) -> Result<Self> {
        Self::new_with_pci_ids(mmio, kernel, 0, 0)
    }

    pub fn new_with_pci_ids(
        mmio: Mmio,
        kernel: &'static dyn KernelOp,
        vendor_id: u16,
        device_id: u16,
    ) -> Result<Self> {
        let reg = XhciRegisters::new(mmio);

        // 检查 xHCI 控制器的寻址能力（HCCPARAMS1 寄存器）
        let hccparams1 = reg.capability.hccparams1.read_volatile();
        let ac64 = hccparams1.addressing_capability(); // Bit[0]: 64-bit Addressing Capability

        // Keep xHCI infrastructure DMA below 4 GiB on bare metal.
        // Some controllers (e.g. Intel RPL-PCH 7A60) advertise AC64 but silently
        // fail to fetch scratchpad buffers or command/event rings from addresses
        // above 4 GiB, causing the first command to never complete.
        let force_32bit_dma =
            vendor_id == Self::INTEL_VENDOR_ID && device_id == Self::INTEL_RPL_PCH_XHCI_DEVICE_ID;
        let dma_mask = if force_32bit_dma || !ac64 {
            u32::MAX as usize
        } else {
            u64::MAX as usize
        };

        let kernel = Kernel::new(dma_mask as _, kernel);

        let reg_shared = Arc::new(RwLock::new(reg.clone()));

        let cmd = CommandRing::new(DmaDirection::Bidirectional, &kernel, reg_shared.clone())?;
        let cmd_finished = cmd.finished_handle();
        let event_ring = EventRing::new(&kernel)?;
        let event_ring_info = event_ring.info();

        let root_hub = XhciRootHub::new(reg.clone(), kernel.clone())?;
        let disable_staged_run_experiments =
            vendor_id == Self::INTEL_VENDOR_ID && device_id == Self::INTEL_RPL_PCH_XHCI_DEVICE_ID;
        let log_first_command_diagnostics = disable_staged_run_experiments;

        if disable_staged_run_experiments {
            info!(
                "xHCI: conservative pre-RUN quirk enabled for pci {:04X}:{:04X} dma_mask={}bit",
                vendor_id,
                device_id,
                if force_32bit_dma { 32 } else { 64 }
            );
        }

        let transfer_result_handler = TransferResultHandler::new(reg_shared.clone());
        let ports = root_hub.waker();

        Ok(Xhci {
            reg: reg_shared,
            kernel,
            cmd,
            dev_ctx: None,
            transfer_result_handler: transfer_result_handler.clone(),
            event_handler: Some(EventHandler::new(
                reg,
                cmd_finished,
                event_ring,
                transfer_result_handler,
                ports,
            )),
            irq_ready: false,
            root_hub: Some(root_hub),
            event_ring_info,
            scratchpad_buf_arr: None,
            pci_vendor_id: (vendor_id != 0).then_some(vendor_id),
            pci_device_id: (device_id != 0).then_some(device_id),
            disable_staged_run_experiments,
            log_first_command_diagnostics,
            first_command_logged: false,
        })
    }

    async fn _init(&mut self) -> Result {
        self.disable_irq();
        // 4.2 Host Controller Initialization
        self.init_ext_caps().await?;
        // After Chip Hardware Reset6 wait until the Controller Not Ready (CNR) flag
        // in the USBSTS is ‘0’ before writing any xHC Operational or Runtime
        // registers.
        self.chip_hardware_reset().await?;
        self.ensure_controller_state("reset", true, false, false)?;
        self.clear_status_bits();

        self.disable_irq();

        // Program the Max Device Slots Enabled (MaxSlotsEn) field in the CONFIG
        // register (5.4.7) to enable the device slots that system software is going to
        // use.
        let max_slots = self.setup_max_device_slots();
        self.dev_ctx = Some(DeviceContextList::new(max_slots as _, self.kernel())?);

        let program_dcbaap =
            self.disable_staged_run_experiments || Self::PROGRAM_DCBAAP_BEFORE_RUN_EXPERIMENT;
        if program_dcbaap {
            self.setup_dcbaap()?;
        }

        let program_crcr =
            self.disable_staged_run_experiments || Self::PROGRAM_CRCR_BEFORE_RUN_EXPERIMENT;
        if program_crcr {
            self.set_cmd_ring()?;
        }

        let program_runtime_ring =
            self.disable_staged_run_experiments || Self::PROGRAM_RUNTIME_RING_BEFORE_RUN_EXPERIMENT;
        if program_runtime_ring {
            self.setup_runtime_ring();
        }

        let scratch_count = self.setup_scratchpads()?;
        self.log_init_setup_summary(
            max_slots,
            program_dcbaap,
            program_crcr,
            program_runtime_ring,
            scratch_count,
        );
        // At this point, the host controller is up and running and the Root Hub ports
        // (5.4.8) will begin reporting device connects, etc., and system software may begin
        // enumerating devices. System software may follow the procedures described in
        // section 4.3, to enumerate attached devices.
        self.start();

        self.wait_for_running().await?;
        let run_status = self.status_snapshot();
        self.ensure_controller_state("run", false, false, false)?;
        self.clear_status_bits();
        info!(
            "xHCI: run state=running halted={} cnr={} reset={} irq_mode={}",
            run_status.halted,
            run_status.cnr,
            run_status.reset,
            if Self::POLL_ONLY_EVENT_HANDLER_SMOKE_VALID {
                "poll-only"
            } else {
                "interrupts"
            }
        );

        // self.reset_ports().await;

        Ok(())
    }

    fn clear_status_bits(&mut self) {
        self.reg.write().operational.usbsts.update_volatile(|r| {
            r.clear_host_system_error();
            r.clear_event_interrupt();
            r.clear_port_change_detect();
            r.clear_save_restore_error();
        });
        self.flush_controller_write();
    }

    async fn new_device(&mut self, info: DeviceAddressInfo) -> Result<Box<dyn DeviceOp>> {
        crate::debug_set_usb_probe_progress(3, info.root_port_id, info.port_id, 0, 0);
        let mut device = Device::new(self).await?;
        device.init(self, &info).await?;

        Ok(Box::new(device))
    }

    async fn init_ext_caps(&mut self) -> Result {
        let caps = self.extended_capabilities();
        let cap_count = caps.len();
        let ac64 = self
            .reg
            .read()
            .capability
            .hccparams1
            .read_volatile()
            .addressing_capability();
        let mut protocol_lines: Vec<String> = Vec::new();

        for cap in caps {
            match cap {
                ExtendedCapability::UsbLegacySupport(usb_legacy_support) => {
                    self.legacy_init(usb_legacy_support).await?;
                }
                ExtendedCapability::XhciSupportedProtocol(proto) => {
                    let h = proto.header.read_volatile();
                    protocol_lines.push(alloc::format!(
                        "usb{}.{}:{}..{}",
                        h.major_revision(),
                        h.minor_revision(),
                        h.compatible_port_offset(),
                        h.compatible_port_offset().saturating_add(h.compatible_port_count()).saturating_sub(1),
                    ));
                }
                _ => {}
            }
        }

        let protocol_summary = if protocol_lines.is_empty() {
            String::from("none")
        } else {
            protocol_lines.as_slice().join(", ")
        };
        info!(
            "xHCI: caps pci={:04X?}:{:04X?} addr={}b ext_caps={} protocols={}",
            self.pci_vendor_id,
            self.pci_device_id,
            if ac64 { 64 } else { 32 },
            cap_count,
            protocol_summary
        );

        Ok(())
    }

    async fn chip_hardware_reset(&mut self) -> Result {
        self.reg.write().operational.usbcmd.update_volatile(|c| {
            c.clear_run_stop();
        });

        self.wait_for_status("controller halt", |status| status.halted)
            .await?;

        self.wait_for_status("controller ready after stop", |status| !status.cnr)
            .await?;

        self.reg.write().operational.usbcmd.update_volatile(|f| {
            f.set_host_controller_reset();
        });

        self.wait_for_status("reset completion", |status| {
            status.halted && !status.cnr && !status.reset
        })
            .await?;
        let status = self.status_snapshot();
        info!(
            "xHCI: reset stop->ready->hc-reset complete halted={} cnr={} reset={} hse={} hce={}",
            status.halted,
            status.cnr,
            status.reset,
            status.hse,
            status.hce
        );

        Ok(())
    }

    fn extended_capabilities(&self) -> Vec<ExtendedCapability<MemMapper>> {
        let hccparams1 = self.reg.read().capability.hccparams1.read_volatile();
        let mapper = MemMapper {};
        let mut out = Vec::new();
        let mut l = match unsafe { List::new(self.reg.read().mmio_base, hccparams1, mapper) } {
            Some(v) => v,
            None => return out,
        };

        for one in &mut l {
            if let Ok(cap) = one {
                out.push(cap);
            } else {
                break;
            }
        }
        out
    }

    async fn legacy_init(&mut self, mut usb_legacy_support: UsbLegacySupport<MemMapper>) -> Result {
        usb_legacy_support.usblegsup.update_volatile(|r| {
            r.set_hc_os_owned_semaphore();
        });

        loop {
            let up = usb_legacy_support.usblegsup.read_volatile();
            if up.hc_os_owned_semaphore() && !up.hc_bios_owned_semaphore() {
                break;
            }
        }

        usb_legacy_support.usblegctlsts.update_volatile(|r| {
            r.clear_usb_smi_enable();
            r.clear_smi_on_host_system_error_enable();
            r.clear_smi_on_os_ownership_enable();
            r.clear_smi_on_pci_command_enable();
            r.clear_smi_on_bar_enable();

            r.clear_smi_on_bar();
            r.clear_smi_on_pci_command();
            r.clear_smi_on_os_ownership_change();
        });

        self.kernel.delay(Duration::from_millis(10));

        Ok(())
    }

    fn setup_max_device_slots(&mut self) -> u8 {
        let mut regs = self.reg.write();
        let max_slots = regs
            .capability
            .hcsparams1
            .read_volatile()
            .number_of_device_slots();

        regs.operational.config.update_volatile(|r| {
            r.set_max_device_slots_enabled(max_slots);
        });
        drop(regs);
        self.flush_controller_write();

        max_slots
    }

    pub(crate) fn dev(&self) -> Result<&DeviceContextList> {
        self.dev_ctx.as_ref().ok_or(USBError::NotInitialized)
    }

    pub(crate) fn dev_mut(&mut self) -> Result<&mut DeviceContextList> {
        self.dev_ctx.as_mut().ok_or(USBError::NotInitialized)
    }

    pub fn disable_irq(&mut self) {
        self.reg.write().operational.usbcmd.update_volatile(|r| {
            r.clear_interrupter_enable();
        });
        self.flush_controller_write();
    }

    pub fn enable_irq(&mut self) {
        self.reg.write().operational.usbcmd.update_volatile(|r| {
            r.set_interrupter_enable();
        });
        self.flush_controller_write();
    }

    fn setup_dcbaap(&mut self) -> Result {
        let dcbaa_addr = self.dev()?.dcbaa.dma_addr();
        self.reg.write().operational.dcbaap.update_volatile(|r| {
            r.set(dcbaa_addr.as_u64());
        });
        self.flush_controller_write();
        Ok(())
    }

    fn set_cmd_ring(&mut self) -> Result {
        let crcr = self.cmd.bus_addr();
        let cycle = self.cmd.cycle();

        self.reg.write().operational.crcr.update_volatile(|r| {
            r.set_command_ring_pointer(crcr.into());
            if cycle {
                r.set_ring_cycle_state();
            } else {
                r.clear_ring_cycle_state();
            }
        });
        self.flush_controller_write();

        Ok(())
    }

    fn setup_runtime_ring(&mut self) {
        let erstz = self.event_ring_info.erstz;
        let erdp = self.event_ring_info.erdp;
        let erstba = self.event_ring_info.erstba;

        {
            let mut reg = self.reg.write();
            let mut ir0 = reg.interrupter_register_set.interrupter_mut(0);

            ir0.erstsz.update_volatile(|r| r.set(erstz as _));
            ir0.erstba.update_volatile(|r| {
                r.set(erstba);
            });
            ir0.erdp.update_volatile(|r| {
                r.set_event_ring_dequeue_pointer(erdp);
                r.set_dequeue_erst_segment_index(0);
                r.clear_event_handler_busy();
            });

            ir0.imod.update_volatile(|im| {
                im.set_interrupt_moderation_interval(0x1F);
                im.set_interrupt_moderation_counter(0);
            });
        }
        self.flush_controller_write();
    }

    fn arm_irq(&mut self) {
        /* Keep the runtime ring programmed before RUN, but only arm signaling when
         * software is ready to consume events. */
        self.clear_status_bits();
        self.reg.write().operational.usbcmd.update_volatile(|r| {
            if Self::ARM_WRAP_EVENT_EXPERIMENT {
                r.set_enable_wrap_event();
            } else {
                r.clear_enable_wrap_event();
            }
        });
        self.flush_controller_write();

        {
            self.reg
                .write()
                .interrupter_register_set
                .interrupter_mut(0)
                .iman
                .update_volatile(|im| {
                    im.clear_interrupt_pending();
                    im.set_interrupt_enable();
                });
        }
        self.flush_controller_write();
    }

    fn setup_scratchpads(&mut self) -> Result<u16> {
        if Self::SKIP_SCRATCHPADS_EXPERIMENT {
            self.dev_mut()?.dcbaa.set(0, 0u64);
            self.flush_controller_write();
            self.scratchpad_buf_arr = None;
            return Ok(0);
        }

        let scratchpad_buf_arr = {
            let buf_count = {
                self.reg
                    .read()
                    .capability
                    .hcsparams2
                    .read_volatile()
                    .max_scratchpad_buffers()
            };
            if buf_count == 0 {
                return Ok(0);
            }
            let scratchpad_buf_arr = ScratchpadBufferArray::new(buf_count as _, &self.kernel)?;

            let bus_addr = scratchpad_buf_arr.bus_addr();

            self.dev_mut()?.dcbaa.set(0, bus_addr);
            self.flush_controller_write();
            scratchpad_buf_arr
        };
        let scratch_count = scratchpad_buf_arr.entries.len() as u16;

        self.scratchpad_buf_arr = Some(scratchpad_buf_arr);

        Ok(scratch_count)
    }

    fn start(&mut self) {
        self.reg.write().operational.usbcmd.update_volatile(|r| {
            r.set_run_stop();
        });
        self.flush_controller_write();
    }

    async fn wait_for_running(&mut self) -> Result {
        self.wait_for_status("controller run", |status| {
            !status.halted && !status.cnr && !status.reset
        })
            .await?;

        // 必须等待至少200ms，否则 port enable = false
        self.kernel.delay(Duration::from_millis(200));

        Ok(())
    }

    pub(crate) fn cmd_request(
        &mut self,
        trb: command::Allowed,
    ) -> impl Future<Output = core::result::Result<CommandCompletion, TransferError>> {
        if self.log_first_command_diagnostics && !self.first_command_logged {
            self.first_command_logged = true;
            self.log_pre_run_diagnostics("pre-first-command");
            debug!(
                "xHCI: first command submission trb={:?} pci={:04X?}:{:04X?}",
                trb,
                self.pci_vendor_id,
                self.pci_device_id
            );
        }
        self.cmd.cmd_request(trb)
    }

    pub(crate) fn is_64bit_ctx(&self) -> bool {
        self.reg
            .read()
            .capability
            .hccparams1
            .read_volatile()
            .context_size()
    }

    pub(crate) fn new_slot_bell(&self, slot: SlotId) -> SlotBell {
        SlotBell::new(slot, self.reg.read().clone())
    }

    pub(crate) async fn device_slot_assignment(
        &mut self,
    ) -> core::result::Result<SlotId, TransferError> {
        crate::debug_set_usb_probe_progress(40, 0, 0, 0, 0);
        info!("xhci lifecycle: slot=0 root_port=0 port=0 stage=enable-slot begin detail=0\n");
        let result = match self
            .cmd_request(command::Allowed::EnableSlot(command::EnableSlot::default()))
            .await
        {
            Ok(result) => result,
            Err(err) => {
                info!("xhci lifecycle: slot=0 root_port=0 port=0 stage=enable-slot failed: {:?}\n", err);
                return Err(err);
            }
        };

        let slot_id = result.slot_id();
        crate::debug_set_usb_probe_progress(40, 0, 0, slot_id, u32::from(slot_id));
        info!(
            "xhci lifecycle: slot={} root_port=0 port=0 stage=enable-slot ok detail={}\n",
            slot_id,
            slot_id
        );
        trace!("assigned slot id: {slot_id}");
        Ok(slot_id.into())
    }

    fn log_pre_run_diagnostics(&self, stage: &'static str) {
        let status = self.status_snapshot();
        let crcr = self.cmd.bus_addr();
        let dcbaap = self
            .reg
            .read()
            .operational
            .dcbaap
            .read_volatile()
            .get();
        let config = self
            .reg
            .read()
            .operational
            .config
            .read_volatile()
            .max_device_slots_enabled();
        let hcsparams2 = self.reg.read().capability.hcsparams2.read_volatile();
        let scratch_count = hcsparams2.max_scratchpad_buffers();
        let scratch_array = self
            .scratchpad_buf_arr
            .as_ref()
            .map(|arr| arr.bus_addr())
            .unwrap_or(0);
        debug!(
            "xHCI: diag stage={} pci={:04X?}:{:04X?} slots={} dcbaap=0x{:X} crcr=0x{:X} scratch_count={} scratch_array=0x{:X} halted={} cnr={} reset={} hse={} hce={}",
            stage,
            self.pci_vendor_id,
            self.pci_device_id,
            config,
            dcbaap,
            crcr.raw(),
            scratch_count,
            scratch_array,
            status.halted,
            status.cnr,
            status.reset,
            status.hse,
            status.hce
        );
    }

    fn log_init_setup_summary(
        &self,
        max_slots: u8,
        program_dcbaap: bool,
        program_crcr: bool,
        program_runtime_ring: bool,
        scratch_count: u16,
    ) {
        let dcbaap = self
            .reg
            .read()
            .operational
            .dcbaap
            .read_volatile()
            .get();
        info!(
            "xHCI: setup slots={} pre_run={{dcbaap:{},crcr:{},runtime:{}}} crcr=0x{:X} dcbaap=0x{:X} erst={{sz:{},ba:0x{:X},dp:0x{:X}}} scratchpads={} quirk={}",
            max_slots,
            if program_dcbaap { "set" } else { "skip" },
            if program_crcr { "set" } else { "skip" },
            if program_runtime_ring { "set" } else { "skip" },
            self.cmd.bus_addr().raw(),
            dcbaap,
            self.event_ring_info.erstz,
            self.event_ring_info.erstba,
            self.event_ring_info.erdp,
            scratch_count,
            if self.disable_staged_run_experiments {
                "conservative-pre-run"
            } else {
                "none"
            }
        );
    }
}

pub struct EventHandler {
    reg: UnsafeCell<XhciRegisters>,
    cmd_finished: Finished<CommandCompletion>,
    event_ring: UnsafeCell<EventRing>,
    transfer_result_handler: TransferResultHandler,
    ports: PortChangeWaker,
}

unsafe impl Send for EventHandler {}
unsafe impl Sync for EventHandler {}

impl EventHandler {
    fn new(
        reg: XhciRegisters,
        cmd_finished: Finished<CommandCompletion>,
        event_ring: EventRing,
        transfer_result_handler: TransferResultHandler,
        ports: PortChangeWaker,
    ) -> Self {
        Self {
            reg: UnsafeCell::new(reg),
            cmd_finished,
            event_ring: UnsafeCell::new(event_ring),
            transfer_result_handler,
            ports,
        }
    }

    #[allow(clippy::mut_from_ref)]
    fn event_ring(&self) -> &mut EventRing {
        unsafe { &mut *self.event_ring.get() }
    }

    #[allow(clippy::mut_from_ref)]
    fn reg(&self) -> &mut XhciRegisters {
        unsafe { &mut *self.reg.get() }
    }

    fn log_stop_port_snapshot(&self) {
        let reg = self.reg();
        for idx in 0..reg.port_register_set.len() {
            let port_id = (idx + 1) as u8;
            let port = reg.port_register_set.read_volatile_at(idx).portsc;
            let interesting = port.current_connect_status()
                || port.connect_status_change()
                || port.port_enabled_disabled_change()
                || port.warm_port_reset_change()
                || port.over_current_change()
                || port.port_reset_change()
                || port.port_link_state_change()
                || port.port_config_error_change();
            if !interesting {
                continue;
            }

            info!(
                "crabusb/xhci: stop-port port={} connect={} enabled={} reset={} speed={} pls={} csc={} pedc={} wrc={} occ={} prc={} plc={} cec={}",
                port_id,
                port.current_connect_status(),
                port.port_enabled_disabled(),
                port.port_reset(),
                port.port_speed(),
                port.port_link_state(),
                port.connect_status_change(),
                port.port_enabled_disabled_change(),
                port.warm_port_reset_change(),
                port.over_current_change(),
                port.port_reset_change(),
                port.port_link_state_change(),
                port.port_config_error_change(),
            );
        }
    }

    fn clean_event_ring(&self) -> (Event, bool) {
        use xhci::ring::trb::event::Allowed;
        let mut event = Event::Nothing;
        let mut drained = false;

        while let Some(allowed) = self.event_ring().next() {
            drained = true;
            match allowed {
                Allowed::CommandCompletion(c) => {
                    let addr = c.command_trb_pointer();
                    let completion_code = c
                        .completion_code()
                        .ok()
                        .map(|code| code as u8)
                        .unwrap_or(0xFF);
                    debug_record_event(c.slot_id(), 0xFF, completion_code, 0, addr);
                    // trace!("[Command] << {allowed:?} @{addr:X}");
                    self.cmd_finished.set_finished(addr.into(), c);
                }
                Allowed::PortStatusChange(st) => {
                    // debug!("Port {} status change event", st.port_id());
                    // let idx = (st.port_id() - 1) as usize;
                    let port_id = st.port_id();
                    self.ports.set_port_changed(port_id);

                    event = Event::PortChange {
                        port: st.port_id() as _,
                    };
                }
                Allowed::TransferEvent(c) => {
                    let slot_id = c.slot_id();
                    let ep_id = c.endpoint_id();
                    let ptr = c.trb_pointer();
                    let completion_code = c
                        .completion_code()
                        .ok()
                        .map(|code| code as u8)
                        .unwrap_or(0xFF);
                    debug_record_event(
                        slot_id,
                        ep_id,
                        completion_code,
                        c.trb_transfer_length(),
                        ptr,
                    );

                    unsafe {
                        self.transfer_result_handler
                            .set_finished(slot_id, ep_id, ptr.into(), c)
                    };
                }
                Allowed::HostController(c) => {
                    info!("crabusb/xhci: host-controller event {:?}", c);
                }
                Allowed::Doorbell(c) => {
                    info!("crabusb/xhci: doorbell event {:?}", c);
                }
                Allowed::BandwidthRequest(c) => {
                    info!("crabusb/xhci: bandwidth-request event {:?}", c);
                }
                Allowed::DeviceNotification(c) => {
                    info!("crabusb/xhci: device-notification event {:?}", c);
                }
                Allowed::MfindexWrap(c) => {
                    info!("crabusb/xhci: mfindex-wrap event {:?}", c);
                }
            }
        }
        (event, drained)
    }
}

impl EventHandlerOp for EventHandler {
    fn handle_event(&self) -> Event {
        let sts = self.reg().operational.usbsts.read_volatile();
        if sts.host_system_error() || sts.host_controller_error() {
            info!(
                "crabusb/xhci: stopping event pump hse={} hce={} halted={} cnr={} pcd={} eint={}",
                sts.host_system_error(),
                sts.host_controller_error(),
                sts.hc_halted(),
                sts.controller_not_ready(),
                sts.port_change_detect(),
                sts.event_interrupt(),
            );
            self.log_stop_port_snapshot();
            return Event::Stopped;
        }
        let irq_pending = self
            .reg()
            .interrupter_register_set
            .interrupter_mut(0)
            .iman
            .read_volatile()
            .interrupt_pending();
        let (res, drained) = self.clean_event_ring();

        if !sts.event_interrupt() && !irq_pending && !drained {
            return Event::Nothing;
        }

        if sts.event_interrupt() {
            self.reg().operational.usbsts.update_volatile(|r| {
                r.clear_event_interrupt();
            });
        }

        // 【关键】GIC 中断模式下，需要手动清除 IMAN.IP
        // 参考: Linux xhci_irq() in xhci-ring.c:3054-3059
        let mut irq = self.reg().interrupter_register_set.interrupter_mut(0);
        if irq_pending {
            irq.iman.update_volatile(|r| {
                r.clear_interrupt_pending();
            });
        }

        if drained || sts.event_interrupt() || irq_pending {
            let erdp = self.event_ring().erdp();
            irq.erdp.update_volatile(|r| {
                r.set_event_ring_dequeue_pointer(erdp);
                r.clear_event_handler_busy();
            });
        }

        res
    }
}
