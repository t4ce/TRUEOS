//! One-machine, native-resolution display bring-up; not a general modesetter.
//!
//! The Linux capture describes Linux, not the firmware state on a later boot.
//! Inspect the current Pipe A timing before writing anything. Keep its native
//! eDP timing/link, DBUF allocation and watermarks, and replace only the primary
//! surface, source geometry, and pipe/primary scaler bindings. This deliberately
//! hands the native mode to the ordinary UI4 plane bootstrap after the
//! primary surface has latched; no synthetic UI4 readiness is published.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Once;

use super::display::{
    PIPE_A_SRC, PIPE_FRMCOUNT_A, PIPE_SCALER_0_A_CTRL, PIPE_SCALER_1_A_CTRL,
    PIPE_SCALER_BINDING_MASK, PIPE_SCALER_ENABLE, PIPE_SCALER_WIN_POS_FROM_CTRL,
    PIPE_SCALER_WIN_SIZE_FROM_CTRL, PIPECONF_A, PIPECONF_STATE,
    PLANE_COLOR_PLANE_GAMMA_DISABLE, PLANE_CTL_ARB_SLOTS_4BPP, PLANE_CTL_ENABLE,
    PLANE_CTL_FORMAT_XRGB_8888, TRANS_DDI_FUNC_CTL_A, TRANS_HTOTAL_A, TRANS_VTOTAL_A,
    UNI_PLANE_AUX_DIST_OFF, UNI_PLANE_AUX_OFFSET_OFF, UNI_PLANE_BASE,
    UNI_PLANE_COLOR_CTL_OFF, UNI_PLANE_CUS_CTL_OFF, UNI_PLANE_KEYMSK_OFF,
    UNI_PLANE_OFFSET_OFF, UNI_PLANE_POS_OFF, UNI_PLANE_SIZE_OFF,
    UNI_PLANE_STRIDE_OFF, UNI_PLANE_SURF_OFF, UNI_PLANE_SURFLIVE_OFF,
};
use super::{Dev, mmio_read, mmio_write};

pub(crate) const ENABLED: bool = true;
// The panel-only experiment is over: normal Spirit startup may proceed.
// The PCI alias, GuC checks, real allocation and plane-latch checks remain.
pub(crate) const SPIRIT_RESEALED: bool = false;
const PHYSICAL_ID: u32 = 0x9A49_8086;
const PHYSICAL_REVISION: u8 = 0x01;
const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2160;
const PITCH_BYTES: u32 = WIDTH * 4;
const FRAME_BYTES: usize = PITCH_BYTES as usize * HEIGHT as usize;
// The desktop boot surface has only a 16 MiB slot. Do not enlarge its mapping
// into a neighboring owner's range. This CPU-authored, never-GPU-written
// probe uses a separate 64 MiB GGTT reservation and needs no EU edge guard.
pub(super) const SURFACE_GPU: u64 = 0xE000_0000;
pub(super) const SURFACE_GPU_CAPACITY: u64 = 0x0400_0000;
const GGTT_MMIO_END: usize = 0x0100_0000;
const POLL_ITERS: usize = 2_000_000;
const PIPE_SOURCE: u32 = ((WIDTH - 1) << 16) | (HEIGHT - 1);
const PLANE_SIZE: u32 = ((HEIGHT - 1) << 16) | (WIDTH - 1);
const PRIMARY_CTL: u32 =
    PLANE_CTL_ENABLE | PLANE_CTL_ARB_SLOTS_4BPP | PLANE_CTL_FORMAT_XRGB_8888;

const _: () = {
    assert!(PITCH_BYTES % 64 == 0);
    assert!(FRAME_BYTES == 33_177_600);
    assert!(FRAME_BYTES as u64 <= SURFACE_GPU_CAPACITY);
    assert!(SURFACE_GPU % 4096 == 0);
    assert!(SURFACE_GPU + SURFACE_GPU_CAPACITY <= 0x1_0000_0000);
    assert!(PIPE_SOURCE == 0x0EFF_086F);
    assert!(PLANE_SIZE == 0x086F_0EFF);
    // Mirror the existing display.rs direct-scanout reservation formula.
    assert!(
        super::DISPLAY_DIRECT_SCANOUT_GGTT_BASE
            + crate::ui4::INTERACTION_OVERLAY_PLANE_SLOT as u64
                * crate::ui4::OUTPUT_COUNT as u64
                * crate::ui4::FrameBuffering::Quad.count() as u64
                * 0x0200_0000
            <= SURFACE_GPU
    );
};

#[derive(Copy, Clone)]
struct Frame {
    phys: u64,
    virt: usize,
}

// Retain backing even if SURFLIVE never acknowledges a published address.
// A timed-out display request is not proof that the hardware cannot fetch it.
static RETAINED_FRAME: Once<Frame> = Once::new();
static ATTEMPTED: AtomicBool = AtomicBool::new(false);
static SCANOUT_LATCHED: AtomicBool = AtomicBool::new(false);

pub(crate) fn is_target(dev: Dev) -> bool {
    ENABLED
        && super::claimed_device().is_some_and(|claimed| {
            claimed.bus == dev.bus
                && claimed.slot == dev.slot
                && claimed.function == dev.function
        })
        // The ordinary u8/u16 reads are intentionally aliased by #44.
        // Raw config dwords still report the physical laptop identity.
        && crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 0) == PHYSICAL_ID
        && crate::pci::config_read_u32(dev.bus, dev.slot, dev.function, 8) as u8
            == PHYSICAL_REVISION
}

/// Stable layout selection, published only after this native surface latches.
pub(super) fn native_scanout_ready() -> bool {
    SCANOUT_LATCHED.load(Ordering::Acquire)
}

pub(crate) fn spirit_resealed() -> bool {
    SPIRIT_RESEALED && super::claimed_device().is_some_and(is_target)
}

/// Select the native surface reservations only for the successfully adopted
/// physical panel. This is not a GuC, RCS, or UI4 execution-success flag.
pub(crate) fn ui4_handoff_active() -> bool {
    SCANOUT_LATCHED.load(Ordering::Acquire)
        && super::claimed_device().is_some_and(is_target)
}

fn wait_frame(dev: Dev) -> bool {
    let before = mmio_read(dev, PIPE_FRMCOUNT_A);
    for _ in 0..POLL_ITERS {
        if mmio_read(dev, PIPE_FRMCOUNT_A) != before {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn detach_primary_scalers(dev: Dev) {
    for ctrl in [PIPE_SCALER_0_A_CTRL, PIPE_SCALER_1_A_CTRL] {
        let value = mmio_read(dev, ctrl);
        let binding = (value & PIPE_SCALER_BINDING_MASK) >> 25;
        // Binding 0 is the pipe scaler; 1 is hardware primary plane 1.
        // Do not steal a scaler bound to a different plane.
        if value & PIPE_SCALER_ENABLE != 0 && binding <= 1 {
            mmio_write(dev, ctrl, 0);
            mmio_write(dev, ctrl + PIPE_SCALER_WIN_POS_FROM_CTRL, 0);
            // WIN_SZ is the scaler arming write, so it must be last.
            mmio_write(dev, ctrl + PIPE_SCALER_WIN_SIZE_FROM_CTRL, 0);
        }
    }
}

fn primary_scalers_detached(dev: Dev) -> bool {
    [PIPE_SCALER_0_A_CTRL, PIPE_SCALER_1_A_CTRL]
        .into_iter()
        .all(|ctrl| {
            let value = mmio_read(dev, ctrl);
            let binding = (value & PIPE_SCALER_BINDING_MASK) >> 25;
            value & PIPE_SCALER_ENABLE == 0 || binding > 1
        })
}

fn prepare_frame(dev: Dev) -> Option<Frame> {
    let (phys, virt) = super::alloc_ggtt_backing(FRAME_BYTES, super::WARM_ALIGN)?;
    let pixels = unsafe { core::slice::from_raw_parts_mut(virt.cast::<u32>(), FRAME_BYTES / 4) };
    for y in 0..HEIGHT as usize {
        for x in 0..WIDTH as usize {
            // XRGB8888 little-endian, matching PRIMARY_CTL (not RGBA byte order).
            // Outer border proves all four native edges; the inner cyan box
            // marks the old 2560x1440 extent. No font/GPU/USB/NIC is required.
            let border = x < 16 || y < 16 || x >= WIDTH as usize - 16 || y >= HEIGHT as usize - 16;
            let old_extent = (x < 2560 && (1436..1440).contains(&y))
                || (y < 1440 && (2556..2560).contains(&x));
            pixels[y * WIDTH as usize + x] = if border {
                0x00FF_FFFF
            } else if old_extent {
                0x0000_FFFF
            } else {
                match (x >= WIDTH as usize / 2, y >= HEIGHT as usize / 2) {
                    (false, false) => 0x0030_1020,
                    (true, false) => 0x0010_3020,
                    (false, true) => 0x0010_2030,
                    (true, true) => 0x0030_3020,
                }
            };
        }
    }
    super::dma_flush(virt, FRAME_BYTES);
    if !super::map_display_scanout_ggtt(dev, phys, FRAME_BYTES, SURFACE_GPU) {
        let _ = super::unmap_display_scanout_ggtt(dev, FRAME_BYTES, SURFACE_GPU);
        crate::dma::dealloc(virt, FRAME_BYTES);
        return None;
    }
    super::ggtt_invalidate(dev);
    Some(Frame { phys, virt: virt as usize })
}

/// Native Pipe A setup followed by the ordinary UI4 bootstrap on success.
/// A failed native setup retains its surface and does not run the desktop path.
/// No new link timing is synthesized: unsupported firmware handoff is left
/// alone. Software polling is bounded; a bus-level stuck MMIO access cannot
/// be made recoverable by a Rust loop bound.
pub(crate) fn init_once(dev: Dev) -> bool {
    if !is_target(dev) {
        return false;
    }
    if ATTEMPTED.swap(true, Ordering::AcqRel) {
        return SCANOUT_LATCHED.load(Ordering::Acquire);
    }
    if dev.mmio.is_null() || dev.mmio_len < GGTT_MMIO_END {
        crate::log!("intel/tgl-native-panel: skipped reason=mmio-window-too-small\n");
        return false;
    }
    let pipeconf = mmio_read(dev, PIPECONF_A);
    let htotal = mmio_read(dev, TRANS_HTOTAL_A);
    let vtotal = mmio_read(dev, TRANS_VTOTAL_A);
    let ddi = mmio_read(dev, TRANS_DDI_FUNC_CTL_A);
    let source_before = mmio_read(dev, PIPE_A_SRC);
    // These are actual boot-time registers, not the earlier Ubuntu snapshot.
    if pipeconf & PIPECONF_STATE == 0
        || ddi & (1 << 31) == 0
        || (htotal & 0xFFFF) + 1 != WIDTH
        || (vtotal & 0xFFFF) + 1 != HEIGHT
    {
        crate::log!(
            "intel/tgl-native-panel: skipped reason=firmware-not-native-active-pipe-a pipeconf=0x{:08X} ddi=0x{:08X} htotal=0x{:08X} vtotal=0x{:08X} source=0x{:08X} link_reprogrammed=0\n",
            pipeconf, ddi, htotal, vtotal, source_before,
        );
        return false;
    }
    let Some(frame) = prepare_frame(dev) else {
        crate::log!("intel/tgl-native-panel: skipped reason=frame-allocation-or-ggtt-map\n");
        return false;
    };
    // From here retain pages even on failure; do not race an uncertain latch.
    RETAINED_FRAME.call_once(|| frame);
    let ctl_before = mmio_read(dev, UNI_PLANE_BASE);
    let surf_before = mmio_read(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF);
    if ctl_before & PLANE_CTL_ENABLE != 0 {
        mmio_write(dev, UNI_PLANE_BASE, ctl_before & !PLANE_CTL_ENABLE);
        mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF, surf_before);
        let _ = mmio_read(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF);
        if !wait_frame(dev) || !wait_frame(dev) {
            // Native source/scaler changes have not been written. Re-arm the
            // untouched old plane, retain the new allocation, and stop.
            mmio_write(dev, UNI_PLANE_BASE, ctl_before);
            mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF, surf_before);
            crate::log!("intel/tgl-native-panel: stopped reason=primary-disable-frame-timeout old-plane-rearmed=1 backing=retained\n");
            return false;
        }
    }
    detach_primary_scalers(dev);
    mmio_write(dev, PIPE_A_SRC, PIPE_SOURCE);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_STRIDE_OFF, PITCH_BYTES / 64);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_POS_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_SIZE_OFF, PLANE_SIZE);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_OFFSET_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_AUX_DIST_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_AUX_OFFSET_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_CUS_CTL_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_KEYMSK_OFF, 0);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_COLOR_CTL_OFF, PLANE_COLOR_PLANE_GAMMA_DISABLE);
    mmio_write(dev, UNI_PLANE_BASE, PRIMARY_CTL);
    core::sync::atomic::fence(Ordering::SeqCst);
    mmio_write(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF, SURFACE_GPU as u32);
    let _ = mmio_read(dev, UNI_PLANE_BASE + UNI_PLANE_SURF_OFF);

    let mut live = 0;
    let mut latched = false;
    for _ in 0..POLL_ITERS {
        live = mmio_read(dev, UNI_PLANE_BASE + UNI_PLANE_SURFLIVE_OFF);
        if live & 0xFFFF_F000 == SURFACE_GPU as u32 {
            latched = true;
            break;
        }
        core::hint::spin_loop();
    }
    let source_after = mmio_read(dev, PIPE_A_SRC);
    let scalers_detached = primary_scalers_detached(dev);
    let readback_ok = latched
        && source_after == PIPE_SOURCE
        && mmio_read(dev, UNI_PLANE_BASE + UNI_PLANE_SIZE_OFF) == PLANE_SIZE
        && scalers_detached;
    SCANOUT_LATCHED.store(readback_ok, Ordering::Release);
    crate::log!(
        "intel/tgl-native-panel: source={}x{} source_before=0x{:08X} source_after=0x{:08X} pitch={} bytes={} gpu=0x{:X} phys=0x{:X} virt=0x{:X} surflive=0x{:08X} latched={} scaler_detached={} link_timing=preserved dbuf=preserved watermarks=preserved ui4_stack_ready=0 spirit_resealed={} proof=cpu-native-border-and-quadrants backing=retained\n",
        WIDTH, HEIGHT, source_before, source_after, PITCH_BYTES, FRAME_BYTES,
        SURFACE_GPU, frame.phys, frame.virt, live, readback_ok as u8,
        scalers_detached as u8, SPIRIT_RESEALED as u8,
    );
    if readback_ok {
        // PIPE_SRC now describes the actual native panel. Re-enter the same
        // allocation, alpha/DBUF, plane-latch and capability-publication path
        // as desktop UI4, with distinct 4K-safe scanout/compositor reservations.
        // Do not retrain eDP or restore the old firmware scaler geometry.
        super::display::log_bsp_display_metrics_probe(dev);
        super::display::init_primary_boot_surface(dev);
        crate::log!(
            "intel/tgl-native-panel: ui4-handoff attempted=1 stack_ready={} spirit_resealed=0 native={}x{} link_timing=preserved\n",
            super::display::ui4_rgba8_plane_stack_is_ready() as u8,
            WIDTH,
            HEIGHT,
        );
    }
    readback_ok
}
