use crate::intel::types::{
    Error, Result, UiPlaneSlot, UiPresent, UiPresentPath, UiRect, UiSurface, UiSurfaceFormat,
};
use spin::Mutex;

const MAX_UI_SURFACES: usize = 8;
const UI_SURFACE_GPU_BASE: u64 = 0x1200_0000;
const UI_SURFACE_GPU_STRIDE: u64 = 0x0200_0000;
const UI_SURFACE_BYTES_PER_PIXEL: u32 = 4;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct UiSurfaceHandle(u32);

impl UiSurfaceHandle {
    #[inline]
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
    pub virt: *mut u8,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
}

unsafe impl Send for UiSurfaceRgbaAccess {}
unsafe impl Sync for UiSurfaceRgbaAccess {}

#[derive(Clone, Copy)]
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

pub fn create_surface(width: u32, height: u32, format: UiSurfaceFormat) -> Result<UiSurfaceHandle> {
    if width == 0 || height == 0 {
        return Err(Error::Invalid);
    }
    let pitch = aligned_pitch_bytes(width)?;
    let raw_len = (pitch as usize)
        .checked_mul(height as usize)
        .ok_or(Error::Invalid)?;
    let byte_len =
        crate::intel::align_up(raw_len, crate::intel::WARM_ALIGN).ok_or(Error::Invalid)?;
    if byte_len as u64 > UI_SURFACE_GPU_STRIDE {
        return Err(Error::OutOfMemory);
    }

    let mut surfaces = SURFACES.lock();
    let Some(slot) = surfaces.iter().position(Option::is_none) else {
        return Err(Error::OutOfMemory);
    };
    let gpu = UI_SURFACE_GPU_BASE + (slot as u64) * UI_SURFACE_GPU_STRIDE;
    let Some(dev) = crate::intel::claimed_device() else {
        return Err(Error::Unsupported);
    };
    let Some((phys, virt)) = crate::dma::alloc(byte_len, crate::intel::WARM_ALIGN) else {
        return Err(Error::OutOfMemory);
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, byte_len);
    }
    crate::intel::dma_flush(virt, byte_len);

    if !crate::intel::map_ggtt(dev, phys, byte_len, gpu) {
        crate::dma::dealloc(virt, byte_len);
        return Err(Error::Unsupported);
    }
    crate::intel::ggtt_invalidate(dev);

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

pub fn surface(handle: UiSurfaceHandle) -> Option<UiSurface> {
    lookup(handle).map(|surface| surface.desc)
}

pub(crate) fn rgba_access(handle: UiSurfaceHandle) -> Option<UiSurfaceRgbaAccess> {
    let surface = lookup(handle)?;
    if surface.desc.format != UiSurfaceFormat::Rgba8888 {
        return None;
    }
    Some(UiSurfaceRgbaAccess {
        virt: surface.virt,
        byte_len: surface.byte_len,
        width: surface.desc.width,
        height: surface.desc.height,
        pitch: surface.desc.pitch,
    })
}

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

pub(crate) fn flush_surface(handle: UiSurfaceHandle) -> bool {
    let Some(surface) = lookup(handle) else {
        return false;
    };
    crate::intel::dma_flush(surface.virt, surface.byte_len);
    true
}

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

pub fn present_surface(
    handle: UiSurfaceHandle,
    present: UiPresent,
    reason: &'static str,
) -> Result<UiPresentPath> {
    let surface = lookup(handle).ok_or(Error::NotFound)?;
    let src = clip_rect_to_surface(present.src, surface.desc).ok_or(Error::Invalid)?;
    if present.dst.is_empty() {
        return Err(Error::Invalid);
    }

    match present.plane {
        UiPlaneSlot::Primary => present_primary(surface, src, present.dst, reason),
        UiPlaneSlot::Overlay(_) => Err(Error::Unsupported),
    }
}

fn present_primary(
    surface: TrustedUiSurface,
    src: UiRect,
    dst: UiRect,
    reason: &'static str,
) -> Result<UiPresentPath> {
    if surface.desc.format == UiSurfaceFormat::Rgba8888 {
        if present_primary_rgba_kernel_blit(surface, src, dst)? {
            return Ok(UiPresentPath::KernelBlit);
        }

        if present_primary_backing_copy(surface, src, dst, reason) {
            return Ok(UiPresentPath::CpuCopy);
        }

        return Err(Error::Unsupported);
    }

    if present_primary_backing_copy(surface, src, dst, reason) {
        return Ok(UiPresentPath::CpuCopy);
    }

    if present_primary_rgba_kernel_blit(surface, src, dst)? {
        return Ok(UiPresentPath::KernelBlit);
    }

    Err(Error::Unsupported)
}

fn present_primary_backing_copy(
    surface: TrustedUiSurface,
    src: UiRect,
    dst: UiRect,
    reason: &'static str,
) -> bool {
    matches!(
        surface.desc.format,
        UiSurfaceFormat::Rgba8888 | UiSurfaceFormat::Xrgb8888 | UiSurfaceFormat::Xbgr8888
    ) && crate::intel::present_ui_surface_to_primary_backing(
        surface.desc,
        surface.virt.cast_const(),
        surface.byte_len,
        src,
        dst,
        reason,
    )
}

fn present_primary_rgba_kernel_blit(
    surface: TrustedUiSurface,
    src: UiRect,
    dst: UiRect,
) -> Result<bool> {
    if surface.desc.format != UiSurfaceFormat::Rgba8888 {
        return Ok(false);
    }

    let src_surface = crate::intel::gpgpu::GpgpuRgba8Surface::new(
        surface.phys,
        surface.desc.gpu,
        surface.byte_len,
        surface.desc.width,
        surface.desc.height,
        surface.desc.pitch,
    )
    .ok_or(Error::Invalid)?;
    let src_rect = crate::intel::gpgpu::GpgpuRect::new(
        i32::try_from(src.x).map_err(|_| Error::Invalid)?,
        i32::try_from(src.y).map_err(|_| Error::Invalid)?,
        src.w,
        src.h,
    );
    let dst_xy = crate::intel::gpgpu::GpgpuPoint::new(
        i32::try_from(dst.x).map_err(|_| Error::Invalid)?,
        i32::try_from(dst.y).map_err(|_| Error::Invalid)?,
    );
    Ok(crate::intel::gpgpu::present_rgba8_rect_to_primary_xrgb_stats_with_flip(
        src_surface,
        src_rect,
        dst_xy,
        false,
    )
    .is_some())
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
