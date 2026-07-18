use alloc::{collections::BTreeMap, sync::Arc, vec::Vec};
use xhci::ring::trb::event::TransferEvent;

use crate::{BusAddr, queue::Finished};

use super::{TraceSampler, reg::XhciRegistersShared, ring::SendRing, sync::IrqLock};

static TRANSFER_DISPATCH_TRACE: TraceSampler = TraceSampler::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TransferId(pub(crate) BusAddr);

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct TransQueueId {
    slot_id: u8,
    ep_id: u8,
}

#[derive(Clone)]
pub struct TransferResultHandler {
    inner: Arc<IrqLock<BTreeMap<TransQueueId, Vec<Finished<TransferEvent>>>>>,
}

unsafe impl Send for TransferResultHandler {}

impl TransferResultHandler {
    pub fn new(reg: XhciRegistersShared) -> Self {
        Self {
            inner: Arc::new(IrqLock::new(BTreeMap::new(), reg)),
        }
    }

    pub fn register_queue(&mut self, slot_id: u8, ep_id: u8, ring: &SendRing<TransferEvent>) {
        let id = TransQueueId { slot_id, ep_id };
        let handle = ring.finished_handle();
        let mut queues_by_id = self.inner.lock();
        let queues = queues_by_id.entry(id).or_default();
        // Endpoint reconfiguration currently leaves old completion queues registered.
        // Prefer the newest ring so reused DMA addresses cannot deliver events to
        // a stale queue and starve the live endpoint.
        queues.insert(0, handle);
    }

    /// Marks a queue completion from the xHCI interrupt path.
    ///
    /// This runs while handling an interrupt, so it must not acquire OS-facing
    /// locks or call into device/file managers. Queue registration is protected
    /// by `IrqLock::lock`, which disables this interrupt source before mutating
    /// the map. The IRQ hot path uses `force_use` and only touches the
    /// pre-registered queue completion slot, then wakes queue-local waiters.
    pub unsafe fn set_finished(&self, slot_id: u8, ep_id: u8, ptr: BusAddr, res: TransferEvent) {
        let queue_id = TransQueueId { slot_id, ep_id };
        if let Some(queues) = unsafe { self.inner.force_use().get(&queue_id) } {
            if let Some(seen) = TRANSFER_DISPATCH_TRACE.sample() {
                trace!(
                    "xhci: transfer dispatch seen={} slot={} ep={} ptr={:#x} code={:?} remaining={}",
                    seen,
                    slot_id,
                    ep_id,
                    ptr.raw(),
                    res.completion_code(),
                    res.trb_transfer_length()
                );
            }
            let mut event = res;
            for q in queues {
                match q.set_finished(ptr, event) {
                    Ok(()) => return,
                    Err(res) => event = res,
                }
            }
            warn!(
                "xhci: transfer event matched endpoint but no ring slot={} ep={} ptr={:#x} code={:?} len={}",
                slot_id,
                ep_id,
                ptr.raw(),
                event.completion_code(),
                event.trb_transfer_length()
            );
        } else {
            warn!(
                "xhci: transfer event has no endpoint queue slot={} ep={} ptr={:#x} code={:?} len={}",
                slot_id,
                ep_id,
                ptr.raw(),
                res.completion_code(),
                res.trb_transfer_length()
            );
        }
    }
}
