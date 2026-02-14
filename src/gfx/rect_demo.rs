use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

static STARTED: AtomicBool = AtomicBool::new(false);

#[repr(C)]
#[derive(Clone, Copy)]
struct Vtx {
    x: f32,
    y: f32,
    r: u8,
    g: u8,
    b: u8,
    _pad: u8,
}

pub fn spawn_moving_rect(spawner: &Spawner) -> bool {
    if STARTED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    spawner.spawn(moving_rect_task()).is_ok()
}

#[embassy_executor::task]
async fn moving_rect_task() {
    // NDC-ish coords (matching current gfx pipeline expectation).
    let w = 0.35f32;
    let h = 0.22f32;
    let y0 = -0.2f32;

    let (r, g, b) = (255u8, 105u8, 180u8); // hot pink

    let mut t: f32 = 0.0;

    loop {
        // Simple horizontal oscillation.
        let x_center = libm::sinf(t) * 0.55;
        t += 0.12;

        let x0 = x_center - w * 0.5;
        let x1 = x_center + w * 0.5;
        let y1 = y0 + h;

        // Two triangles (6 verts).
        let verts = [
            Vtx {
                x: x0,
                y: y0,
                r,
                g,
                b,
                _pad: 0,
            },
            Vtx {
                x: x1,
                y: y0,
                r,
                g,
                b,
                _pad: 0,
            },
            Vtx {
                x: x1,
                y: y1,
                r,
                g,
                b,
                _pad: 0,
            },
            Vtx {
                x: x0,
                y: y0,
                r,
                g,
                b,
                _pad: 0,
            },
            Vtx {
                x: x1,
                y: y1,
                r,
                g,
                b,
                _pad: 0,
            },
            Vtx {
                x: x0,
                y: y1,
                r,
                g,
                b,
                _pad: 0,
            },
        ];

        let bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                verts.as_ptr() as *const u8,
                core::mem::size_of_val(verts.as_slice()),
            )
        };

        // Clear uses console-blue and draws the rect.
        // Ignore return code for now; the command exists to prove the end-to-end path.
        unsafe {
            let _ = crate::surface::io::cabi::trueos_cabi_gfx_draw_rgb_triangles(
                0x00_08_18_30,
                bytes.as_ptr(),
                bytes.len(),
            );
        }

        Timer::after(EmbassyDuration::from_millis(100)).await;
    }
}
