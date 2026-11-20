#![allow(dead_code)]
extern crate alloc;
#[macro_use]
extern crate anyhow;
macro_rules! trace { ($($arg:tt)*) => {}; }

pub mod osal { pub type Kernel = dma_api::DeviceDma; }
pub mod backend { pub mod ty { pub mod transfer { @TRANSFER_TYPE@ } } }
mod transfer_impl { @TRANSFER_IMPL@ }
use backend::ty::transfer::{Transfer, TransferKind};
use usb_if::{err::TransferError, transfer::Direction};
use xhci::ring::trb::event::{TransferEvent, CompletionCode};

struct TraceSampler;
impl TraceSampler { fn sample(&self) -> Option<usize> { None } }
static TRANSFER_LENGTH_TRACE: TraceSampler = TraceSampler;

#[derive(Clone, Copy, PartialEq, Eq)]
struct TransferId(u64);
struct ControlTd { requested: usize, actual: Option<usize> }
struct NormalTrbTd { trb: TransferId, offset: usize, len: usize }
struct NormalTd { trbs: Vec<NormalTrbTd> }
impl NormalTd { @NORMAL_ACTUAL@ @NORMAL_REQUESTED@ }
#[derive(Clone, Copy)]
struct IsoPacketTd { trb: TransferId, actual: Option<usize> }
enum SubmittedTdKind { Normal(NormalTd), Control(ControlTd), Iso { packets: Vec<IsoPacketTd> } }
struct SubmittedTd { transfer: Transfer, kind: SubmittedTdKind }
struct Endpoint;
impl Endpoint { @COMPLETION@ }
trait CompletionResult { fn to_result(self) -> Result<(), TransferError>; }
impl CompletionResult for CompletionCode {
    fn to_result(self) -> Result<(), TransferError> {
        match self {
            CompletionCode::Success | CompletionCode::ShortPacket => Ok(()),
            _ => Err(TransferError::Other(anyhow!("completion failure"))),
        }
    }
}
@ISO_ACTUAL@

#[cfg(test)]
mod tests {
    use super::*;
    use core::{alloc::Layout, num::NonZeroUsize, ptr::NonNull};
    use dma_api::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Backend { cache_ops: AtomicUsize }
    static BACKEND: Backend = Backend { cache_ops: AtomicUsize::new(0) };
    impl DmaOp for Backend {
        fn page_size(&self) -> usize { 4096 }
        unsafe fn alloc_contiguous(&self, _constraints: DmaConstraints, layout: Layout) -> Option<DmaAllocHandle> {
            let ptr = NonNull::new(unsafe { std::alloc::alloc_zeroed(layout) })?;
            Some(unsafe { DmaAllocHandle::new(ptr, ptr, (ptr.as_ptr() as u64).into(), layout) })
        }
        unsafe fn dealloc_contiguous(&self, handle: DmaAllocHandle) {
            unsafe { std::alloc::dealloc(handle.allocation_ptr().as_ptr(), handle.layout()) };
        }
        unsafe fn alloc_coherent(&self, _: DmaConstraints, _: Layout) -> Option<DmaAllocHandle> { panic!("coherent device uses normal mapping") }
        unsafe fn dealloc_coherent(&self, _: DmaAllocHandle) -> Result<(), DmaError> { unreachable!() }
        unsafe fn map_streaming(&self, constraints: DmaConstraints, addr: NonNull<u8>, size: NonZeroUsize, _: DmaDirection) -> Result<DmaMapHandle, DmaError> {
            let layout = Layout::from_size_align(size.get(), constraints.align)?;
            let bounce = NonNull::new(unsafe { std::alloc::alloc(layout) }).ok_or(DmaError::NoMemory)?;
            // Poison rather than zero: stale/unreceived bounce bytes must not be copied back.
            unsafe { bounce.as_ptr().write_bytes(0xCC, size.get()) };
            Ok(unsafe { DmaMapHandle::new(addr, (bounce.as_ptr() as u64).into(), layout, Some(bounce)) })
        }
        unsafe fn unmap_streaming(&self, handle: DmaMapHandle) {
            unsafe { std::alloc::dealloc(handle.bounce_ptr().unwrap().as_ptr(), handle.layout()) };
        }
        fn flush(&self, _: NonNull<u8>, _: usize) { self.cache_ops.fetch_add(1, Ordering::Relaxed); }
        fn invalidate(&self, _: NonNull<u8>, _: usize) { self.cache_ops.fetch_add(1, Ordering::Relaxed); }
        fn flush_invalidate(&self, _: NonNull<u8>, _: usize) { self.cache_ops.fetch_add(1, Ordering::Relaxed); }
    }
    fn dma() -> DeviceDma {
        DeviceDma::new(DmaDeviceInfo::new(DmaDomainId::Direct, DmaCoherency::Coherent, DmaConstraints::new(u64::MAX)), &BACKEND)
    }
    fn transfer(buffer: &mut [u8], kind: TransferKind, direction: Direction) -> Transfer {
        Transfer::new(&dma(), kind, direction, Some((NonNull::new(buffer.as_mut_ptr()).unwrap(), buffer.len()))).unwrap()
    }
    fn event(remaining: u32) -> TransferEvent {
        TransferEvent::try_from([0, 0, (1 << 24) | remaining, 32 << 10]).unwrap()
    }
    fn bounce(t: &Transfer) -> &mut [u8] {
        let mapping = t.mapping.as_ref().unwrap();
        unsafe { core::slice::from_raw_parts_mut(mapping.bounce_ptr().unwrap().as_ptr(), mapping.bytes_len()) }
    }

    #[test]
    fn coherent_out_still_copies_to_bounce_before_device_ownership() {
        let mut buffer = [1, 2, 3, 4];
        let transfer = transfer(&mut buffer, TransferKind::Bulk, Direction::Out);
        assert_eq!(bounce(&transfer), &[0xCC; 4]);
        transfer.prepare_for_device_all();
        assert_eq!(bounce(&transfer), &buffer);
        assert_eq!(BACKEND.cache_ops.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn short_in_copies_only_completed_prefix() {
        let mut buffer = [0xA5; 8];
        let transfer = transfer(&mut buffer, TransferKind::Bulk, Direction::In);
        transfer.prepare_for_device_all();
        bounce(&transfer)[..3].copy_from_slice(&[1, 2, 3]);
        let normal = NormalTd { trbs: vec![NormalTrbTd { trb: TransferId(1), offset: 0, len: 8 }] };
        let completed = Endpoint.transfer_from_completion(SubmittedTd { transfer, kind: SubmittedTdKind::Normal(normal) }, TransferId(1), event(5)).unwrap();
        assert_eq!(completed.transfer_len, 3);
        drop(completed);
        assert_eq!(buffer, [1, 2, 3, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5]);
    }

    #[test]
    fn short_iso_preserves_packet_offsets_and_unreceived_gaps() {
        let mut buffer = [0xA5; 12];
        let transfer = transfer(&mut buffer, TransferKind::Isochronous { packet_lengths: vec![4, 4, 4] }, Direction::In);
        transfer.prepare_for_device_all();
        bounce(&transfer)[..2].copy_from_slice(&[1, 2]);
        bounce(&transfer)[4..7].copy_from_slice(&[3, 4, 5]);
        let packets = vec![
            IsoPacketTd { trb: TransferId(1), actual: Some(2) },
            IsoPacketTd { trb: TransferId(2), actual: Some(3) },
            IsoPacketTd { trb: TransferId(3), actual: Some(0) },
        ];
        let completed = Endpoint.transfer_from_completion(SubmittedTd { transfer, kind: SubmittedTdKind::Iso { packets } }, TransferId(3), event(0)).unwrap();
        assert_eq!(completed.transfer_len, 5);
        assert_eq!(completed.iso_packet_actual_lengths, [2, 3, 0]);
        drop(completed);
        assert_eq!(buffer, [1, 2, 0xA5, 0xA5, 3, 4, 5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5]);
    }

    #[test]
    fn control_in_uses_reported_actual_length() {
        let mut buffer = [0xA5; 8];
        let transfer = transfer(&mut buffer, TransferKind::Bulk, Direction::In);
        transfer.prepare_for_device_all();
        bounce(&transfer)[..2].copy_from_slice(&[9, 8]);
        let kind = SubmittedTdKind::Control(ControlTd { requested: 8, actual: Some(2) });
        let completed = Endpoint.transfer_from_completion(SubmittedTd { transfer, kind }, TransferId(1), event(0)).unwrap();
        drop(completed);
        assert_eq!(buffer, [9, 8, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5, 0xA5]);
    }
}
