use alloc::{format, string::String, vec, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_RAPLE_TEX_ID: u32 = 4_722;
const UI2_RAPLE_CONTENT_ID: u32 = 49;
const UI2_RAPLE_WINDOW_TITLE: &str = "raple";
const UI2_RAPLE_VIEW_W: u32 = 440;
const UI2_RAPLE_VIEW_H: u32 = 272;
const UI2_RAPLE_WINDOW_X: f32 = 460.0;
const UI2_RAPLE_WINDOW_Y: f32 = 84.0;
const UI2_RAPLE_WINDOW_Z: i16 = 37;
const UI2_RAPLE_WINDOW_ALPHA: u8 = 0xFF;
const UI2_RAPLE_BG_RGBA: [u8; 4] = [0x11, 0x15, 0x1A, 0xFF];
const UI2_RAPLE_PANEL_RGBA: [u8; 4] = [0x18, 0x1D, 0x24, 0xFF];
const UI2_RAPLE_GRID_RGBA: [u8; 4] = [0x27, 0x2E, 0x38, 0xFF];
const UI2_RAPLE_TEXT_RGBA: [u8; 4] = [0xE8, 0xEF, 0xF7, 0xFF];
const UI2_RAPLE_DIM_RGBA: [u8; 4] = [0x91, 0xA0, 0xB1, 0xFF];
const UI2_RAPLE_ACCENT_RGBA: [u8; 4] = [0x84, 0xD1, 0x9B, 0xFF];
const UI2_RAPLE_WARN_RGBA: [u8; 4] = [0xF0, 0xB1, 0x66, 0xFF];
const UI2_RAPLE_HOT_RGBA: [u8; 4] = [0xFF, 0xB0, 0x57, 0xFF];
const UI2_RAPLE_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;
const UI2_RAPLE_FONT_SIZE_CASE: usize = UI2_RAPLE_FONT_TIER.size_case();
const UI2_RAPLE_HISTORY_SECS: usize = 180;
const UI2_RAPLE_CHART_H: usize = 112;
const UI2_RAPLE_PAD_X: usize = 10;
const UI2_RAPLE_PAD_Y: usize = 8;

#[derive(Clone, Copy)]
struct DomainDisplay {
    icon: char,
    label: &'static str,
    power_w: Option<f64>,
    joules: f64,
}

fn line_height() -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(UI2_RAPLE_FONT_TIER).max(1))
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
    let end_y = y.saturating_add(h).min(dst_h);
    let end_x = x.saturating_add(w).min(dst_w);
    for row in y.min(dst_h)..end_y {
        for col in x.min(dst_w)..end_x {
            let idx = (row * dst_w + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn put_pixel(dst: &mut [u8], dst_w: usize, dst_h: usize, x: usize, y: usize, rgba: [u8; 4]) {
    if x >= dst_w || y >= dst_h {
        return;
    }
    let idx = (y * dst_w + x) * 4;
    dst[idx] = rgba[0];
    dst[idx + 1] = rgba[1];
    dst[idx + 2] = rgba[2];
    dst[idx + 3] = rgba[3];
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
    let max_width_px = dst_w.saturating_sub(x);
    let _ = ui2::ui2_font_blit_text_rgba(
        dst,
        dst_w,
        dst_h,
        atlases,
        UI2_RAPLE_FONT_TIER,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

fn format_watts(v: Option<f64>) -> String {
    match v {
        Some(v) if v.is_finite() => format!("{:>5.1} W", v),
        _ => String::from("   --.- W"),
    }
}

fn format_joules(v: f64) -> String {
    if !v.is_finite() {
        return String::from("--");
    }
    if v >= 1000.0 {
        format!("{:.1} kJ", v / 1000.0)
    } else {
        format!("{:.1} J", v)
    }
}

fn chart_max_watts(history: &[f32]) -> f32 {
    let mut max_w = 1.0f32;
    for sample in history {
        if sample.is_finite() {
            max_w = max_w.max(*sample);
        }
    }
    max_w.max(10.0)
}

fn render_chart(
    dst: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    history: &[f32],
    x: usize,
    y: usize,
    w: usize,
    h: usize,
) {
    fill_rect(dst, dst_w, dst_h, x, y, w, h, UI2_RAPLE_PANEL_RGBA);

    if w < 8 || h < 8 {
        return;
    }

    for row in [0usize, h / 2, h.saturating_sub(1)] {
        for col in 0..w {
            put_pixel(dst, dst_w, dst_h, x + col, y + row, UI2_RAPLE_GRID_RGBA);
        }
    }

    for frac in [0usize, w / 3, (2 * w) / 3, w.saturating_sub(1)] {
        for row in 0..h {
            put_pixel(dst, dst_w, dst_h, x + frac, y + row, UI2_RAPLE_GRID_RGBA);
        }
    }

    let max_watts = chart_max_watts(history);
    let top = format!("{:.0}W", max_watts);
    let mid = format!("{:.0}W", max_watts * 0.5);
    render_text(dst, dst_w, dst_h, atlases, x + 4, y + 2, top.as_str(), UI2_RAPLE_DIM_RGBA);
    render_text(
        dst,
        dst_w,
        dst_h,
        atlases,
        x + 4,
        y + h / 2 + 1,
        mid.as_str(),
        UI2_RAPLE_DIM_RGBA,
    );
    render_text(
        dst,
        dst_w,
        dst_h,
        atlases,
        x + 2,
        y + h.saturating_sub(line_height() + 2),
        "-3m                      -2m                      -1m                      now",
        UI2_RAPLE_DIM_RGBA,
    );

    if history.len() < 2 {
        return;
    }

    let plot_top = y + line_height() + 6;
    let plot_bottom = y + h.saturating_sub(line_height() + 8);
    if plot_bottom <= plot_top + 2 {
        return;
    }
    let plot_h = plot_bottom - plot_top;
    let plot_w = w.saturating_sub(2);

    let mut prev_py = None::<usize>;
    for (idx, sample) in history.iter().enumerate() {
        let px = x + 1 + (idx * plot_w / history.len().max(1));
        let norm = if max_watts > 0.0 {
            (*sample / max_watts).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let py = plot_bottom.saturating_sub((norm * plot_h as f32) as usize);
        if let Some(prev) = prev_py {
            let from = prev.min(py);
            let to = prev.max(py);
            for yy in from..=to {
                put_pixel(dst, dst_w, dst_h, px, yy, UI2_RAPLE_HOT_RGBA);
            }
        } else {
            put_pixel(dst, dst_w, dst_h, px, py, UI2_RAPLE_HOT_RGBA);
        }
        prev_py = Some(py);
    }
}

fn domain_rows(
    current: &crate::rapl::RaplSnapshot,
    previous: Option<&crate::rapl::RaplSnapshot>,
) -> Option<[DomainDisplay; 5]> {
    let latest = current.latest?;
    let elapsed = previous
        .map(|prev| current.last_update_ms.saturating_sub(prev.last_update_ms) as f64 / 1000.0)
        .unwrap_or(0.0);
    let prev_probe = previous.and_then(|prev| prev.latest);

    let power_for = |cur: crate::rapl::RaplSample, prev: Option<crate::rapl::RaplSample>| {
        let Some(prev) = prev else {
            return None;
        };
        cur.average_power_watts_since(prev, latest.units, elapsed)
    };

    Some([
        DomainDisplay {
            icon: '\u{26A1}',
            label: "pkg",
            power_w: power_for(latest.package, prev_probe.map(|p| p.package)),
            joules: latest.package.joules,
        },
        DomainDisplay {
            icon: '\u{1F9E0}',
            label: "pp0",
            power_w: power_for(latest.core, prev_probe.map(|p| p.core)),
            joules: latest.core.joules,
        },
        DomainDisplay {
            icon: '\u{1F5A5}',
            label: "pp1",
            power_w: power_for(latest.graphics, prev_probe.map(|p| p.graphics)),
            joules: latest.graphics.joules,
        },
        DomainDisplay {
            icon: '\u{1F4BE}',
            label: "dram",
            power_w: power_for(latest.dram, prev_probe.map(|p| p.dram)),
            joules: latest.dram.joules,
        },
        DomainDisplay {
            icon: '\u{1F50C}',
            label: "psys",
            power_w: power_for(latest.platform, prev_probe.map(|p| p.platform)),
            joules: latest.platform.joules,
        },
    ])
}

fn compose_raple(
    atlases: &ui2::Ui2FontCpuAtlases,
    current: &crate::rapl::RaplSnapshot,
    previous: Option<&crate::rapl::RaplSnapshot>,
    history: &[f32],
) -> Vec<u8> {
    let dst_w = UI2_RAPLE_VIEW_W as usize;
    let dst_h = UI2_RAPLE_VIEW_H as usize;
    let mut buf = vec![0u8; dst_w * dst_h * 4];
    fill_rect(&mut buf, dst_w, dst_h, 0, 0, dst_w, dst_h, UI2_RAPLE_BG_RGBA);

    let lh = line_height();
    let mut y = UI2_RAPLE_PAD_Y;

    render_text(
        &mut buf,
        dst_w,
        dst_h,
        atlases,
        UI2_RAPLE_PAD_X,
        y,
        "⚡ raple live · 1s watch feed",
        UI2_RAPLE_TEXT_RGBA,
    );
    y += lh + 4;

    let status = if current.sample_valid {
        format!(
            "status online  seq={}  stamp={}ms",
            current.update_count, current.last_update_ms
        )
    } else if current.cpuid_supported {
        format!(
            "status waiting  seq={}  cpuid=intel+msr  probe=empty",
            current.update_count
        )
    } else {
        String::from("status unsupported  cpuid does not advertise intel+msr")
    };
    render_text(
        &mut buf,
        dst_w,
        dst_h,
        atlases,
        UI2_RAPLE_PAD_X,
        y,
        status.as_str(),
        if current.sample_valid {
            UI2_RAPLE_DIM_RGBA
        } else {
            UI2_RAPLE_WARN_RGBA
        },
    );
    y += lh + 6;

    let Some(rows) = domain_rows(current, previous) else {
        render_text(
            &mut buf,
            dst_w,
            dst_h,
            atlases,
            UI2_RAPLE_PAD_X,
            y,
            "RAPL snapshot not available yet. The window stays subscribed and fills in as soon as the service publishes data.",
            UI2_RAPLE_TEXT_RGBA,
        );
        return buf;
    };

    let label_x = UI2_RAPLE_PAD_X;
    let power_x = 92usize;
    let energy_x = 172usize;

    render_text(&mut buf, dst_w, dst_h, atlases, label_x, y, "domain", UI2_RAPLE_DIM_RGBA);
    render_text(&mut buf, dst_w, dst_h, atlases, power_x, y, "avg power", UI2_RAPLE_DIM_RGBA);
    render_text(&mut buf, dst_w, dst_h, atlases, energy_x, y, "energy", UI2_RAPLE_DIM_RGBA);
    y += lh + 3;

    for row in rows {
        let label = format!("{} {}", row.icon, row.label);
        let power = format_watts(row.power_w);
        let energy = format_joules(row.joules);
        render_text(
            &mut buf,
            dst_w,
            dst_h,
            atlases,
            label_x,
            y,
            label.as_str(),
            UI2_RAPLE_TEXT_RGBA,
        );
        render_text(
            &mut buf,
            dst_w,
            dst_h,
            atlases,
            power_x,
            y,
            power.as_str(),
            row.power_w
                .map(|w| if w >= 25.0 { UI2_RAPLE_HOT_RGBA } else { UI2_RAPLE_ACCENT_RGBA })
                .unwrap_or(UI2_RAPLE_DIM_RGBA),
        );
        render_text(
            &mut buf,
            dst_w,
            dst_h,
            atlases,
            energy_x,
            y,
            energy.as_str(),
            UI2_RAPLE_TEXT_RGBA,
        );
        y += lh + 2;
    }

    y += 4;
    render_text(
        &mut buf,
        dst_w,
        dst_h,
        atlases,
        UI2_RAPLE_PAD_X,
        y,
        "📈 package watts · rolling 3 minute view",
        UI2_RAPLE_TEXT_RGBA,
    );
    y += lh + 4;

    render_chart(
        &mut buf,
        dst_w,
        dst_h,
        atlases,
        history,
        UI2_RAPLE_PAD_X,
        y,
        dst_w.saturating_sub(UI2_RAPLE_PAD_X * 2),
        UI2_RAPLE_CHART_H.min(dst_h.saturating_sub(y + 2)),
    );

    buf
}

fn maybe_push_history(
    history: &mut Vec<f32>,
    previous: &crate::rapl::RaplSnapshot,
    current: &crate::rapl::RaplSnapshot,
) {
    let Some(cur) = current.latest else {
        return;
    };
    let Some(prev) = previous.latest else {
        return;
    };
    let elapsed = current.last_update_ms.saturating_sub(previous.last_update_ms) as f64 / 1000.0;
    let Some(pkg_w) = cur.package.average_power_watts_since(prev.package, cur.units, elapsed) else {
        return;
    };
    if !pkg_w.is_finite() || pkg_w < 0.0 {
        return;
    }
    if history.len() >= UI2_RAPLE_HISTORY_SECS {
        history.remove(0);
    }
    history.push(pkg_w as f32);
}

fn present_raple(
    surface: &ui2::Ui2SurfaceWindow,
    atlases: &ui2::Ui2FontCpuAtlases,
    current: &crate::rapl::RaplSnapshot,
    previous: Option<&crate::rapl::RaplSnapshot>,
    history: &[f32],
) {
    let _ = surface.bind_hosted_scroll_state(UI2_RAPLE_CONTENT_ID, UI2_RAPLE_VIEW_W, UI2_RAPLE_VIEW_H);
    let rgba = compose_raple(atlases, current, previous, history);
    let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        surface.tex_id(),
        UI2_RAPLE_VIEW_W,
        UI2_RAPLE_VIEW_H,
        rgba.as_slice(),
        surface.window_id(),
        "ui2-raple-present",
    );
}

#[embassy_executor::task]
pub async fn ui2_raple_demo_task() {
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_RAPLE_FONT_SIZE_CASE) else {
        return;
    };

    let Some(surface) = ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        UI2_RAPLE_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_RAPLE_WINDOW_X,
            y: UI2_RAPLE_WINDOW_Y,
            w: UI2_RAPLE_VIEW_W as f32,
            h: UI2_RAPLE_VIEW_H as f32,
        },
        UI2_RAPLE_WINDOW_Z,
        UI2_RAPLE_WINDOW_ALPHA,
        UI2_RAPLE_TEX_ID,
        true,
        UI2_RAPLE_VIEW_W,
        UI2_RAPLE_VIEW_H,
    ) else {
        crate::log!("ui2-raple: window creation failed\n");
        return;
    };

    let window_id = surface.window_id();
    let _ = ui2::set_window_decorations(window_id, ui2::Ui2WindowDecorationMode::None);
    let _ = ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = ui2::set_window_bottom_scrollbar_visible(window_id, false);

    let mut history = Vec::with_capacity(UI2_RAPLE_HISTORY_SECS);
    let mut current = crate::rapl::latest_snapshot();
    let mut previous = None::<crate::rapl::RaplSnapshot>;
    present_raple(&surface, &atlases, &current, previous.as_ref(), history.as_slice());

    let mut receiver = crate::rapl::subscribe();
    loop {
        let next = if let Some(rx) = receiver.as_mut() {
            rx.changed().await
        } else {
            Timer::after(EmbassyDuration::from_secs(1)).await;
            crate::rapl::latest_snapshot()
        };

        if next.update_count != current.update_count {
            maybe_push_history(&mut history, &current, &next);
            previous = Some(current);
            current = next;
        }

        present_raple(&surface, &atlases, &current, previous.as_ref(), history.as_slice());
    }
}
