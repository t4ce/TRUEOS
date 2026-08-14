use core::sync::atomic::{AtomicU64, Ordering};

use crate::graphics::primitives::{Error, Result, UiRect, UiSurface, UiSurfaceFormat};
use spin::Mutex;

// Keep a generous table of trusted physical-buffer handles. This is separate
// from the logical window/frame limits: a streaming frame consumes three
// handles, and physical memory plus producer GPU VA remain bounded below.
const MAX_UI_SURFACES: usize = 64;
pub(crate) const UI_SURFACE_GPU_BASE: u64 = 0x1200_0000;
// Producer surfaces are mapped into the direct-RCS PPGTT on demand. Keep this
// arena below render's persistent-font range at 0x2000_0000 and well inside
// the direct-RCS 1 GiB PPGTT. Packing actual aligned allocation sizes avoids
// the former 32 MiB-per-handle VA waste and makes room for normal UI4 growth.
pub(crate) const UI_SURFACE_GPU_LIMIT: u64 = 0x2000_0000;
const UI_SURFACE_MAX_BYTES: u64 = 0x0200_0000;
const UI_SURFACE_MAX_PHYS_EXCLUSIVE: u64 = 1u64 << 39;
const UI_SURFACE_BYTES_PER_PIXEL: u32 = 4;

const _: () = {
    assert!(UI_SURFACE_GPU_BASE % 4096 == 0);
    assert!(UI_SURFACE_GPU_LIMIT % 4096 == 0);
    assert!(UI_SURFACE_MAX_BYTES % 4096 == 0);
    assert!(UI_SURFACE_GPU_BASE < UI_SURFACE_GPU_LIMIT);
    assert!(UI_SURFACE_MAX_BYTES <= UI_SURFACE_GPU_LIMIT - UI_SURFACE_GPU_BASE);
    assert!(UI_SURFACE_GPU_LIMIT <= crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES);
    assert!(UI_SURFACE_MAX_PHYS_EXCLUSIVE <= 1u64 << 46);
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct UiSurfaceHandle(u32);

impl UiSurfaceHandle {
    #[inline]
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[inline]
    const fn from_slot(slot: usize) -> Self {
        Self((slot as u32) + 1)
    }

    #[inline]
    const fn slot(self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            Some((self.0 - 1) as usize)
        }
    }
}

#[derive(Clone, Copy)]
struct TrustedUiSurface {
    desc: UiSurface,
    phys: u64,
    virt: *mut u8,
    byte_len: usize,
}

unsafe impl Send for TrustedUiSurface {}
unsafe impl Sync for TrustedUiSurface {}

#[derive(Clone, Copy)]
pub(crate) struct UiSurfaceRgbaAccess {
    pub phys: u64,
    pub gpu: u64,
    pub virt: *mut u8,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

unsafe impl Send for UiSurfaceRgbaAccess {}
unsafe impl Sync for UiSurfaceRgbaAccess {}

#[derive(Clone, Copy)]
#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) struct UiSurfacePixelAccess {
    pub virt: *mut u8,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: UiSurfaceFormat,
}

unsafe impl Send for UiSurfacePixelAccess {}
unsafe impl Sync for UiSurfacePixelAccess {}

static SURFACES: Mutex<[Option<TrustedUiSurface>; MAX_UI_SURFACES]> =
    Mutex::new([None; MAX_UI_SURFACES]);
static SURFACE_TOO_LARGE_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static HANDLE_CAPACITY_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static PRODUCER_VA_CAPACITY_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static DMA_CAPACITY_REJECTIONS: AtomicU64 = AtomicU64::new(0);
static HIGH_PHYSICAL_ADMISSIONS: AtomicU64 = AtomicU64::new(0);

pub fn create_surface(width: u32, height: u32, format: UiSurfaceFormat) -> Result<UiSurfaceHandle> {
    create_surface_with_initialization(width, height, format, true)
}

/// Allocate a producer-only surface without touching its pixels on the CPU.
///
/// The caller must keep the allocation unpublished until a trusted GPU
/// producer has overwritten the complete surface and supplied its exact
/// release fence. `ui4::frame_pool` records and enforces that requirement for
/// every frame created through its GPU-full-overwrite constructor.
pub(crate) fn create_gpu_full_overwrite_surface(
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
) -> Result<UiSurfaceHandle> {
    create_surface_with_initialization(width, height, format, false)
}

fn create_surface_with_initialization(
    width: u32,
    height: u32,
    format: UiSurfaceFormat,
    initialize_cpu: bool,
) -> Result<UiSurfaceHandle> {
    if width == 0 || height == 0 {
        return Err(Error::Invalid);
    }
    let pitch = aligned_pitch_bytes(width)?;
    let raw_len = (pitch as usize)
        .checked_mul(height as usize)
        .ok_or(Error::Invalid)?;
    let byte_len =
        crate::intel::align_up(raw_len, crate::intel::WARM_ALIGN).ok_or(Error::Invalid)?;
    if byte_len as u64 > UI_SURFACE_MAX_BYTES {
        let active = SURFACES.lock().iter().flatten().count();
        log_allocation_rejected("surface-too-large", active, width, height, pitch, byte_len);
        return Err(Error::OutOfMemory);
    }

    let mut surfaces = SURFACES.lock();
    let active = surfaces.iter().flatten().count();
    let Some(slot) = surfaces.iter().position(Option::is_none) else {
        drop(surfaces);
        log_allocation_rejected("handle-capacity", active, width, height, pitch, byte_len);
        return Err(Error::OutOfMemory);
    };
    let Some(gpu) = allocate_surface_gpu_va(&surfaces[..], byte_len) else {
        drop(surfaces);
        log_allocation_rejected("producer-va-capacity", active, width, height, pitch, byte_len);
        return Err(Error::OutOfMemory);
    };
    let Some((phys, virt)) = allocate_surface_backing(byte_len) else {
        drop(surfaces);
        log_allocation_rejected("dma-capacity", active, width, height, pitch, byte_len);
        return Err(Error::OutOfMemory);
    };
    if initialize_cpu {
        unsafe {
            core::ptr::write_bytes(virt, 0, byte_len);
        }
        crate::intel::dma_flush(virt, byte_len);
    }

    // This is a producer address, not a display-plane slot. Render/compute
    // submission maps the DMA allocation into its PPGTT before use. A future
    // UI4 presenter must import the selected front buffer into a display-owned
    // GGTT slot only while that plane owns it; eagerly mapping all offscreen
    // surfaces here would overwrite current scanout reservations.

    surfaces[slot] = Some(TrustedUiSurface {
        desc: UiSurface {
            gpu,
            width,
            height,
            pitch,
            format,
        },
        phys,
        virt,
        byte_len,
    });
    Ok(UiSurfaceHandle::from_slot(slot))
}

fn allocate_surface_backing(byte_len: usize) -> Option<(u64, *mut u8)> {
    // UI4 frame pixels are not consumed by a legacy 32-bit DMA device. The
    // producer imports them through a private Gen12 PPGTT and presentation
    // imports the selected front buffer through the Gen12 display GGTT. Both
    // page-table formats carry physical addresses above 4 GiB. Prefer the low
    // DMA arena while it has space, but preserve UI admission by falling back
    // to ordinary system memory within XeLP's conservative 39-bit range.
    crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN).or_else(|| {
        let allocation = crate::dma::alloc_with_max(
            byte_len,
            crate::intel::WARM_ALIGN,
            Some(UI_SURFACE_MAX_PHYS_EXCLUSIVE),
        )?;
        let occurrence = HIGH_PHYSICAL_ADMISSIONS
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1);
        if occurrence <= 8 || occurrence.is_power_of_two() {
            crate::log_info!(
                target: "ui4";
                "ui4 trusted-surface backing admitted from high physical memory occurrence={} phys=0x{:X} bytes=0x{:X} reason=sub4g-dma-capacity ownership=producer-ppgtt+display-ggtt\n",
                occurrence,
                allocation.0,
                byte_len,
            );
        }
        Some(allocation)
    })
}

fn allocate_surface_gpu_va(surfaces: &[Option<TrustedUiSurface>], byte_len: usize) -> Option<u64> {
    let byte_len = u64::try_from(byte_len).ok()?;
    if byte_len == 0 || byte_len > UI_SURFACE_MAX_BYTES {
        return None;
    }

    let mut candidate = UI_SURFACE_GPU_BASE;
    loop {
        let candidate_end = candidate.checked_add(byte_len)?;
        if candidate_end > UI_SURFACE_GPU_LIMIT {
            return None;
        }

        let mut next_candidate = candidate;
        for surface in surfaces.iter().flatten() {
            let surface_start = surface.desc.gpu;
            let surface_end = surface_start.checked_add(surface.byte_len as u64)?;
            if candidate < surface_end && surface_start < candidate_end {
                next_candidate = next_candidate.max(surface_end);
            }
        }
        if next_candidate == candidate {
            return Some(candidate);
        }
        candidate = next_candidate;
    }
}

fn log_allocation_rejected(
    reason: &'static str,
    active: usize,
    width: u32,
    height: u32,
    pitch: u32,
    byte_len: usize,
) {
    let counter = match reason {
        "surface-too-large" => &SURFACE_TOO_LARGE_REJECTIONS,
        "handle-capacity" => &HANDLE_CAPACITY_REJECTIONS,
        "producer-va-capacity" => &PRODUCER_VA_CAPACITY_REJECTIONS,
        _ => &DMA_CAPACITY_REJECTIONS,
    };
    let occurrences = counter.fetch_add(1, Ordering::Relaxed).saturating_add(1);
    if occurrences <= 8 {
        crate::log_warn!(
            target: "ui4";
            "ui4 trusted-surface allocation rejected reason={} occurrences={} active={} requested={} handle_max={} request={}x{} pitch=0x{:X} bytes=0x{:X} max_surface_bytes=0x{:X} producer_va=0x{:X}..0x{:X} action=reject-create\n",
            reason,
            occurrences,
            active,
            active.saturating_add(1),
            MAX_UI_SURFACES,
            width,
            height,
            pitch,
            byte_len,
            UI_SURFACE_MAX_BYTES,
            UI_SURFACE_GPU_BASE,
            UI_SURFACE_GPU_LIMIT,
        );
    } else if occurrences >= 64 && occurrences.is_power_of_two() {
        crate::log_trace!(
            target: "ui4";
            "ui4 trusted-surface allocation still rejected reason={} occurrences={} active={} requested={} handle_max={} request={}x{} pitch=0x{:X} bytes=0x{:X} action=reject-create\n",
            reason,
            occurrences,
            active,
            active.saturating_add(1),
            MAX_UI_SURFACES,
            width,
            height,
            pitch,
            byte_len,
        );
    }
}

pub fn destroy_surface(handle: UiSurfaceHandle) -> bool {
    let Some(slot) = handle.slot() else {
        return false;
    };
    let mut surfaces = SURFACES.lock();
    let Some(surface) = surfaces.get_mut(slot).and_then(Option::take) else {
        return false;
    };
    crate::dma::dealloc(surface.virt, surface.byte_len);
    true
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn surface(handle: UiSurfaceHandle) -> Option<UiSurface> {
    lookup(handle).map(|surface| surface.desc)
}

pub(crate) fn rgba_access(handle: UiSurfaceHandle) -> Option<UiSurfaceRgbaAccess> {
    let surface = lookup(handle)?;
    if surface.desc.format != UiSurfaceFormat::Rgba8888 {
        return None;
    }
    Some(UiSurfaceRgbaAccess {
        phys: surface.phys,
        gpu: surface.desc.gpu,
        virt: surface.virt,
        byte_len: surface.byte_len,
        width: surface.desc.width,
        height: surface.desc.height,
        pitch: surface.desc.pitch,
    })
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn pixel_access(handle: UiSurfaceHandle) -> Option<UiSurfacePixelAccess> {
    let surface = lookup(handle)?;
    Some(UiSurfacePixelAccess {
        virt: surface.virt,
        byte_len: surface.byte_len,
        width: surface.desc.width,
        height: surface.desc.height,
        pitch: surface.desc.pitch,
        format: surface.desc.format,
    })
}

pub(crate) fn gpgpu_rgba_surface(
    handle: UiSurfaceHandle,
) -> Option<crate::intel::gpgpu::GpgpuRgba8Surface> {
    let surface = lookup(handle)?;
    if surface.desc.format != UiSurfaceFormat::Rgba8888 {
        return None;
    }
    crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        surface.desc.gpu,
        surface.byte_len,
        surface.desc.width,
        surface.desc.height,
        surface.desc.pitch,
    )
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn flush_surface(handle: UiSurfaceHandle) -> bool {
    let Some(surface) = lookup(handle) else {
        return false;
    };
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    true
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn write_surface_rgba(
    handle: UiSurfaceHandle,
    dst: UiRect,
    src_rgba: &[u8],
    src_pitch: usize,
) -> Result<()> {
    let surface = lookup(handle).ok_or(Error::NotFound)?;
    let dst = clip_rect_to_surface(dst, surface.desc).ok_or(Error::Invalid)?;
    let row_bytes = (dst.w as usize)
        .checked_mul(UI_SURFACE_BYTES_PER_PIXEL as usize)
        .ok_or(Error::Invalid)?;
    if src_pitch < row_bytes || src_rgba.len() < src_pitch.saturating_mul(dst.h as usize) {
        return Err(Error::Invalid);
    }

    match surface.desc.format {
        UiSurfaceFormat::Rgba8888 => {
            for row in 0..dst.h as usize {
                let src_off = row.saturating_mul(src_pitch);
                let dst_off = ((dst.y as usize + row).saturating_mul(surface.desc.pitch as usize))
                    .saturating_add((dst.x as usize).saturating_mul(4));
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        src_rgba.as_ptr().add(src_off),
                        surface.virt.add(dst_off),
                        row_bytes,
                    );
                }
            }
        }
        UiSurfaceFormat::Xrgb8888 | UiSurfaceFormat::Xbgr8888 => {
            write_rgba_to_xrgb_surface(surface, dst, src_rgba, src_pitch);
        }
    }

    flush_surface_rect(surface, dst);
    Ok(())
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn clear_surface_rgb(handle: UiSurfaceHandle, rgb: u32) -> Result<()> {
    let surface = lookup(handle).ok_or(Error::NotFound)?;
    let rect = UiRect::new(0, 0, surface.desc.width, surface.desc.height);
    if rect.is_empty() {
        return Err(Error::Invalid);
    }

    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    let dst_pitch_pixels = surface.desc.pitch as usize / 4;
    let dst_words = surface.virt as *mut u32;
    for y in 0..surface.desc.height as usize {
        let row = unsafe { dst_words.add(y.saturating_mul(dst_pitch_pixels)) };
        for x in 0..surface.desc.width as usize {
            let pixel = match surface.desc.format {
                UiSurfaceFormat::Rgba8888 => r | (g << 8) | (b << 16) | (0xFF << 24),
                UiSurfaceFormat::Xbgr8888 => (b << 16) | (g << 8) | r,
                UiSurfaceFormat::Xrgb8888 => (r << 16) | (g << 8) | b,
            };
            unsafe {
                core::ptr::write_volatile(row.add(x), pixel);
            }
        }
    }
    flush_surface_rect(surface, rect);
    Ok(())
}

fn lookup(handle: UiSurfaceHandle) -> Option<TrustedUiSurface> {
    let slot = handle.slot()?;
    SURFACES.lock().get(slot).copied().flatten()
}

fn aligned_pitch_bytes(width: u32) -> Result<u32> {
    let bytes = width
        .checked_mul(UI_SURFACE_BYTES_PER_PIXEL)
        .ok_or(Error::Invalid)?;
    crate::intel::align_up(bytes as usize, 64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(Error::Invalid)
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn clip_rect_to_surface(rect: UiRect, surface: UiSurface) -> Option<UiRect> {
    if rect.is_empty() || rect.x >= surface.width || rect.y >= surface.height {
        return None;
    }
    let w = rect.w.min(surface.width.saturating_sub(rect.x));
    let h = rect.h.min(surface.height.saturating_sub(rect.y));
    if w == 0 || h == 0 {
        None
    } else {
        Some(UiRect { w, h, ..rect })
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn write_rgba_to_xrgb_surface(
    surface: TrustedUiSurface,
    dst: UiRect,
    src_rgba: &[u8],
    src_pitch: usize,
) {
    let dst_pitch_pixels = surface.desc.pitch as usize / 4;
    let dst_words = surface.virt as *mut u32;
    for y in 0..dst.h as usize {
        let src_off = y.saturating_mul(src_pitch);
        let dst_row = unsafe {
            dst_words.add((dst.y as usize + y).saturating_mul(dst_pitch_pixels) + dst.x as usize)
        };
        for x in 0..dst.w as usize {
            let p = src_off + x.saturating_mul(4);
            let r = src_rgba[p] as u32;
            let g = src_rgba[p + 1] as u32;
            let b = src_rgba[p + 2] as u32;
            let pixel = match surface.desc.format {
                UiSurfaceFormat::Xbgr8888 => (b << 16) | (g << 8) | r,
                _ => (r << 16) | (g << 8) | b,
            };
            unsafe {
                core::ptr::write_volatile(dst_row.add(x), pixel);
            }
        }
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
fn flush_surface_rect(surface: TrustedUiSurface, rect: UiRect) {
    let start = (rect.y as usize)
        .saturating_mul(surface.desc.pitch as usize)
        .saturating_add((rect.x as usize).saturating_mul(4));
    let bytes = (rect.h as usize)
        .saturating_sub(1)
        .saturating_mul(surface.desc.pitch as usize)
        .saturating_add((rect.w as usize).saturating_mul(4));
    if start < surface.byte_len {
        let bytes = bytes.min(surface.byte_len.saturating_sub(start));
        crate::intel::dma_flush(unsafe { surface.virt.add(start) }, bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MAX_UI_SURFACES, TrustedUiSurface, UI_SURFACE_GPU_BASE, UI_SURFACE_GPU_LIMIT,
        allocate_surface_gpu_va,
    };
    use crate::graphics::primitives::{UiSurface, UiSurfaceFormat};

    const TEST_BYTES: usize = 0x20_0000;

    fn test_surface(gpu: u64, byte_len: usize) -> TrustedUiSurface {
        TrustedUiSurface {
            desc: UiSurface {
                gpu,
                width: 1,
                height: 1,
                pitch: 64,
                format: UiSurfaceFormat::Rgba8888,
            },
            phys: 0,
            virt: core::ptr::null_mut(),
            byte_len,
        }
    }

    #[test]
    fn producer_va_packs_real_allocation_sizes() {
        let mut surfaces = [None; MAX_UI_SURFACES];
        surfaces[7] = Some(test_surface(UI_SURFACE_GPU_BASE, TEST_BYTES));
        surfaces[2] = Some(test_surface(UI_SURFACE_GPU_BASE + TEST_BYTES as u64, TEST_BYTES));

        assert_eq!(
            allocate_surface_gpu_va(&surfaces, TEST_BYTES),
            Some(UI_SURFACE_GPU_BASE + (2 * TEST_BYTES) as u64)
        );
    }

    #[test]
    fn producer_va_reuses_first_fitting_hole() {
        let mut surfaces = [None; MAX_UI_SURFACES];
        surfaces[0] = Some(test_surface(UI_SURFACE_GPU_BASE + TEST_BYTES as u64, TEST_BYTES));

        assert_eq!(allocate_surface_gpu_va(&surfaces, TEST_BYTES), Some(UI_SURFACE_GPU_BASE));
    }

    #[test]
    fn producer_va_rejects_exhausted_arena() {
        let mut surfaces = [None; MAX_UI_SURFACES];
        let max_surface_bytes = 0x0200_0000usize;
        for (slot, surface) in surfaces.iter_mut().take(7).enumerate() {
            *surface = Some(test_surface(
                UI_SURFACE_GPU_BASE + (slot * max_surface_bytes) as u64,
                max_surface_bytes,
            ));
        }

        assert_eq!(allocate_surface_gpu_va(&surfaces, TEST_BYTES), None);
    }

    #[test]
    fn producer_va_fits_all_preview_sized_handles() {
        let mut surfaces = [None; MAX_UI_SURFACES];
        let preview_bytes = 768usize * 512 * 4;
        for slot in 0..MAX_UI_SURFACES {
            let gpu = allocate_surface_gpu_va(&surfaces, preview_bytes)
                .expect("packed preview-sized surface must fit");
            surfaces[slot] = Some(test_surface(gpu, preview_bytes));
        }
        let last = surfaces[MAX_UI_SURFACES - 1].unwrap();
        assert!(last.desc.gpu + last.byte_len as u64 <= UI_SURFACE_GPU_LIMIT);
    }
}
