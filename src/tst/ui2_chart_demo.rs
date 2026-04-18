use alloc::{format, string::String, vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::{self, Ui2Rect};

// ---------------------------------------------------------------------------
// Axis settings – change these to adjust the visible range of the plot.
// ---------------------------------------------------------------------------

/// X-axis range.
const AXIS_X_MIN: f64 = -2.0;
const AXIS_X_MAX: f64 = 2.0;

/// Y-axis range.
const AXIS_Y_MIN: f64 = -1.0;
const AXIS_Y_MAX: f64 = 1.0;

/// Tick spacing on both axes.
const TICK_STEP: f64 = 0.5;

// ---------------------------------------------------------------------------
// Window / appearance constants.
// ---------------------------------------------------------------------------

const UI2_CHART_TEX_ID: u32 = 4_720;
const UI2_CHART_CONTENT_ID: u32 = 48;
const UI2_CHART_WINDOW_TITLE: &str = "Chart";
const UI2_CHART_VIEW_W: u32 = 480;
const UI2_CHART_VIEW_H: u32 = 320;
const UI2_CHART_WINDOW_X: f32 = 140.0;
const UI2_CHART_WINDOW_Y: f32 = 100.0;
const UI2_CHART_WINDOW_Z: i16 = 39;
const UI2_CHART_WINDOW_ALPHA: u8 = 0xFF;

const BG_RGBA: [u8; 4] = [0x18, 0x1C, 0x24, 0xFF];
const AXIS_RGBA: [u8; 4] = [0x88, 0x98, 0xAA, 0xFF];
const TICK_RGBA: [u8; 4] = [0x55, 0x66, 0x77, 0xFF];
const GRID_RGBA: [u8; 4] = [0x28, 0x2E, 0x38, 0xFF];
const SINE_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const LABEL_RGBA: [u8; 4] = [0xAA, 0xBB, 0xCC, 0xFF];

/// Smallest font tier.
const FONT_TIER: ui2::Ui2FontTier = ui2::Ui2FontTier::Third;
const FONT_SIZE_CASE: usize = FONT_TIER.size_case();

/// Margins (pixels) reserved for axis labels.
const MARGIN_LEFT: usize = 40;
const MARGIN_BOTTOM: usize = 20;
const MARGIN_TOP: usize = 8;
const MARGIN_RIGHT: usize = 8;

// ---------------------------------------------------------------------------
// Helpers.
// ---------------------------------------------------------------------------

fn font_line_height() -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(FONT_TIER).max(1))
}

fn measure_text(text: &str) -> usize {
    ui2::ui2_font_measure_text(FONT_TIER, text).width_px.max(1) as usize
}

fn fill_rect(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let ey = y.saturating_add(h).min(dst_h);
    let ex = x.saturating_add(w).min(dst_w);
    for row in y.min(dst_h)..ey {
        for col in x.min(dst_w)..ex {
            let i = (row * dst_w + col) * 4;
            dst[i] = rgba[0];
            dst[i + 1] = rgba[1];
            dst[i + 2] = rgba[2];
            dst[i + 3] = rgba[3];
        }
    }
}

fn put_pixel(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize, rgba: [u8; 4]) {
    if x < dst_w && y < dst_h {
        let i = (y * dst_w + x) * 4;
        dst[i] = rgba[0];
        dst[i + 1] = rgba[1];
        dst[i + 2] = rgba[2];
        dst[i + 3] = rgba[3];
    }
}

fn render_text(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: usize,
    y: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let max_w = dst_w.saturating_sub(x);
    let _ = ui2::ui2_font_blit_text_rgba(
        dst, dst_w, dst_h, atlases, FONT_TIER, x, y, max_w, text, rgba,
    );
}

// ---------------------------------------------------------------------------
// Coordinate mapping.
// ---------------------------------------------------------------------------

/// Map a data-space value to pixel coordinate inside the plot area.
fn data_to_px_x(val: f64, plot_w: usize) -> f64 {
    let frac = (val - AXIS_X_MIN) / (AXIS_X_MAX - AXIS_X_MIN);
    frac * (plot_w as f64)
}

fn data_to_px_y(val: f64, plot_h: usize) -> f64 {
    // Y is inverted: low data values are at the bottom (high pixel y).
    let frac = (val - AXIS_Y_MIN) / (AXIS_Y_MAX - AXIS_Y_MIN);
    (1.0 - frac) * (plot_h as f64)
}

// ---------------------------------------------------------------------------
// Compose the chart RGBA buffer.
// ---------------------------------------------------------------------------

fn compose_chart(atlases: &ui2::Ui2FontCpuAtlases, w: u32, h: u32) -> Vec<u8> {
    let dst_w = w as usize;
    let dst_h = h as usize;
    let mut buf = vec![0u8; dst_w * dst_h * 4];

    // Background.
    fill_rect(&mut buf, dst_w, dst_h, 0, 0, dst_w, dst_h, BG_RGBA);

    let plot_x0 = MARGIN_LEFT;
    let plot_y0 = MARGIN_TOP;
    let plot_w = dst_w.saturating_sub(MARGIN_LEFT + MARGIN_RIGHT).max(1);
    let plot_h = dst_h.saturating_sub(MARGIN_TOP + MARGIN_BOTTOM).max(1);

    let lh = font_line_height();

    // --- Grid lines at each tick (skip 0) ---
    {
        let mut v = AXIS_X_MIN;
        while v <= AXIS_X_MAX + TICK_STEP * 0.01 {
            if libm::fabs(v) > 1e-9 {
                let px = data_to_px_x(v, plot_w);
                let ix = plot_x0 + (px as usize).min(plot_w);
                if ix > plot_x0 && ix < plot_x0 + plot_w {
                    for row in plot_y0..plot_y0 + plot_h {
                        put_pixel(&mut buf, dst_w, dst_h, ix, row, GRID_RGBA);
                    }
                }
            }
            v += TICK_STEP;
        }
        let mut v = AXIS_Y_MIN;
        while v <= AXIS_Y_MAX + TICK_STEP * 0.01 {
            if libm::fabs(v) > 1e-9 {
                let py = data_to_px_y(v, plot_h);
                let iy = plot_y0 + (py as usize).min(plot_h);
                if iy > plot_y0 && iy < plot_y0 + plot_h {
                    for col in plot_x0..plot_x0 + plot_w {
                        put_pixel(&mut buf, dst_w, dst_h, col, iy, GRID_RGBA);
                    }
                }
            }
            v += TICK_STEP;
        }
    }

    // --- Axes (at data 0,0 if visible) ---
    {
        // X-axis (horizontal line at y=0)
        let y0_px = data_to_px_y(0.0, plot_h);
        let iy0 = plot_y0 + (y0_px as usize).min(plot_h);
        if iy0 >= plot_y0 && iy0 < plot_y0 + plot_h {
            for col in plot_x0..plot_x0 + plot_w {
                put_pixel(&mut buf, dst_w, dst_h, col, iy0, AXIS_RGBA);
            }
        }
        // Y-axis (vertical line at x=0)
        let x0_px = data_to_px_x(0.0, plot_w);
        let ix0 = plot_x0 + (x0_px as usize).min(plot_w);
        if ix0 >= plot_x0 && ix0 < plot_x0 + plot_w {
            for row in plot_y0..plot_y0 + plot_h {
                put_pixel(&mut buf, dst_w, dst_h, ix0, row, AXIS_RGBA);
            }
        }
    }

    // --- Tick marks + labels ---
    {
        // X-axis ticks (labels at bottom)
        let tick_len = 4usize;
        // Where the x-axis sits in pixel space
        let axis_y_px = plot_y0 + (data_to_px_y(0.0, plot_h) as usize).min(plot_h);
        let label_y = plot_y0 + plot_h + 2; // just below plot area

        let mut v = AXIS_X_MIN;
        while v <= AXIS_X_MAX + TICK_STEP * 0.01 {
            if libm::fabs(v) > 1e-9 {
                let px = data_to_px_x(v, plot_w);
                let ix = plot_x0 + (px as usize).min(plot_w);
                if ix > plot_x0 && ix < plot_x0 + plot_w {
                    // Tick mark
                    let ty = if axis_y_px >= plot_y0 && axis_y_px < plot_y0 + plot_h {
                        axis_y_px
                    } else {
                        plot_y0 + plot_h - 1
                    };
                    for dy in 0..tick_len {
                        put_pixel(&mut buf, dst_w, dst_h, ix, ty + dy, TICK_RGBA);
                    }
                    // Label
                    let label = format_tick(v);
                    let tw = measure_text(label.as_str());
                    let lx = ix.saturating_sub(tw / 2);
                    if label_y + lh <= dst_h {
                        render_text(
                            &mut buf,
                            dst_w,
                            dst_h,
                            atlases,
                            lx,
                            label_y,
                            label.as_str(),
                            LABEL_RGBA,
                        );
                    }
                }
            }
            v += TICK_STEP;
        }

        // Y-axis ticks (labels on left)
        let axis_x_px = plot_x0 + (data_to_px_x(0.0, plot_w) as usize).min(plot_w);

        let mut v = AXIS_Y_MIN;
        while v <= AXIS_Y_MAX + TICK_STEP * 0.01 {
            if libm::fabs(v) > 1e-9 {
                let py = data_to_px_y(v, plot_h);
                let iy = plot_y0 + (py as usize).min(plot_h);
                if iy >= plot_y0 && iy < plot_y0 + plot_h {
                    // Tick mark
                    let tx = if axis_x_px >= plot_x0 && axis_x_px < plot_x0 + plot_w {
                        axis_x_px.saturating_sub(tick_len)
                    } else {
                        plot_x0
                    };
                    for dx in 0..tick_len {
                        put_pixel(&mut buf, dst_w, dst_h, tx + dx, iy, TICK_RGBA);
                    }
                    // Label (right-aligned to the left of the plot area)
                    let label = format_tick(v);
                    let tw = measure_text(label.as_str());
                    let lx = plot_x0.saturating_sub(tw + 3);
                    let ly = iy.saturating_sub(lh / 2);
                    render_text(
                        &mut buf,
                        dst_w,
                        dst_h,
                        atlases,
                        lx,
                        ly,
                        label.as_str(),
                        LABEL_RGBA,
                    );
                }
            }
            v += TICK_STEP;
        }
    }

    // --- Sine wave ---
    {
        let mut prev_y: Option<usize> = None;
        for px_col in 0..plot_w {
            let data_x = AXIS_X_MIN + (px_col as f64 / plot_w as f64) * (AXIS_X_MAX - AXIS_X_MIN);
            let data_y = libm::sin(data_x * core::f64::consts::PI);
            let py = data_to_px_y(data_y, plot_h);
            let iy = (py as isize).clamp(0, plot_h as isize - 1) as usize;
            let screen_x = plot_x0 + px_col;
            let screen_y = plot_y0 + iy;

            // Fill vertical span between previous and current to avoid gaps.
            if let Some(prev) = prev_y {
                let from = prev.min(iy);
                let to = prev.max(iy);
                for y in from..=to {
                    put_pixel(&mut buf, dst_w, dst_h, screen_x, plot_y0 + y, SINE_RGBA);
                }
            } else {
                put_pixel(&mut buf, dst_w, dst_h, screen_x, screen_y, SINE_RGBA);
            }
            prev_y = Some(iy);
        }
    }

    buf
}

/// Format a tick value nicely: drop the decimal if it's integer, otherwise one decimal.
fn format_tick(v: f64) -> String {
    let rounded = libm::round(v * 10.0) / 10.0;
    if libm::fabs(rounded - libm::round(rounded)) < 1e-9 {
        format!("{}", rounded as i32)
    } else {
        format!("{:.1}", rounded)
    }
}

// ---------------------------------------------------------------------------
// Present helper.
// ---------------------------------------------------------------------------

fn present_chart(
    surface: &ui2::Ui2SurfaceWindow,
    atlases: &ui2::Ui2FontCpuAtlases,
    w: u32,
    h: u32,
) {
    let rgba = compose_chart(atlases, w, h);
    let _ = surface.bind_hosted_scroll_state(UI2_CHART_CONTENT_ID, w, h);
    let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        surface.tex_id(),
        w,
        h,
        rgba.as_slice(),
        surface.window_id(),
        "ui2-chart-present",
    );
}

// ---------------------------------------------------------------------------
// Task entry point.
// ---------------------------------------------------------------------------

#[embassy_executor::task]
pub async fn ui2_chart_demo_task() {
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(FONT_SIZE_CASE) else {
        return;
    };

    let content_w = UI2_CHART_VIEW_W;
    let content_h = UI2_CHART_VIEW_H;
    let Some(surface) = ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        UI2_CHART_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_CHART_WINDOW_X,
            y: UI2_CHART_WINDOW_Y,
            w: UI2_CHART_VIEW_W as f32,
            h: UI2_CHART_VIEW_H as f32,
        },
        UI2_CHART_WINDOW_Z,
        UI2_CHART_WINDOW_ALPHA,
        UI2_CHART_TEX_ID,
        true,
        content_w,
        content_h,
    ) else {
        crate::log!("ui2-chart: window creation failed\n");
        return;
    };

    // Scrollbars
    let _ = ui2::set_window_vertical_scrollbar_side(
        surface.window_id(),
        ui2::Ui2WindowVerticalScrollbarSide::Right,
    );
    let _ = ui2::set_window_horizontal_scrollbar_side(
        surface.window_id(),
        ui2::Ui2WindowHorizontalScrollbarSide::Bottom,
    );

    // Initial render.
    present_chart(&surface, &atlases, content_w, content_h);

    // Re-render on resize.
    let mut last_viewport = (content_w, content_h);
    loop {
        Timer::after(EmbassyDuration::from_millis(200)).await;
        let viewport = ui2::window_content_rect_by_id(surface.window_id())
            .map(|r| (r.w.max(1.0) as u32, r.h.max(1.0) as u32))
            .unwrap_or(last_viewport);
        if viewport != last_viewport {
            last_viewport = viewport;
            present_chart(&surface, &atlases, viewport.0, viewport.1);
        }
    }
}
