use core::{cell::Cell, mem::size_of, ptr, slice};

use spin::Mutex;

use super::{PIPECONF_A, PIPECONF_STATE};
use crate::intel::{
    AVC_CONTROL_GGTT_BASE, DISPLAY_INTERACTION_GGTT_LIMIT, Dev, GEN8_PAGE_PRESENT,
    GGTT_ALIAS_BASE_OFF, GGTT_PAGE_BYTES, alloc_ggtt_backing, dma_flush, ggtt_invalidate, map_ggtt,
    mmio_read, mmio_write, read_ggtt_pte,
};

// Pipe A, DSB engine 0. See the Intel display PRM's Display State Buffer chapter.
const DSB_HEAD: usize = 0x70B00;
const DSB_TAIL: usize = DSB_HEAD + 0x04;
const DSB_CTRL: usize = DSB_HEAD + 0x08;
const DSB_INTERRUPT: usize = DSB_HEAD + 0x28;
const DSB_CURRENT_HEAD: usize = DSB_HEAD + 0x2C;
const DSB_ENABLE: u32 = 1 << 31;
const DSB_BUSY: u32 = 1;
const DSB_IRQ_POLL_ERROR: u32 = 1 << 1;
const DSB_IRQ_RESPONSE_TIMEOUT: u32 = 1 << 2;
const DSB_IRQ_GTT_FAULT: u32 = 1 << 3;
const DSB_IRQ_STATUS: u32 = 1 | DSB_IRQ_POLL_ERROR | DSB_IRQ_RESPONSE_TIMEOUT | DSB_IRQ_GTT_FAULT;

const PIPE_ENABLE: u32 = 1 << 31;
const LUT_INDEX: u32 = 0x490A8;
const LUT_DATA: u32 = 0x490AC;
const LUT_AUTO_INCREMENT: u32 = 1 << 13;
const LUT_ENTRIES: usize = 17 * 17 * 17;
const DSB_MMIO_WRITE_ALL_BYTES: u32 = 0x01F0_0000;
const DSB_INDEXED_WRITE: u32 = 0x0900_0000;

const CACHELINE_BYTES: usize = 64;
const DATA_AT: usize = 4;
const RESTORE_AT: usize = DATA_AT + LUT_ENTRIES.next_multiple_of(2);
const COMMAND_BYTES: usize =
    ((RESTORE_AT + 2) * size_of::<u32>()).next_multiple_of(CACHELINE_BYTES);
const PAGE_BYTES: usize = GGTT_PAGE_BYTES as usize;
const BACKING_BYTES: usize = (COMMAND_BYTES + CACHELINE_BYTES).next_multiple_of(PAGE_BYTES);
const GGTT_BASE: u64 = 0x4800_0000;
const GGTT_END: u64 = GGTT_BASE + BACKING_BYTES as u64;
const GGTT_MMIO_END: usize = GGTT_ALIAS_BASE_OFF + (GGTT_END / GGTT_PAGE_BYTES) as usize * 8;
const _: () = {
    assert!(GGTT_BASE.is_multiple_of(GGTT_PAGE_BYTES));
    assert!(GGTT_BASE >= DISPLAY_INTERACTION_GGTT_LIMIT);
    assert!(GGTT_END <= AVC_CONTROL_GGTT_BASE);
};

#[derive(Clone, Copy)]
struct Backing {
    phys: u64,
    virt: usize,
}

#[derive(PartialEq, Eq)]
enum Phase {
    Idle,
    Active,
    Quarantined,
}

struct State {
    backing: Option<Backing>,
    phase: Phase,
}

static STATE: Mutex<State> = Mutex::new(State {
    backing: None,
    phase: Phase::Idle,
});

#[derive(Clone, Copy)]
pub(crate) enum LutDsbPoll {
    Pending,
    Complete,
    Failed(&'static str),
    Uncertain(&'static str),
}

#[derive(Clone, Copy)]
enum UploadProgress {
    Running { saw_busy: bool },
    Resetting { interrupt: u32 },
    Finished(LutDsbPoll),
}

pub(crate) struct LutDsbUpload {
    device: Dev,
    tail: u32,
    progress: Cell<UploadProgress>,
}

fn pipe_a_active(dev: Dev) -> bool {
    let config = mmio_read(dev, PIPECONF_A);
    config != u32::MAX && config & (PIPE_ENABLE | PIPECONF_STATE) == PIPE_ENABLE | PIPECONF_STATE
}

fn check_idle(dev: Dev, state: &State) -> Result<(), &'static str> {
    if state.phase != Phase::Idle {
        return Err("chroma-dsb-port-blocked");
    }
    if dev.device_id != 0x4680
        || dev.revision_id != 0x0C
        || crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) != 0x4680_8086
        || crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8 != 0x0C
    {
        return Err("chroma-dsb-unsupported-device");
    }
    if dev.mmio.is_null() || dev.mmio_len < GGTT_MMIO_END || !pipe_a_active(dev) {
        return Err("chroma-dsb-pipe-a-unavailable");
    }
    if mmio_read(dev, DSB_CTRL) != 0 || mmio_read(dev, DSB_INTERRUPT) != 0 {
        return Err("chroma-dsb-engine-not-idle");
    }
    Ok(())
}

// Call under DISPLAY_COLOR_ROUTE_LOCK before accessing the shared LUT port.
pub(crate) fn check_lut_dsb_idle(dev: Dev) -> Result<(), &'static str> {
    check_idle(dev, &STATE.lock())
}

pub(crate) fn lut_dsb_port_blocked() -> bool {
    STATE.lock().phase != Phase::Idle
}

fn backing(dev: Dev, state: &mut State) -> Result<Backing, &'static str> {
    let page_offsets = || (0..BACKING_BYTES as u64).step_by(PAGE_BYTES);
    if state.backing.is_none() {
        if !page_offsets().all(|offset| read_ggtt_pte(dev, GGTT_BASE + offset) == Some(0)) {
            return Err("chroma-dsb-ggtt-occupied");
        }
        let (phys, virt) = alloc_ggtt_backing(BACKING_BYTES, PAGE_BYTES)
            .ok_or("chroma-dsb-dma-allocation-failed")?;
        // Retain the allocation even if installing or checking its PTEs fails.
        state.backing = Some(Backing {
            phys,
            virt: virt as usize,
        });
        if !map_ggtt(dev, phys, BACKING_BYTES, GGTT_BASE) {
            return Err("chroma-dsb-ggtt-map-failed");
        }
        ggtt_invalidate(dev);
    }
    let buffer = state.backing.expect("DSB backing assigned");
    if !page_offsets().all(|offset| {
        read_ggtt_pte(dev, GGTT_BASE + offset) == Some((buffer.phys + offset) | GEN8_PAGE_PRESENT)
    }) {
        return Err("chroma-dsb-ggtt-readback-mismatch");
    }
    Ok(buffer)
}

fn write_commands(buffer: Backing, entries: &[u32], restore_index: u32, bytes: usize) {
    let ptr = buffer.virt as *mut u8;
    let words = bytes / size_of::<u32>();
    // This is the permanent, aligned buffer for the idle DSB. Initialize the
    // instruction padding and trailing NOOPs before handing it back to DMA.
    let commands = unsafe {
        ptr::write_bytes(ptr, 0, bytes);
        slice::from_raw_parts_mut(ptr.cast::<u32>(), words)
    };
    commands[..DATA_AT].copy_from_slice(&[
        LUT_AUTO_INCREMENT,
        DSB_MMIO_WRITE_ALL_BYTES | LUT_INDEX,
        LUT_ENTRIES as u32,
        DSB_INDEXED_WRITE | LUT_DATA,
    ]);
    commands[DATA_AT..DATA_AT + LUT_ENTRIES].copy_from_slice(entries);
    commands[RESTORE_AT..RESTORE_AT + 2]
        .copy_from_slice(&[restore_index, DSB_MMIO_WRITE_ALL_BYTES | LUT_INDEX]);
    dma_flush(ptr, bytes);
}

/// The caller holds the route lock and keeps the LUT reserved through retirement.
pub(crate) fn start_lut_dsb(
    dev: Dev,
    entries: &[u32],
    restore_index: u32,
) -> Result<LutDsbUpload, &'static str> {
    if entries.len() != LUT_ENTRIES || entries.iter().any(|entry| entry >> 30 != 0) {
        return Err("chroma-dsb-lut-entry-range");
    }
    if restore_index == u32::MAX {
        return Err("chroma-dsb-index-unavailable");
    }
    let mut state = STATE.lock();
    check_idle(dev, &state)?;
    let buffer = backing(dev, &mut state)?;

    // Choose a tail distinct from the observed current head. A previous run's
    // final pointer must never look like retirement of an ignored tail write.
    let current_head = mmio_read(dev, DSB_CURRENT_HEAD);
    if current_head == u32::MAX {
        return Err("chroma-dsb-current-head-unavailable");
    }
    let mut bytes = COMMAND_BYTES;
    if current_head == GGTT_BASE as u32 + bytes as u32 {
        bytes += CACHELINE_BYTES;
    }
    let tail = GGTT_BASE as u32 + bytes as u32;
    write_commands(buffer, entries, restore_index, bytes);

    mmio_write(dev, DSB_CTRL, DSB_ENABLE);
    if mmio_read(dev, DSB_CTRL) != DSB_ENABLE {
        return Err("chroma-dsb-engine-enable-failed");
    }
    mmio_write(dev, DSB_HEAD, GGTT_BASE as u32);
    if mmio_read(dev, DSB_HEAD) != GGTT_BASE as u32 {
        mmio_write(dev, DSB_CTRL, 0);
        mmio_read(dev, DSB_CTRL);
        return Err("chroma-dsb-head-write-failed");
    }

    state.phase = Phase::Active;
    // Tail posting is the trigger. Every outcome after this requires a token.
    mmio_write(dev, DSB_TAIL, tail);
    Ok(LutDsbUpload {
        device: dev,
        tail,
        progress: Cell::new(UploadProgress::Running { saw_busy: false }),
    })
}

fn interrupt_error(status: u32) -> &'static str {
    if status & DSB_IRQ_GTT_FAULT != 0 {
        "chroma-dsb-ggtt-fault"
    } else if status & DSB_IRQ_RESPONSE_TIMEOUT != 0 {
        "chroma-dsb-mmio-response-timeout"
    } else if status & DSB_IRQ_POLL_ERROR != 0 {
        "chroma-dsb-poll-error"
    } else {
        "chroma-dsb-unexpected-program-interrupt"
    }
}

impl LutDsbUpload {
    /// Poll under the route lock. Release the port only after a proven drain.
    pub(crate) fn poll(&self, dev: Dev) -> LutDsbPoll {
        let progress = self.progress.get();
        if let UploadProgress::Finished(result) = progress {
            return result;
        }
        let mut state = STATE.lock();
        let result = self.poll_active(dev, progress);
        match result {
            LutDsbPoll::Pending => return result,
            LutDsbPoll::Complete | LutDsbPoll::Failed(_) => state.phase = Phase::Idle,
            LutDsbPoll::Uncertain(_) => state.phase = Phase::Quarantined,
        }
        self.progress.set(UploadProgress::Finished(result));
        result
    }

    fn poll_active(&self, dev: Dev, progress: UploadProgress) -> LutDsbPoll {
        if dev.mmio != self.device.mmio
            || dev.mmio_len != self.device.mmio_len
            || (dev.bus, dev.slot, dev.function)
                != (self.device.bus, self.device.slot, self.device.function)
            || !pipe_a_active(dev)
        {
            return LutDsbPoll::Uncertain("chroma-dsb-engine-ownership-lost");
        }
        let saw_busy = match progress {
            UploadProgress::Running { saw_busy } => saw_busy,
            UploadProgress::Resetting { interrupt } => return Self::poll_reset(dev, interrupt),
            UploadProgress::Finished(result) => return result,
        };
        if mmio_read(dev, DSB_HEAD) != GGTT_BASE as u32 || mmio_read(dev, DSB_TAIL) != self.tail {
            return LutDsbPoll::Uncertain("chroma-dsb-engine-ownership-lost");
        }

        // Read BUSY after the pointer. A prefetched tail can precede the last
        // instruction completing, and an idle read before it can precede start.
        let head = mmio_read(dev, DSB_CURRENT_HEAD);
        let control = mmio_read(dev, DSB_CTRL);
        let interrupt = mmio_read(dev, DSB_INTERRUPT);
        if head == u32::MAX || control & !DSB_BUSY != DSB_ENABLE || interrupt & !DSB_IRQ_STATUS != 0
        {
            return LutDsbPoll::Uncertain("chroma-dsb-engine-state-lost");
        }
        if control & DSB_BUSY != 0 {
            self.progress
                .set(UploadProgress::Running { saw_busy: true });
            return LutDsbPoll::Pending;
        }
        if head != self.tail && interrupt != 0 {
            return LutDsbPoll::Uncertain(interrupt_error(interrupt));
        }
        if head != self.tail && !saw_busy {
            return LutDsbPoll::Pending;
        }

        // BUSY covers DMA start through the last instruction. For uploads
        // too fast to observe BUSY, require the fresh tail as completion proof.
        mmio_write(dev, DSB_CTRL, 0);
        self.progress.set(UploadProgress::Resetting { interrupt });
        Self::poll_reset(dev, interrupt)
    }

    fn poll_reset(dev: Dev, interrupt: u32) -> LutDsbPoll {
        match mmio_read(dev, DSB_CTRL) {
            0 => (),
            DSB_BUSY => return LutDsbPoll::Pending,
            _ => return LutDsbPoll::Uncertain("chroma-dsb-engine-disable-failed"),
        }
        let pending = mmio_read(dev, DSB_INTERRUPT);
        if pending & !interrupt != 0 {
            return LutDsbPoll::Uncertain("chroma-dsb-engine-status-changed");
        }
        if interrupt == 0 {
            return LutDsbPoll::Complete;
        }
        if pending != 0 {
            mmio_write(dev, DSB_INTERRUPT, pending);
            if mmio_read(dev, DSB_INTERRUPT) != 0 {
                return LutDsbPoll::Uncertain("chroma-dsb-interrupt-clear-failed");
            }
        }
        LutDsbPoll::Failed(interrupt_error(interrupt))
    }
}

impl Drop for LutDsbUpload {
    fn drop(&mut self) {
        if !matches!(self.progress.get(), UploadProgress::Finished(_)) {
            // An abandon or timeout cannot prove that DMA stopped. Keep both
            // the memory and the LUT port reserved until the next boot.
            STATE.lock().phase = Phase::Quarantined;
        }
    }
}
