extern crate alloc;

use alloc::{vec, vec::Vec};

const UI2_ANALOG_CLOCK_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::AnalogClock.get();
const UI2_ANALOG_CLOCK_CONTENT_ID: u32 = crate::tst::ui2::ids::Ui2DemoContentId::AnalogClock.get();
const UI2_ANALOG_CLOCK_WINDOW_TITLE: &str = "Analog 360";
const UI2_ANALOG_CLOCK_VIEW_W: u32 = 192;
const UI2_ANALOG_CLOCK_VIEW_H: u32 = 192;
const UI2_ANALOG_CLOCK_WINDOW_X: f32 = 500.0;
const UI2_ANALOG_CLOCK_WINDOW_Y: f32 = 92.0;
const UI2_ANALOG_CLOCK_WINDOW_Z: i16 = 40;
const UI2_ANALOG_CLOCK_WINDOW_ALPHA: u8 = 0xFF;
const UI2_ANALOG_CLOCK_PI: f32 = 3.141_592_7;
const UI2_ANALOG_CLOCK_TAU: f32 = UI2_ANALOG_CLOCK_PI * 2.0;

const UI2_ANALOG_CLOCK_BG_RGBA: [u8; 4] = [0x0B, 0x0F, 0x14, 0xFF];
const UI2_ANALOG_CLOCK_FACE_RGBA: [u8; 4] = [0x14, 0x1A, 0x22, 0xFF];
const UI2_ANALOG_CLOCK_RING_RGBA: [u8; 4] = [0x7A, 0x92, 0xA0, 0xFF];
const UI2_ANALOG_CLOCK_TICK_RGBA: [u8; 4] = [0xC5, 0xD1, 0xD8, 0xFF];
const UI2_ANALOG_CLOCK_MINOR_TICK_RGBA: [u8; 4] = [0x54, 0x65, 0x70, 0xFF];
const UI2_ANALOG_CLOCK_NEEDLE_RGBA: [u8; 4] = [0xF0, 0x66, 0x5E, 0xFF];
const UI2_ANALOG_CLOCK_CAP_RGBA: [u8; 4] = [0xF5, 0xF0, 0xE8, 0xFF];

#[derive(Clone, Copy, Debug)]
struct DirtyRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl DirtyRect {
    fn needle(second: u32) -> Self {
        let (cx, cy) = clock_center();
        let (ex, ey) = needle_endpoint(second);
        let pad = 7;
        let left = (cx.min(ex) - pad).max(0) as u32;
        let top = (cy.min(ey) - pad).max(0) as u32;
        let right = (cx.max(ex) + pad + 1).min(UI2_ANALOG_CLOCK_VIEW_W as i32) as u32;
        let bottom = (cy.max(ey) + pad + 1).min(UI2_ANALOG_CLOCK_VIEW_H as i32) as u32;
        Self {
            x: left,
            y: top,
            w: right.saturating_sub(left).max(1),
            h: bottom.saturating_sub(top).max(1),
        }
    }

    fn union(self, other: Self) -> Self {
        let left = self.x.min(other.x);
        let top = self.y.min(other.y);
        let right = self
            .x
            .saturating_add(self.w)
            .max(other.x.saturating_add(other.w));
        let bottom = self
            .y
            .saturating_add(self.h)
            .max(other.y.saturating_add(other.h));
        Self {
            x: left,
            y: top,
            w: right.saturating_sub(left).max(1),
            h: bottom.saturating_sub(top).max(1),
        }
    }
}

fn clock_center() -> (i32, i32) {
    ((UI2_ANALOG_CLOCK_VIEW_W / 2) as i32, (UI2_ANALOG_CLOCK_VIEW_H / 2) as i32)
}

fn second_angle(second: u32) -> f32 {
    ((second % 60) as f32 / 60.0) * UI2_ANALOG_CLOCK_TAU - UI2_ANALOG_CLOCK_PI * 0.5
}

fn needle_endpoint(second: u32) -> (i32, i32) {
    let (cx, cy) = clock_center();
    let angle = second_angle(second);
    let radius = 76.0;
    (
        libm::roundf(cx as f32 + libm::cosf(angle) * radius) as i32,
        libm::roundf(cy as f32 + libm::sinf(angle) * radius) as i32,
    )
}

fn fill_rgba(dst: &mut [u8], rgba: [u8; 4]) {
    for px in dst.chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

fn put_pixel_rgba(dst: &mut [u8], width: u32, height: u32, x: i32, y: i32, rgba: [u8; 4]) {
    if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
        return;
    }
    let idx = ((y as u32 * width + x as u32) as usize) * 4;
    dst[idx..idx + 4].copy_from_slice(&rgba);
}

fn draw_disc_rgba(
    dst: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    radius: i32,
    rgba: [u8; 4],
) {
    let r2 = radius * radius;
    for y in -radius..=radius {
        for x in -radius..=radius {
            if x * x + y * y <= r2 {
                put_pixel_rgba(dst, width, height, cx + x, cy + y, rgba);
            }
        }
    }
}

fn draw_line_rgba(
    dst: &mut [u8],
    width: u32,
    height: u32,
    mut x0: i32,
    mut y0: i32,
    x1: i32,
    y1: i32,
    thickness: i32,
    rgba: [u8; 4],
) {
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_disc_rgba(dst, width, height, x0, y0, thickness, rgba);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = err.saturating_mul(2);
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn draw_annulus_rgba(
    dst: &mut [u8],
    width: u32,
    height: u32,
    cx: i32,
    cy: i32,
    inner_radius: i32,
    outer_radius: i32,
    rgba: [u8; 4],
) {
    let inner2 = inner_radius * inner_radius;
    let outer2 = outer_radius * outer_radius;
    for y in (cy - outer_radius)..=(cy + outer_radius) {
        for x in (cx - outer_radius)..=(cx + outer_radius) {
            let dx = x - cx;
            let dy = y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 >= inner2 && d2 <= outer2 {
                put_pixel_rgba(dst, width, height, x, y, rgba);
            }
        }
    }
}

fn draw_clock_face() -> Vec<u8> {
    let width = UI2_ANALOG_CLOCK_VIEW_W;
    let height = UI2_ANALOG_CLOCK_VIEW_H;
    let mut pixels = vec![0u8; (width as usize) * (height as usize) * 4];
    fill_rgba(pixels.as_mut_slice(), UI2_ANALOG_CLOCK_BG_RGBA);

    let (cx, cy) = clock_center();
    draw_disc_rgba(pixels.as_mut_slice(), width, height, cx, cy, 88, UI2_ANALOG_CLOCK_FACE_RGBA);
    draw_annulus_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        cx,
        cy,
        84,
        87,
        UI2_ANALOG_CLOCK_RING_RGBA,
    );
    draw_annulus_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        cx,
        cy,
        65,
        66,
        [0x28, 0x31, 0x39, 0xFF],
    );

    for tick in 0..60u32 {
        let major = tick % 5 == 0;
        let angle = second_angle(tick);
        let outer = 80.0;
        let inner = if major { 66.0 } else { 73.0 };
        let x0 = libm::roundf(cx as f32 + libm::cosf(angle) * inner) as i32;
        let y0 = libm::roundf(cy as f32 + libm::sinf(angle) * inner) as i32;
        let x1 = libm::roundf(cx as f32 + libm::cosf(angle) * outer) as i32;
        let y1 = libm::roundf(cy as f32 + libm::sinf(angle) * outer) as i32;
        draw_line_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            x0,
            y0,
            x1,
            y1,
            if major { 2 } else { 1 },
            if major {
                UI2_ANALOG_CLOCK_TICK_RGBA
            } else {
                UI2_ANALOG_CLOCK_MINOR_TICK_RGBA
            },
        );
    }

    draw_disc_rgba(pixels.as_mut_slice(), width, height, cx, cy, 4, UI2_ANALOG_CLOCK_CAP_RGBA);
    pixels
}

fn draw_needle(pixels: &mut [u8], second: u32) {
    let (cx, cy) = clock_center();
    let (ex, ey) = needle_endpoint(second);
    draw_line_rgba(
        pixels,
        UI2_ANALOG_CLOCK_VIEW_W,
        UI2_ANALOG_CLOCK_VIEW_H,
        cx,
        cy,
        ex,
        ey,
        1,
        UI2_ANALOG_CLOCK_NEEDLE_RGBA,
    );
    draw_disc_rgba(
        pixels,
        UI2_ANALOG_CLOCK_VIEW_W,
        UI2_ANALOG_CLOCK_VIEW_H,
        cx,
        cy,
        5,
        UI2_ANALOG_CLOCK_CAP_RGBA,
    );
}

fn restore_rect_from_face(pixels: &mut [u8], face: &[u8], rect: DirtyRect) {
    let stride = UI2_ANALOG_CLOCK_VIEW_W as usize * 4;
    let row_bytes = rect.w as usize * 4;
    for row in 0..rect.h as usize {
        let offset = ((rect.y as usize + row) * stride) + rect.x as usize * 4;
        pixels[offset..offset + row_bytes].copy_from_slice(&face[offset..offset + row_bytes]);
    }
}

fn copy_rect_rgba(pixels: &[u8], rect: DirtyRect) -> Vec<u8> {
    let stride = UI2_ANALOG_CLOCK_VIEW_W as usize * 4;
    let row_bytes = rect.w as usize * 4;
    let mut out = Vec::with_capacity(row_bytes.saturating_mul(rect.h as usize));
    for row in 0..rect.h as usize {
        let offset = ((rect.y as usize + row) * stride) + rect.x as usize * 4;
        out.extend_from_slice(&pixels[offset..offset + row_bytes]);
    }
    out
}

fn current_time_seconds() -> u64 {
    crate::r::net::ntp::current_unix_seconds()
        .or_else(crate::time::unix_time_seconds)
        .unwrap_or_else(crate::time::uptime_seconds)
}

fn current_second_index() -> u32 {
    (current_time_seconds() % 60) as u32
}

fn millis_until_next_second() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    let now = embassy_time_driver::now();
    let rem = now % hz;
    let ticks = if rem == 0 { hz } else { hz - rem };
    ((ticks.saturating_mul(1000).saturating_add(hz - 1)) / hz).clamp(25, 1000)
}

#[embassy_executor::task]
pub async fn ui2_analog_clock_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-analog-clock-demo");
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::get_or_create_for_hosted_content_with_size(
        UI2_ANALOG_CLOCK_WINDOW_TITLE,
        crate::r::ui2::Ui2Rect {
            x: UI2_ANALOG_CLOCK_WINDOW_X,
            y: UI2_ANALOG_CLOCK_WINDOW_Y,
            w: UI2_ANALOG_CLOCK_VIEW_W as f32,
            h: UI2_ANALOG_CLOCK_VIEW_H as f32,
        },
        UI2_ANALOG_CLOCK_WINDOW_Z,
        UI2_ANALOG_CLOCK_WINDOW_ALPHA,
        UI2_ANALOG_CLOCK_CONTENT_ID,
        UI2_ANALOG_CLOCK_TEX_ID,
        false,
        UI2_ANALOG_CLOCK_VIEW_W,
        UI2_ANALOG_CLOCK_VIEW_H,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-analog-clock-demo");

    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_resize_maintain_aspect(window_id, true);
    let _ = crate::r::ui2::set_window_content_preserve_scale(window_id, true);

    let (face, mut pixels, mut last_second, initial_uploaded) =
        crate::r::spawn_service::with_task_domain("ui2-analog-clock-demo", || {
            let face = draw_clock_face();
            let mut pixels = face.clone();
            let last_second = current_second_index();
            draw_needle(pixels.as_mut_slice(), last_second);
            let uploaded = surface.upload_rgba_owned(pixels.clone(), "ui2-analog-clock-init");
            (face, pixels, last_second, uploaded)
        });
    if !initial_uploaded {
        return;
    }

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-analog-clock-demo") {
            break;
        }

        let next_second = current_second_index();
        if next_second != last_second {
            let uploaded =
                crate::r::spawn_service::with_task_domain("ui2-analog-clock-demo", || {
                    let dirty =
                        DirtyRect::needle(last_second).union(DirtyRect::needle(next_second));
                    restore_rect_from_face(pixels.as_mut_slice(), face.as_slice(), dirty);
                    draw_needle(pixels.as_mut_slice(), next_second);
                    let region = copy_rect_rgba(pixels.as_slice(), dirty);
                    surface.upload_rgba_region(
                        dirty.x,
                        dirty.y,
                        dirty.w,
                        dirty.h,
                        region.as_slice(),
                        "ui2-analog-clock-tick",
                    )
                });
            if !uploaded {
                break;
            }
            last_second = next_second;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(
            "ui2-analog-clock-demo",
            millis_until_next_second(),
        )
        .await
        {
            break;
        }
    }
}
