//! Per-pipe 3D-LUT transport for CHROMATIC (Tiger Lake display PRM pp245–246,
//! register PRM vol2c pp1413–1416). This owns no gamma/CSC register state.
//!
//! The LUT is post-blend and output-wide. DC5/DC6 lose LUT RAM; keep the desired
//! table in the service so display-resume integration can submit it again.

use alloc::{boxed::Box, vec};
use spin::Mutex;

#[path = "chroma_grade.rs"]
mod grade;
pub(super) use grade::{LUT_EDGE, LUT_ENTRIES, Settings, identity_code, unpack_node};

#[derive(Clone, Copy)]
struct Registers {
    control: usize,
    index: usize,
    data: usize,
    config: usize,
}

/// Only A and B have this block. Do not extrapolate register addresses for
/// C/D: those pipes may still have software editing buckets, never LUT MMIO.
pub(super) const fn supports_pipe_lut(pipe: usize) -> bool {
    matches!(pipe, 0 | 1)
}

fn registers(pipe: usize) -> Result<Registers, &'static str> {
    match pipe {
        0 => Ok(Registers {
            control: 0x490A4,
            index: 0x490A8,
            data: 0x490AC,
            config: 0x70008,
        }),
        1 => Ok(Registers {
            control: 0x491A4,
            index: 0x491A8,
            data: 0x491AC,
            config: 0x71008,
        }),
        _ => Err("chroma-3dlut-pipe-unsupported"),
    }
}
const LUT_ENABLE: u32 = 1 << 31;
const LUT_NEW_READY: u32 = 1 << 30;
const LUT_ALLOW_DB_DISABLE: u32 = 1 << 29;
const INDEX_AUTO_INCREMENT: u32 = 1 << 13;
const CHICKEN_MISC: usize = 0x420B0;
const LUT_FIRST_FRAME_ENABLE: u32 = 1 << 10;
const PIPE_ENABLE: u32 = 1 << 31;

static PIPE_LUT: Mutex<()> = Mutex::new(());

pub(super) struct PipeLutSnapshot {
    pipe: usize,
    control: u32,
    entries: Box<[u32]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PipeLutStatus {
    Disabled,
    Pending,
    Active,
}

fn device(regs: Registers) -> Result<crate::intel::Dev, &'static str> {
    let dev = crate::intel::claimed_device().ok_or("chroma-intel-unavailable")?;
    // Check raw PCI identity: the laptop's compatibility experiment aliases
    // ordinary reads/Dev.device_id to ADL-S. TGL/ADL/RPL have the same Gen12+
    // Pipe A/B 17^3 LUT port (Intel platform datasheets; intel_color_regs.h).
    let physical = crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0);
    if !matches!(
        physical,
        0x9A49_8086 | 0x4680_8086 | 0x46D1_8086 | 0xA780_8086
    ) {
        return Err("chroma-3dlut-device-unsupported");
    }
    if dev.mmio_len < regs.config + 4
        || crate::intel::mmio_read(dev, regs.config) & PIPE_ENABLE == 0
    {
        return Err("chroma-pipe-unavailable");
    }
    Ok(dev)
}

fn ready(dev: crate::intel::Dev, regs: Registers) -> Result<u32, &'static str> {
    let control = crate::intel::mmio_read(dev, regs.control);
    if control == u32::MAX {
        return Err("chroma-3dlut-unavailable");
    }
    if control & LUT_NEW_READY != 0 {
        return Err("chroma-3dlut-busy");
    }
    Ok(control)
}

/// Cheap capability/commit check. A pending vblank update is a valid state,
/// including when the editor is reopened immediately after an earlier edit.
pub(super) fn pipe_status(pipe: usize) -> Result<PipeLutStatus, &'static str> {
    let regs = registers(pipe)?;
    let _guard = PIPE_LUT.lock();
    let dev = device(regs)?;
    let control = crate::intel::mmio_read(dev, regs.control);
    if control == u32::MAX {
        return Err("chroma-3dlut-unavailable");
    }
    Ok(if control & LUT_NEW_READY != 0 {
        PipeLutStatus::Pending
    } else if control & LUT_ENABLE != 0 {
        PipeLutStatus::Active
    } else {
        PipeLutStatus::Disabled
    })
}

/// Re-arm a service-owned grade after an observed pipe/LUT reset. Intended for
/// reopen or display-resume boundaries, not the animation loop. Only five
/// nonzero identity sentinels are read and the indirect index is restored.
///
/// This detects enable loss and backing-table loss; it cannot prove that the
/// internal working RAM survived DC5/DC6 while a retained backing table did.
/// An actual display-resume handler must unconditionally call program_pipe.
pub(super) fn rearm_pipe_if_lost(
    pipe: usize,
    entries: &[u32; LUT_ENTRIES],
) -> Result<bool, &'static str> {
    let regs = registers(pipe)?;
    let _guard = PIPE_LUT.lock();
    let dev = device(regs)?;
    let control = ready(dev, regs)?;
    let mut lost = control & LUT_ENABLE == 0;
    if !lost {
        let old_index = crate::intel::mmio_read(dev, regs.index);
        for index in [
            1,
            LUT_EDGE,
            LUT_EDGE * LUT_EDGE,
            LUT_ENTRIES / 2,
            LUT_ENTRIES - 1,
        ] {
            crate::intel::mmio_write(dev, regs.index, index as u32);
            if crate::intel::mmio_read(dev, regs.data) & 0x3fff_ffff != entries[index] {
                lost = true;
                break;
            }
        }
        crate::intel::mmio_write(dev, regs.index, old_index);
    }
    if lost {
        upload(dev, regs, entries, control | LUT_ENABLE)?;
    }
    Ok(lost)
}

/// Snapshot the pre-session state for rollback if initial frame creation fails.
/// The service keeps an accepted grade on ordinary Escape-close.
pub(super) fn snapshot_pipe(pipe: usize) -> Result<PipeLutSnapshot, &'static str> {
    let regs = registers(pipe)?;
    let mut entries = vec![0; LUT_ENTRIES].into_boxed_slice();
    let _guard = PIPE_LUT.lock();
    let dev = device(regs)?;
    let control = ready(dev, regs)?;
    let old_index = crate::intel::mmio_read(dev, regs.index);
    crate::intel::mmio_write(dev, regs.index, INDEX_AUTO_INCREMENT);
    for entry in entries.iter_mut() {
        *entry = crate::intel::mmio_read(dev, regs.data) & 0x3fff_ffff;
    }
    crate::intel::mmio_write(dev, regs.index, old_index);
    Ok(PipeLutSnapshot {
        pipe,
        control,
        entries,
    })
}

/// Upload a complete back-buffer table, then arm it for the next vblank.
/// A busy result is retryable on the next service tick; never spin on the E-core
/// waiting for a display event. Hardware alone clears New LUT Ready.
pub(super) fn program_pipe(pipe: usize, entries: &[u32; LUT_ENTRIES]) -> Result<(), &'static str> {
    let regs = registers(pipe)?;
    let _guard = PIPE_LUT.lock();
    let dev = device(regs)?;
    let control = ready(dev, regs)?;
    upload(dev, regs, entries, control | LUT_ENABLE)
}

fn upload(
    dev: crate::intel::Dev,
    regs: Registers,
    entries: &[u32],
    control: u32,
) -> Result<(), &'static str> {
    if entries.len() != LUT_ENTRIES || entries.iter().any(|value| value & 0xc000_0000 != 0) {
        return Err("chroma-3dlut-entry-range");
    }
    // TGL's PRM requires this workaround for the first pipe-on frame. Do not
    // assume a generation-specific chicken bit on the other admitted devices.
    if crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) == 0x9A49_8086 {
        let chicken = crate::intel::mmio_read(dev, CHICKEN_MISC);
        crate::intel::mmio_write(dev, CHICKEN_MISC, chicken | LUT_FIRST_FRAME_ENABLE);
    }
    let old_index = crate::intel::mmio_read(dev, regs.index);
    // Vol12's auto-increment prose depends on this explicit Vol2c bit.
    crate::intel::mmio_write(dev, regs.index, INDEX_AUTO_INCREMENT);
    for &entry in entries {
        crate::intel::mmio_write(dev, regs.data, entry);
    }
    crate::intel::mmio_write(dev, regs.index, old_index);
    let armed = (control & (LUT_ENABLE | LUT_ALLOW_DB_DISABLE)) | LUT_NEW_READY;
    crate::intel::mmio_write(dev, regs.control, armed);
    // Posting read; Ready may already have cleared if vblank raced this read.
    let readback = crate::intel::mmio_read(dev, regs.control);
    if readback & LUT_ENABLE != armed & LUT_ENABLE {
        return Err("chroma-3dlut-enable-readback");
    }
    Ok(())
}

pub(super) fn restore_pipe(snapshot: &PipeLutSnapshot) -> Result<(), &'static str> {
    let regs = registers(snapshot.pipe)?;
    let _guard = PIPE_LUT.lock();
    let dev = device(regs)?;
    ready(dev, regs)?;
    if snapshot.control & LUT_ENABLE == 0 {
        // Disabled LUTs do not consume New LUT Ready. Do not arm an update
        // that would stay permanently pending while the function is disabled.
        crate::intel::mmio_write(dev, regs.control, snapshot.control & LUT_ALLOW_DB_DISABLE);
        return Ok(());
    }
    upload(dev, regs, &snapshot.entries, snapshot.control)
}

// Compatibility entry points for callers that still exclusively own Pipe A.
pub(super) fn pipe_a_status() -> Result<PipeLutStatus, &'static str> {
    pipe_status(0)
}
pub(super) fn rearm_pipe_a_if_lost(entries: &[u32; LUT_ENTRIES]) -> Result<bool, &'static str> {
    rearm_pipe_if_lost(0, entries)
}
pub(super) fn program_pipe_a(entries: &[u32; LUT_ENTRIES]) -> Result<(), &'static str> {
    program_pipe(0, entries)
}
pub(super) fn snapshot_pipe_a() -> Result<PipeLutSnapshot, &'static str> {
    snapshot_pipe(0)
}
pub(super) fn restore_pipe_a(snapshot: &PipeLutSnapshot) -> Result<(), &'static str> {
    if snapshot.pipe != 0 {
        return Err("chroma-3dlut-snapshot-pipe");
    }
    restore_pipe(snapshot)
}
