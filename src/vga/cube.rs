use core::sync::atomic::{AtomicU32, Ordering};

use libm::{cosf, roundf, sinf, sqrtf};
use spin::Mutex;

static TICK_COUNT: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone)]
struct Quat {
    w: f32,
    x: f32,
    y: f32,
    z: f32,
}

impl Quat {
    const fn identity() -> Self {
        Self {
            w: 1.0,
            x: 0.0,
            y: 0.0,
            z: 0.0,
        }
    }

    fn mul(self, b: Self) -> Self {
        // (w1, v1) * (w2, v2) = (w1*w2 - dot(v1,v2), w1*v2 + w2*v1 + cross(v1,v2))
        Self {
            w: self.w * b.w - (self.x * b.x + self.y * b.y + self.z * b.z),
            x: self.w * b.x + b.w * self.x + (self.y * b.z - self.z * b.y),
            y: self.w * b.y + b.w * self.y + (self.z * b.x - self.x * b.z),
            z: self.w * b.z + b.w * self.z + (self.x * b.y - self.y * b.x),
        }
    }

    fn normalized(self) -> Self {
        let n2 = self.w * self.w + self.x * self.x + self.y * self.y + self.z * self.z;
        if !(n2 > 0.0) {
            return Self::identity();
        }
        let inv = 1.0 / sqrtf(n2);
        Self {
            w: self.w * inv,
            x: self.x * inv,
            y: self.y * inv,
            z: self.z * inv,
        }
    }

    fn from_axis_angle(ax: f32, ay: f32, az: f32, angle: f32) -> Self {
        let n2 = ax * ax + ay * ay + az * az;
        if !(n2 > 0.0) {
            return Self::identity();
        }
        let inv_n = 1.0 / sqrtf(n2);
        let (nx, ny, nz) = (ax * inv_n, ay * inv_n, az * inv_n);

        let half = 0.5 * angle;
        let (s, c) = (sinf(half), cosf(half));
        Self {
            w: c,
            x: nx * s,
            y: ny * s,
            z: nz * s,
        }
        .normalized()
    }

    fn rotate_vec(self, vx: f32, vy: f32, vz: f32) -> (f32, f32, f32) {
        // Efficient quaternion-vector rotation (no full q*v*q^-1):
        // t = 2 * cross(q.xyz, v)
        // v' = v + q.w * t + cross(q.xyz, t)
        let (qx, qy, qz, qw) = (self.x, self.y, self.z, self.w);
        let tx = 2.0 * (qy * vz - qz * vy);
        let ty = 2.0 * (qz * vx - qx * vz);
        let tz = 2.0 * (qx * vy - qy * vx);

        let vpx = vx + qw * tx + (qy * tz - qz * ty);
        let vpy = vy + qw * ty + (qz * tx - qx * tz);
        let vpz = vz + qw * tz + (qx * ty - qy * tx);
        (vpx, vpy, vpz)
    }
}

static ORIENT: Mutex<Quat> = Mutex::new(Quat::identity());

pub fn tick() {
    // BSP-only bring-up indicator drawn directly into the Limine framebuffer.
    // Intentionally synced to *raw calls*: one animation step per `tick()` call.
    let n = TICK_COUNT.fetch_add(1, Ordering::Relaxed);

    // Quaternion-based rotation, updated strictly once per raw tick() call.
    // - Speed eases up/down (acceleration/deceleration)
    // - Axis precesses a bit (subtle direction changes)
    let nf = n as f32;
    let speed = 0.65 + 0.35 * sinf(nf * 0.017); // 0.30..1.00-ish
    let base = core::f32::consts::TAU / 420.0;
    let angle = base * speed;

    let ax = 0.22 * sinf(nf * 0.011);
    let ay = 1.00 + 0.08 * cosf(nf * 0.007);
    let az = 0.18 * cosf(nf * 0.013);
    let dq = Quat::from_axis_angle(ax, ay, az, angle);

    let q = {
        let mut q = ORIENT.lock();
        *q = dq.mul(*q).normalized();
        *q
    };

    let _ = super::with_framebuffer(|fb| {
        // Place a small cube at the top-right, avoiding the banner text on the left.
        let margin = 6usize;
        let size = (fb.width.min(fb.height) / 6).clamp(48, 120);
        let origin_x = fb.width.saturating_sub(size).saturating_sub(margin);
        let origin_y = margin;

        // Clear the cube tile so it stands out cleanly.
        fb.clear_rect(origin_x, origin_y, size, size, super::DEFAULT_BG_COLOR);
        draw_wire_cube(fb, origin_x as i32, origin_y as i32, size as i32, q);

        // Cursor visualization (mouse/tablet) is drawn inside this reserved BSP tile.
        // This preserves the "partitioned immediate-mode" VGA model: no global overlay and
        // no restore step that could clobber other producers' pixels.
        draw_cursor_rings_in_tile(fb, origin_x as i32, origin_y as i32, size as i32);
    });

    // Keep the boot logo visible even if overlays draw after it.
    crate::efi::acpi::bgrt::log_once();
}

fn draw_cursor_rings_in_tile(fb: &super::FramebufferSurface, ox: i32, oy: i32, size: i32) {
    if size <= 2 {
        return;
    }

    // Snapshot cursor positions up-front so we don't hold the HID runtime lock while drawing.
    let mice = crate::usb::hid::mouse_cursor_snapshot();
    let tablets = crate::usb::hid::tablet_cursor_snapshot();

    let span = (size - 1).max(1) as f32;

    #[inline]
    fn draw_ring_clipped(
        fb: &super::FramebufferSurface,
        ox: i32,
        oy: i32,
        size: i32,
        x: i32,
        y: i32,
        color: u32,
    ) {
        const R: i32 = 3;
        const R2: i32 = R * R;
        let max_x = ox + size;
        let max_y = oy + size;

        for dy in -R..=R {
            for dx in -R..=R {
                let d2 = dx * dx + dy * dy;
                if (d2 - R2).abs() > 2 {
                    continue;
                }
                let px = x + dx;
                let py = y + dy;
                if px < ox || py < oy || px >= max_x || py >= max_y {
                    continue;
                }
                fb.plot(px, py, color);
            }
        }
    }

    for (mx, my) in mice {
        let x = ox + roundf((mx as f32) * span) as i32;
        let y = oy + roundf((my as f32) * span) as i32;
        draw_ring_clipped(fb, ox, oy, size, x, y, 0x00_00_FF_00);
    }

    for (tx, ty) in tablets {
        let x = ox + roundf((tx as f32) * span) as i32;
        let y = oy + roundf((ty as f32) * span) as i32;
        draw_ring_clipped(fb, ox, oy, size, x, y, 0x00_FF_00_FF);
    }
}

fn draw_wire_cube(fb: &super::FramebufferSurface, ox: i32, oy: i32, size: i32, q: Quat) {
    if size <= 8 {
        return;
    }

    let cx = ox + size / 2;
    let cy = oy + size / 2;
    let scale = (size as f32) * 0.42;

    // Perspective distance.
    let d = 3.2f32;

    // Vertices in bit order: x=bit0, y=bit1, z=bit2.
    let mut pts = [(0i32, 0i32, 0f32); 8];
    for i in 0..8 {
        let x0 = if (i & 1) == 0 { -1.0f32 } else { 1.0f32 };
        let y0 = if (i & 2) == 0 { -1.0f32 } else { 1.0f32 };
        let z0 = if (i & 4) == 0 { -1.0f32 } else { 1.0f32 };

        let (x2, y2, z2) = q.rotate_vec(x0, y0, z0);

        let inv = 1.0f32 / (z2 + d);
        let sx = cx as f32 + x2 * scale * inv;
        let sy = cy as f32 + y2 * scale * inv;
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
