//! Bounded headless export of one Spirit background frame.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;
use trueos_vm::vmcall::{OP_BP_LILYVFX_BACKGROUND_V1, STATUS_OK, call_with_payload_reported_len};

pub(crate) const CONTROL_BYTES: usize = 33 * 4;
const FRAME_BYTES: usize = 256 * 256 * 4;
pub(crate) const CHUNK_BYTES: usize = FRAME_BYTES / 2;

struct PendingFrame {
    ticket: u64,
    pixels: Vec<u8>,
}

// A complete frame exceeds the VM-call payload. Hold one response per guest
// until its second read or teardown. Tickets reject a superseded read.
static NEXT_TICKET: AtomicU64 = AtomicU64::new(1);
static PENDING: [Mutex<Option<PendingFrame>>; crate::hv::TRUEOS_VM_ID_LIMIT] =
    [const { Mutex::new(None) }; crate::hv::TRUEOS_VM_ID_LIMIT];

pub(crate) fn discard_guest(vm_id: u8) {
    if let Some(slot) = PENDING.get(vm_id as usize) {
        *slot.lock() = None;
    }
}

pub(crate) fn capture_guest(vm_id: u8, controls: &[u8], first: &mut [u8]) -> Result<u64, i32> {
    let slot = PENDING.get(vm_id as usize).ok_or(-22)?;
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(FRAME_BYTES).map_err(|_| -12)?;
    pixels.resize(FRAME_BYTES, 0);
    crate::intel::gpgpu::capture_spirit_background(controls, &mut pixels)?;
    let ticket = NEXT_TICKET.fetch_add(1, Ordering::Relaxed);
    first.copy_from_slice(&pixels[..CHUNK_BYTES]);
    *slot.lock() = Some(PendingFrame { ticket, pixels });
    Ok(ticket)
}

pub(crate) fn finish_guest(vm_id: u8, ticket: u64, tail: &mut [u8]) -> Result<(), i32> {
    let mut slot = PENDING.get(vm_id as usize).ok_or(-22)?.lock();
    let frame = slot
        .as_ref()
        .filter(|frame| frame.ticket == ticket)
        .ok_or(-116)?;
    tail.copy_from_slice(&frame.pixels[CHUNK_BYTES..]);
    *slot = None;
    Ok(())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lilyvfx_background_v1(
    controls: *const u8,
    controls_len: usize,
    output: *mut u8,
    output_len: usize,
) -> i32 {
    if controls.is_null()
        || output.is_null()
        || controls_len != CONTROL_BYTES
        || output_len != FRAME_BYTES
    {
        return -22;
    }

    // Copy before borrowing the output so overlapping caller storage is safe.
    let mut page = [0u8; CONTROL_BYTES];
    page.copy_from_slice(unsafe { core::slice::from_raw_parts(controls, CONTROL_BYTES) });
    let output = unsafe { core::slice::from_raw_parts_mut(output, FRAME_BYTES) };
    if crate::hv::current_hull_guest_context_vm_id().is_none() {
        return crate::intel::gpgpu::capture_spirit_background(&page, output)
            .map_or_else(|error| error, |()| 0);
    }

    let (head, tail) = output.split_at_mut(CHUNK_BYTES);
    let (status, ticket, size) =
        call_with_payload_reported_len(OP_BP_LILYVFX_BACKGROUND_V1, 0, 0, &page, head);
    if status != STATUS_OK {
        return -5;
    }
    if size == 0 {
        return match i32::try_from(ticket as i64) {
            Ok(error) if error < 0 => error,
            _ => -5,
        };
    }
    if size != CHUNK_BYTES {
        return -5;
    }
    let (status, result, size) =
        call_with_payload_reported_len(OP_BP_LILYVFX_BACKGROUND_V1, 1, ticket, &[], tail);
    if status != STATUS_OK {
        return -5;
    }
    if result != 0 {
        return result as i64 as i32;
    }
    if size != CHUNK_BYTES {
        return -5;
    }
    0
}
