//! Font-free laptop Wi-Fi diagnostics on a dedicated retained plane.
//! Callback-driven CPU writes; the display owner centers this compact plane.

use spin::Once;

pub(crate) const PLANE_SLOT: usize = 6;
const BG: u32 = 0;
const SQUARE_SIZE: usize = 64;
const IP_CELL_SIZE: usize = 16;
const IP_WIDTH: usize = 8 * IP_CELL_SIZE;
const MARKER_GAP: usize = 8;
const MARKER_HEIGHT: usize = SQUARE_SIZE + MARKER_GAP + IP_CELL_SIZE;
static SURFACE: Once<Surface> = Once::new();
// Serializes all CPU writes, including initial publication.
static STATUS: spin::Mutex<()> = spin::Mutex::new(());
static WIFI_COLOR: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0xff00_00ff);
static WIFI_ATTEMPT_MS: core::sync::atomic::AtomicU64 =
    core::sync::atomic::AtomicU64::new(u64::MAX);
// 256 means no current Wi-Fi IPv4 address; 0..255 is its last octet.
static WIFI_IP_OCTET: core::sync::atomic::AtomicU16 = core::sync::atomic::AtomicU16::new(256);

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
            && diagnostic_geometry(self.width as usize, self.height as usize).is_some()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Rect {
    left: usize,
    top: usize,
    width: usize,
    height: usize,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DiagnosticGeometry {
    square: Rect,
    ip: Rect,
}

/// Center the complete marker group, including its eight-bit address row.
fn diagnostic_geometry(width: usize, height: usize) -> Option<DiagnosticGeometry> {
    if width < IP_WIDTH || height < MARKER_HEIGHT {
        return None;
    }
    let top = (height - MARKER_HEIGHT) / 2;
    Some(DiagnosticGeometry {
        square: Rect {
            left: (width - SQUARE_SIZE) / 2,
            top,
            width: SQUARE_SIZE,
            height: SQUARE_SIZE,
        },
        ip: Rect {
            left: (width - IP_WIDTH) / 2,
            top: top + SQUARE_SIZE + MARKER_GAP,
            width: IP_WIDTH,
            height: IP_CELL_SIZE,
        },
    })
}

/// Boot-only handoff before normal UI4 bootstrap.
/// # Safety
/// The caller permanently retains this mapped allocation. After this call,
/// only this service may write it; display may read it but no GPU may render
/// into it, and no other owner may clear, resize, free or reuse it.
pub(crate) unsafe fn install(surface: Surface) -> bool {
    if !surface.valid() {
        return false;
    }
    let installed = *SURFACE.call_once(|| {
        let _status = STATUS.lock();
        repaint(surface);
        surface
    }) == surface;
    // Close the race with callbacks before Once publishes the surface.
    if installed {
        let _status = STATUS.lock();
        repaint(surface);
    }
    installed
}

// Retain existing caller APIs while this plane presents only radio/IP markers.
pub(crate) fn assignment_changed(_assigned: bool) {}
pub(crate) fn usb_stage(_stage: &'static str) {}
pub(crate) fn usb_controller_stage(_stage: &'static str) {}
pub(crate) fn font_ready() {}

pub(crate) fn wifi_retry_started() {
    WIFI_ATTEMPT_MS.store(
        trueos_time::Instant::now().as_millis(),
        core::sync::atomic::Ordering::Relaxed,
    );
    wifi_color_changed(0xff00_00ff);
}

pub(crate) fn wifi_color_changed(color: u32) {
    let attempted = WIFI_ATTEMPT_MS.load(core::sync::atomic::Ordering::Relaxed);
    // Keep a promptly rejected retry visible for half a second.
    let color = if color == 0xffff_0000
        && attempted != u64::MAX
        && trueos_time::Instant::now()
            .as_millis()
            .saturating_sub(attempted)
            < 500
    {
        0xff00_00ff
    } else {
        color
    };
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

fn flush_rect(surface: Surface, rect: Rect) {
    let _ = crate::intel::dma_flush_strided_rows(
        (surface.virt + rect.top * surface.pitch_bytes as usize + rect.left * 4) as *mut u8,
        rect.width * 4,
        surface.pitch_bytes as usize,
        rect.height,
    );
}

fn write_wifi_square(surface: Surface, rect: Rect) {
    let color = WIFI_COLOR.load(core::sync::atomic::Ordering::Relaxed);
    for y in rect.top..rect.top + rect.height {
        for x in rect.left..rect.left + rect.width {
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt + y * surface.pitch_bytes as usize + x * 4) as *mut u32,
                    color,
                );
            }
        }
    }
}

/// Eight 16px cells, MSB first: a dot is zero, a vertical line is one.
fn write_wifi_ip(surface: Surface, rect: Rect) {
    let octet = WIFI_IP_OCTET.load(core::sync::atomic::Ordering::Relaxed);
    for y in 0..rect.height {
        for x in 0..rect.width {
            let cell = x / IP_CELL_SIZE;
            let dx = x % IP_CELL_SIZE;
            let ink = octet < 256
                && (7..10).contains(&dx)
                && if octet & (1 << (7 - cell)) != 0 {
                    (2..14).contains(&y)
                } else {
                    (11..14).contains(&y)
                };
            let color = if ink { 0xff00_0000 } else { 0xffff_ffff };
            unsafe {
                core::ptr::write_volatile(
                    (surface.virt
                        + (rect.top + y) * surface.pitch_bytes as usize
                        + (rect.left + x) * 4) as *mut u32,
                    color,
                );
            }
        }
    }
}

fn paint_wifi_square(surface: Surface) {
    let Some(geometry) = diagnostic_geometry(surface.width as usize, surface.height as usize)
    else {
        return;
    };
    write_wifi_square(surface, geometry.square);
    flush_rect(surface, geometry.square);
}

fn paint_wifi_ip(surface: Surface) {
    let Some(geometry) = diagnostic_geometry(surface.width as usize, surface.height as usize)
    else {
        return;
    };
    write_wifi_ip(surface, geometry.ip);
    flush_rect(surface, geometry.ip);
}

fn repaint(surface: Surface) {
    let Some(geometry) = diagnostic_geometry(surface.width as usize, surface.height as usize)
    else {
        return;
    };
    unsafe {
        clear_surface(surface);
    }
    write_wifi_square(surface, geometry.square);
    write_wifi_ip(surface, geometry.ip);
    flush_rect(
        surface,
        Rect {
            left: 0,
            top: 0,
            width: surface.width as usize,
            height: surface.height as usize,
        },
    );
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
