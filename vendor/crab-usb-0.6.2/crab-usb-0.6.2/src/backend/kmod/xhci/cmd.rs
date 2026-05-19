use alloc::sync::Arc;
use core::sync::atomic::{Ordering, fence};

use mbarrier::wmb;
use spin::{Mutex, RwLock};
use usb_if::err::TransferError;
use xhci::{
    registers::doorbell,
    ring::trb::{command, event::CommandCompletion},
};

use super::{reg::XhciRegisters, ring::SendRing};
use crate::{
    BusAddr, debug_record_submit, err::ConvertXhciError, osal::Kernel, queue::Finished,
};

#[derive(Clone)]
pub struct CommandRing(Arc<Mutex<Inner>>);

impl CommandRing {
    fn submit(&self, trb: command::Allowed) -> BusAddr {
        let mut inner = self.0.lock();
        let trb_addr = inner.ring.enque_command(trb);
        debug_record_submit(0xFF, 0, 0, trb_addr.raw());
        wmb();
        fence(Ordering::SeqCst);
        #[cfg(target_arch = "x86")]
        unsafe {
            core::arch::x86::_mm_mfence();
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::x86_64::_mm_mfence();
        }
        inner
            .reg
            .write()
            .doorbell
            .write_volatile_at(0, doorbell::Register::default());
        let _ = inner.reg.read().operational.usbsts.read_volatile();
        fence(Ordering::SeqCst);
        #[cfg(target_arch = "x86")]
        unsafe {
            core::arch::x86::_mm_mfence();
        }
        #[cfg(target_arch = "x86_64")]
        unsafe {
            core::arch::x86_64::_mm_mfence();
        }
        trb_addr
    }

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

    pub fn submit_for_poll(&mut self, trb: command::Allowed) -> BusAddr {
        self.submit(trb)
    }

    pub fn poll_finished(&self, addr: BusAddr) -> Option<CommandCompletion> {
        let inner = self.0.lock();
        inner.ring.get_finished(addr)
    }

    pub async fn cmd_request(
        &mut self,
        trb: command::Allowed,
    ) -> Result<CommandCompletion, TransferError> {
        let trb_addr = self.submit(trb);
        let fur = {
            let inner = self.0.lock();
            inner.ring.take_finished_future(trb_addr)
        };

        let res = fur.await;

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
