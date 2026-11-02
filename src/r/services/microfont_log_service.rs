//! Live laptop IP verdict and USB bring-up stage on the retained text surface.
//! CPU-only writer on the retained Slot4 surface; no TCP log consumption.

use spin::Once;

pub(crate) const PLANE_SLOT: usize = 4;
const SCALE: usize = 4;
const PERIOD_MS: u64 = 25;
const BG: u32 = 0xFFFF_FFFF;
static SURFACE: Once<Surface> = Once::new();
// Protected by STATUS together with all surface writes.
static WIFI_COLOR: core::sync::atomic::AtomicU32 =
    core::sync::atomic::AtomicU32::new(0xff00_00ff);
static WIFI_ATTEMPT_MS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(u64::MAX);
// 256 means no current Wi-Fi IPv4 address; 0..255 is its last octet.
static WIFI_IP_OCTET: core::sync::atomic::AtomicU16 =
    core::sync::atomic::AtomicU16::new(256);
// Synchronous publication works even before the background executor runs.
static STATUS: spin::Mutex<(&'static str, Option<bool>, bool)> =
    spin::Mutex::new(("USB service not started", None, false));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Surface {
    pub phys: u64,
    pub gpu: u64,
    pub virt: usize,
    pub byte_len: usize,
    pub width: u32,
    pub height: u32,
    pub pitch_bytes: u32,
}

impl Surface {
    fn valid(self) -> bool {
        let Some(row_bytes) = (self.width as usize).checked_mul(4) else {
            return false;
        };
        let Some(bytes) = (self.pitch_bytes as usize).checked_mul(self.height as usize) else {
            return false;
        };
        self.virt != 0
            && self.virt % 4 == 0
            && self.phys != 0
            && self.phys % 4096 == 0
            && self.gpu % 4096 == 0
            && self.pitch_bytes % 4 == 0
            && self.pitch_bytes as usize >= row_bytes
            && bytes <= self.byte_len
            && self.byte_len <= isize::MAX as usize
            && self.virt.checked_add(self.byte_len).is_some()
            && self.width as usize >= 3 * SCALE * microfont::FWIDTH
            && self.height as usize >= SCALE * microfont::FHEIGHT
    }
}

/// Boot-only handoff, before AP registration and normal UI4 bootstrap.
/// # Safety
/// The caller permanently retains this mapped allocation. After this call,
/// only this service may write it; display may read it but no GPU may render
/// into it, and no other owner may clear, resize, free or reuse it.
pub(crate) unsafe fn install(surface: Surface) -> bool {
    if !surface.valid() {
        return false;
    }
    let installed = *SURFACE.call_once(|| {
        let mut status = STATUS.lock();
        status.1 = crate::net::laptop_profile::assignment_result();
        repaint(surface, *status);
        surface
    }) == surface;
    // Close the race with stage/configuration callbacks before Once publication.
    if installed {
        let mut status = STATUS.lock();
        status.1 = crate::net::laptop_profile::assignment_result();
        repaint(surface, *status);
    }
    installed
}

/// Called synchronously by NIC configuration; independent of AP/task timers.
pub(crate) fn assignment_changed(assigned: bool) {
    let mut status = STATUS.lock();
    if status.1 != Some(assigned) {
        status.1 = Some(assigned);
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

/// Publish the operation before awaiting hardware; a stalled transfer leaves its
/// exact stage visible. Never takes a NIC lock or depends on task timers.
pub(crate) fn usb_stage(stage: &'static str) {
    let mut status = STATUS.lock();
    status.2 = true;
    if status.0 != stage {
        status.0 = stage;
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

/// Once the adapter is found, another controller must not hide its progress.
pub(crate) fn usb_controller_stage(stage: &'static str) {
    let mut status = STATUS.lock();
    if !status.2 && status.0 != stage {
        status.0 = stage;
        if let Some(surface) = surface() {
            repaint(surface, *status);
        }
    }
}

pub(crate) fn font_ready() {
    let status = STATUS.lock();
    if let Some(surface) = surface() { repaint(surface, *status); }
}

/// Radio status uses the same retained CPU surface, independent of fonts/UI.
pub(crate) fn wifi_retry_started() {
    WIFI_ATTEMPT_MS.store(trueos_time::Instant::now().as_millis(), core::sync::atomic::Ordering::Relaxed);
    wifi_color_changed(0xff00_00ff);
}

pub(crate) fn wifi_color_changed(color: u32) {
    let attempted = WIFI_ATTEMPT_MS.load(core::sync::atomic::Ordering::Relaxed);
    // Make even a promptly rejected retry visible for half a second.
    let color = if color == 0xffff_0000 && attempted != u64::MAX
        && trueos_time::Instant::now().as_millis().saturating_sub(attempted) < 500
    { 0xff00_00ff } else { color };
    let _status = STATUS.lock();
    if WIFI_COLOR.swap(color, core::sync::atomic::Ordering::Relaxed) != color {
        if let Some(surface) = surface() {
            paint_wifi_square(surface);
        }
    }
}

pub(crate) fn wifi_ipv4_changed(address: Option<[u8; 4]>) {
    let octet = address.map_or(256, |ip| u16::from(ip[3]));
    let _status = STATUS.lock();
    if WIFI_IP_OCTET.swap(octet, core::sync::atomic::Ordering::Relaxed) != octet {
        if let Some(surface) = surface() {
            paint_wifi_ip(surface);
        }
    }
}

/// Eight 16px cells, MSB first: a dot is zero, a vertical line is one.
/// Independent of the font atlas so this works during early boot.
fn paint_wifi_ip(surface: Surface) {
    let octet = WIFI_IP_OCTET.load(core::sync::atomic::Ordering::Relaxed);
    let left = (surface.width as usize).saturating_sub(8 * 16);
    let top = 72;
    for y in top..88usize.min(surface.height as usize) {
        for x in left..(left + 128).min(surface.width as usize) {
            let cell = (x - left) / 16;
            let dx = (x - left) % 16;
            let dy = y - top;
            let ink = octet < 256 && if octet & (1 << (7 - cell)) != 0 {
                (7..10).contains(&dx) && (2..14).contains(&dy)
            } else {
                (7..10).contains(&dx) && (11..14).contains(&dy)
            };
            let color = if ink { 0xff00_0000 } else { 0xffff_ffff };
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt + y * surface.pitch_bytes as usize + x * 4) as *mut u32,
                    color,
                );
            }
        }
    }
    if surface.height as usize > top {
        let _ = crate::intel::dma_flush_strided_rows(
            (surface.virt + top * surface.pitch_bytes as usize + left * 4) as *mut u8,
            128usize.min(surface.width as usize) * 4,
            surface.pitch_bytes as usize,
            (surface.height as usize - top).min(16),
        );
    }
}

fn paint_wifi_square(surface: Surface) {
    let color = WIFI_COLOR.load(core::sync::atomic::Ordering::Relaxed);
    let width = 64usize.min(surface.width as usize);
    let height = 64usize.min(surface.height as usize);
    // Top-right corner; clip for small surfaces and preserve pitch padding.
    let left = surface.width as usize - width;
    for y in 0..height {
        for x in left..surface.width as usize {
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt + y * surface.pitch_bytes as usize + x * 4) as *mut u32,
                    color,
                );
            }
        }
    }
    let _ = crate::intel::dma_flush_strided_rows(
        (surface.virt + left * 4) as *mut u8,
        width * 4,
        surface.pitch_bytes as usize,
        height,
    );
}

fn repaint(surface: Surface, status: (&str, Option<bool>, bool)) {
    unsafe {
        clear_surface(surface);
    }
    paint_verdict(surface, status.1.unwrap_or(false));
    // Fit the status below the verdict, including on small diagnostic surfaces.
    let top = (surface.height as usize + microfont::FHEIGHT * SCALE) / 2;
    let scale = SCALE
        .min(surface.width as usize / (status.0.len().max(1) * microfont::FWIDTH))
        .min((surface.height as usize - top) / microfont::FHEIGHT);
    if scale > 0 {
        paint_text(surface, status.0.as_bytes(), scale, top);
    }
    paint_wifi_square(surface);
    paint_wifi_ip(surface);
    crate::intel::dma_cache_flush_range(surface.virt as *const u8, surface.byte_len);
}

pub(crate) fn enabled() -> bool {
    SURFACE.get().is_some()
}
pub(crate) fn surface() -> Option<Surface> {
    SURFACE.get().copied()
}
pub(crate) fn owns_plane(pipe: usize, slot: usize) -> bool {
    pipe == 0 && slot == PLANE_SLOT && enabled()
}

/// Fill visible pixels only; preserve pitch padding and allocation guards.
unsafe fn clear_surface(surface: Surface) {
    for y in 0..surface.height as usize {
        for x in 0..surface.width as usize {
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt + y * surface.pitch_bytes as usize + x * 4) as *mut u32,
                    BG,
                );
            }
        }
    }
}

fn paint_verdict(surface: Surface, assigned: bool) {
    let text: &[u8] = if assigned { b"Yes" } else { b"No" };
    let top = (surface.height as usize - microfont::FHEIGHT * SCALE) / 2;
    paint_text(surface, text, SCALE, top);
}

fn paint_text(surface: Surface, text: &[u8], scale: usize, top: usize) {
    let width = text.len() * microfont::FWIDTH * scale;
    let height = microfont::FHEIGHT * scale;
    let left = (surface.width as usize - width) / 2;
    for (column, &byte) in text.iter().enumerate() {
        let Some(atlas) = super::microfont_mask_service::ready_atlas() else { return; };
        let ch = microfont::glyph_character(byte).unwrap_or(' ');
        for y in 0..height {
            for x in 0..microfont::FWIDTH * scale {
                let alpha = u32::from(atlas.coverage(ch, x as u32, y as u32,
                    (microfont::FWIDTH * scale) as u32, height as u32));
                let color = 0xff00_0000 | ((255 - alpha) * 0x010101);
                let offset = (top + y) * surface.pitch_bytes as usize
                    + (left + column * microfont::FWIDTH * scale + x) * 4;
                unsafe {
                    core::ptr::write_volatile((surface.virt + offset) as *mut u32, color);
                }
            }
        }
    }
    let _ = crate::intel::dma_flush_strided_rows(
        (surface.virt + top * surface.pitch_bytes as usize + left * 4) as *mut u8,
        width * 4,
        surface.pitch_bytes as usize,
        height,
    );
}

#[cfg(not(test))]
#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn microfont_log_task(assigned_slot: u32) {
    if crate::cpu::CpuProfile::current().map(|cpu| cpu.slot()) != Some(assigned_slot) {
        return;
    }
    let Some(_surface) = surface() else {
        return;
    };
    // Keep the IP result live: a slow discovery must never freeze a false No.
    loop {
        if let Some(assigned) = crate::net::laptop_profile::assignment_result() {
            assignment_changed(assigned);
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(PERIOD_MS)).await;
    }
}
