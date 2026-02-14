use core::sync::atomic::{AtomicBool, Ordering};

use libm::{cosf, roundf, sinf};

static RENDERED_ONCE: AtomicBool = AtomicBool::new(false);

pub fn tick() {
    // This is a BSP-only bring-up indicator drawn directly into the Limine framebuffer.
    // Keep it one-shot so it can’t cause visible flicker or chew CPU during boot.
    if RENDERED_ONCE.swap(true, Ordering::AcqRel) {
        return;
    }

    let _ = super::with_framebuffer(|fb| {
        // Place a small cube at the top-right, avoiding the banner text on the left.
        let margin = 6usize;
        let size = (fb.width.min(fb.height) / 6).clamp(48, 120);
        let origin_x = fb.width.saturating_sub(size).saturating_sub(margin);
        let origin_y = margin;

        // Clear the cube tile so it stands out cleanly.
        fb.clear_rect(origin_x, origin_y, size, size, super::DEFAULT_BG_COLOR);
        draw_wire_cube(fb, origin_x as i32, origin_y as i32, size as i32);
    });

    // Keep the boot logo visible even if overlays draw after it.
    crate::efi::acpi::bgrt::log_once();
}

fn draw_wire_cube(fb: &super::FramebufferSurface, ox: i32, oy: i32, size: i32) {
    if size <= 8 {
        return;
    }

    let cx = ox + size / 2;
    let cy = oy + size / 2;
    let scale = (size as f32) * 0.42;

    // Fixed “nice looking” orientation; this is a boot proof marker, not an animation.
    let ax = 0.65f32;
    let ay = 0.95f32;
    let (cax, sax) = (cosf(ax), sinf(ax));
    let (cay, say) = (cosf(ay), sinf(ay));

    // Perspective distance.
    let d = 3.2f32;

    // Vertices in bit order: x=bit0, y=bit1, z=bit2.
    let mut pts = [(0i32, 0i32, 0f32); 8];
    for i in 0..8 {
        let x0 = if (i & 1) == 0 { -1.0f32 } else { 1.0f32 };
        let y0 = if (i & 2) == 0 { -1.0f32 } else { 1.0f32 };
        let z0 = if (i & 4) == 0 { -1.0f32 } else { 1.0f32 };

        // Rotate around X.
        let y1 = y0 * cax - z0 * sax;
        let z1 = y0 * sax + z0 * cax;

        // Rotate around Y.
        let x2 = x0 * cay + z1 * say;
        let z2 = -x0 * say + z1 * cay;

        let inv = 1.0f32 / (z2 + d);
        let sx = cx as f32 + x2 * scale * inv;
        let sy = cy as f32 + y1 * scale * inv;
        pts[i] = (roundf(sx) as i32, roundf(sy) as i32, z2);
    }

    // Connect edges between vertices that differ by exactly one bit.
    for a in 0..8u8 {
        for bit in 0..3u8 {
            let b = a ^ (1u8 << bit);
            if a < b {
                let (x0, y0, z0) = pts[a as usize];
                let (x1, y1, z1) = pts[b as usize];
                let z = (z0 + z1) * 0.5;
                let color = if z >= 0.0 {
                    0x00_FF_FF_FF
                } else {
                    0x00_90_A0_B0
                };
                fb.draw_line(x0, y0, x1, y1, color);
            }
        }
    }
}
