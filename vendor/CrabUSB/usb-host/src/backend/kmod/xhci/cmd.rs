use alloc::sync::Arc;

use mbarrier::wmb;
use spin::{Mutex, RwLock};
use usb_if::err::TransferError;
use xhci::{
    registers::doorbell,
    ring::trb::{command, event::CommandCompletion},
};

use super::{reg::XhciRegisters, ring::SendRing};
use crate::{err::ConvertXhciError, osal::Kernel, queue::Finished};

#[derive(Clone)]
pub struct CommandRing(Arc<Mutex<Inner>>);

impl CommandRing {
    pub fn new(
        direction: crate::osal::DmaDirection,
        dma: &Kernel,
        reg: Arc<RwLock<XhciRegisters>>,
    ) -> crate::err::Result<Self> {
        let ring = SendRing::new(direction, dma)?;
        let inner = Inner { ring, reg };
        Ok(Self(Arc::new(Mutex::new(inner))))
    }

    pub fn bus_addr(&self) -> crate::BusAddr {
        let inner = self.0.lock();
        inner.ring.bus_addr()
    }

    pub fn cycle(&self) -> bool {
        let inner = self.0.lock();
        inner.ring.cycle()
    }

    pub fn finished_handle(&self) -> Finished<CommandCompletion> {
        let inner = self.0.lock();
        inner.ring.finished_handle()
    }

    pub async fn cmd_request(
        &mut self,
        trb: command::Allowed,
    ) -> Result<CommandCompletion, TransferError> {
        trace!("xhci: command request begin");
        let fur = {
            trace!("xhci: command request locking ring");
            let mut inner = self.0.lock();
            trace!("xhci: command request ring locked");
            let trb_addr = inner.ring.enque_command(trb);
            trace!("xhci: command request enqueued trb={:#x}", trb_addr.raw());
            let fur = inner.ring.take_finished_future(trb_addr);
            trace!("xhci: command request waiter registered trb={:#x}", trb_addr.raw());
            wmb();
            trace!("xhci: command request write doorbell trb={:#x}", trb_addr.raw());
            {
                let mut regs = inner.reg.write();
                let before = regs.operational.crcr.read_volatile();
                let usbsts_before = regs.operational.usbsts.read_volatile();
                regs.doorbell
                    .write_volatile_at(0, doorbell::Register::default());
                let after = regs.operational.crcr.read_volatile();
                let usbsts_after = regs.operational.usbsts.read_volatile();
                trace!(
                    "xhci: command doorbell trb={:#x} crr_before={} crr_after={} halted_before={} halted_after={} hse_before={} hse_after={} hce_before={} hce_after={}",
                    trb_addr.raw(),
                    before.command_ring_running(),
                    after.command_ring_running(),
                    usbsts_before.hc_halted(),
                    usbsts_after.hc_halted(),
                    usbsts_before.host_system_error(),
                    usbsts_after.host_system_error(),
                    usbsts_before.host_controller_error(),
                    usbsts_after.host_controller_error()
                );
            }
            trace!("xhci: command request waiting completion trb={:#x}", trb_addr.raw());
            fur
        };

        let res = fur.await;
        trace!("xhci: command request completion received");

        match res.completion_code() {
            Ok(code) => code.to_result()?,
            Err(e) => Err(TransferError::Other(anyhow!("Command failed: {e:?}")))?,
        }

        Ok(res)
    }
}

struct Inner {
    ring: SendRing<CommandCompletion>,
    reg: Arc<RwLock<XhciRegisters>>,
}
