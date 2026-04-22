use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_CURRENCY_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Currency.get();
const UI2_CURRENCY_CONTENT_ID: u32 = crate::tst_ui2_ids::Ui2DemoContentId::Currency.get();
const UI2_CURRENCY_WINDOW_TITLE: &str = "Currency";
const UI2_CURRENCY_VIEW_W: u32 = 360;
const UI2_CURRENCY_VIEW_H: u32 = 180;
const UI2_CURRENCY_WINDOW_X: f32 = 210.0;
const UI2_CURRENCY_WINDOW_Y: f32 = 90.0;
const UI2_CURRENCY_WINDOW_Z: i16 = 39;
const UI2_CURRENCY_WINDOW_ALPHA: u8 = 0xFF;

const UI2_CURRENCY_BG_RGBA: [u8; 4] = [0x18, 0x1B, 0x22, 0xFF];
const UI2_CURRENCY_HEADER_BG_RGBA: [u8; 4] = [0x21, 0x26, 0x31, 0xFF];
const UI2_CURRENCY_ROW_BG_RGBA: [u8; 4] = [0x1C, 0x21, 0x2A, 0xFF];
const UI2_CURRENCY_TEXT_RGBA: [u8; 4] = [0xEC, 0xF2, 0xF8, 0xFF];
const UI2_CURRENCY_DIM_RGBA: [u8; 4] = [0x94, 0xA2, 0xB3, 0xFF];
const UI2_CURRENCY_ACCENT_RGBA: [u8; 4] = [0x79, 0xCF, 0xB0, 0xFF];

const UI2_CURRENCY_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_CURRENCY_FONT_SIZE_CASE: usize = UI2_CURRENCY_FONT_TIER.size_case();
const UI2_CURRENCY_PAD_X: usize = 10;
const UI2_CURRENCY_PAD_Y: usize = 8;
const UI2_CURRENCY_ROW_GAP_Y: usize = 4;

const FXFEED_URL: &str = "https://api.fxfeed.io/v2/latest?base=USD&currencies=EUR,GBP,JPY&api_key=fxf_SwF1T46MmH8uCkOO7tOc";

#[derive(Clone, Debug)]
struct CurrencyRow {
    pair: String,
    value: String,
}

#[derive(Clone, Debug)]
struct CurrencySnapshot {
    header: String,
    subheader: String,
    rows: Vec<CurrencyRow>,
    footer: String,
}

fn currency_line_height() -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(UI2_CURRENCY_FONT_TIER).max(1))
}

fn currency_measure_width(text: &str) -> usize {
    ui2::ui2_font_measure_text(UI2_CURRENCY_FONT_TIER, text)
        .width_px
        .max(1) as usize
}

fn fill_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let end_y = y.saturating_add(h).min(dst_height);
    let end_x = x.saturating_add(w).min(dst_width);
    for row in y.min(dst_height)..end_y {
        for col in x.min(dst_width)..end_x {
            let idx = (row * dst_width + col) * 4;
            dst[idx] = rgba[0];
            dst[idx + 1] = rgba[1];
            dst[idx + 2] = rgba[2];
            dst[idx + 3] = rgba[3];
        }
    }
}

fn render_text_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: usize,
    y: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let max_width_px = dst_width.saturating_sub(x);
    let _ = ui2::ui2_font_blit_text_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        UI2_CURRENCY_FONT_TIER,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

fn parse_rate(raw: &str, code: &str) -> Option<f64> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    let success = root.get("success")?.as_bool()?;
    if !success {
        return None;
    }
    root.get("rates")?.get(code)?.as_f64()
}

fn parse_string_field(raw: &str, key: &str) -> Option<String> {
    let root: serde_json::Value = serde_json::from_str(raw).ok()?;
    root.get(key)?.as_str().map(ToString::to_string)
}

fn build_loading_snapshot() -> CurrencySnapshot {
    CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: "USD base  |  EUR GBP JPY".to_string(),
        rows: Vec::new(),
        footer: "Loading FXFeed...".to_string(),
    }
}

fn build_error_snapshot(message: &str) -> CurrencySnapshot {
    CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: "USD base  |  EUR GBP JPY".to_string(),
        rows: Vec::new(),
        footer: message.to_string(),
    }
}

fn build_currency_snapshot(raw: &str) -> Option<CurrencySnapshot> {
    let base = parse_string_field(raw, "base")?;
    let date = parse_string_field(raw, "date")?;
    let eur = parse_rate(raw, "EUR")?;
    let gbp = parse_rate(raw, "GBP")?;
    let jpy = parse_rate(raw, "JPY")?;

    Some(CurrencySnapshot {
        header: "Currency Converter".to_string(),
        subheader: format!("1 {} =", base),
        rows: vec![
            CurrencyRow {
                pair: "EUR".to_string(),
                value: format!("{:.6}", eur),
            },
            CurrencyRow {
                pair: "GBP".to_string(),
                value: format!("{:.6}", gbp),
            },
            CurrencyRow {
                pair: "JPY".to_string(),
                value: format!("{:.4}", jpy),
            },
        ],
        footer: format!("Updated {}", date),
    })
}

fn currency_content_size(snapshot: &CurrencySnapshot) -> (u32, u32) {
    let line_height = currency_line_height();
    let line_step = line_height.saturating_add(UI2_CURRENCY_ROW_GAP_Y);

    let mut max_width = currency_measure_width(snapshot.header.as_str());
    max_width = max_width.max(currency_measure_width(snapshot.subheader.as_str()));
    max_width = max_width.max(currency_measure_width(snapshot.footer.as_str()));
    for row in snapshot.rows.iter() {
        let line = format!("{}  {}", row.pair, row.value);
        max_width = max_width.max(currency_measure_width(line.as_str()));
    }

    let total_lines = 2 + snapshot.rows.len().max(1) + 1;
    let content_w = max_width
        .saturating_add(UI2_CURRENCY_PAD_X * 2)
        .max(UI2_CURRENCY_VIEW_W as usize);
    let content_h = total_lines
        .saturating_mul(line_step)
        .saturating_add(UI2_CURRENCY_PAD_Y * 2)
        .max(UI2_CURRENCY_VIEW_H as usize);
    (content_w as u32, content_h as u32)
}

fn compose_currency_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &CurrencySnapshot,
    content_w: u32,
    content_h: u32,
) -> Vec<u8> {
    let dst_width = content_w as usize;
    let dst_height = content_h as usize;
    let mut rgba = vec![0u8; dst_width.saturating_mul(dst_height).saturating_mul(4)];

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        dst_height,
        UI2_CURRENCY_BG_RGBA,
    );

    let line_height = currency_line_height();
    let line_step = line_height.saturating_add(UI2_CURRENCY_ROW_GAP_Y);

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        line_step.saturating_mul(2).saturating_add(UI2_CURRENCY_PAD_Y),
        UI2_CURRENCY_HEADER_BG_RGBA,
    );

    let mut y = UI2_CURRENCY_PAD_Y;
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.header.as_str(),
        UI2_CURRENCY_ACCENT_RGBA,
    );
    y = y.saturating_add(line_step);
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.subheader.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );
    y = y.saturating_add(line_step).saturating_add(2);

    if snapshot.rows.is_empty() {
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_CURRENCY_PAD_X,
            y,
            snapshot.footer.as_str(),
            UI2_CURRENCY_TEXT_RGBA,
        );
        return rgba;
    }

    for row in snapshot.rows.iter() {
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            UI2_CURRENCY_PAD_X.saturating_sub(4),
            y.saturating_sub(2),
            dst_width.saturating_sub(UI2_CURRENCY_PAD_X.saturating_sub(4) * 2),
            line_step.saturating_add(4),
            UI2_CURRENCY_ROW_BG_RGBA,
        );
        let line = format!("{}  {}", row.pair, row.value);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_CURRENCY_PAD_X,
            y,
            line.as_str(),
            UI2_CURRENCY_TEXT_RGBA,
        );
        y = y.saturating_add(line_step);
    }

    y = y.saturating_add(4);
    render_text_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_CURRENCY_PAD_X,
        y,
        snapshot.footer.as_str(),
        UI2_CURRENCY_DIM_RGBA,
    );

    rgba
}

fn present_snapshot(
    surface: &ui2::Ui2SurfaceWindow,
    atlases: &ui2::Ui2FontCpuAtlases,
    snapshot: &CurrencySnapshot,
) {
    let (content_w, content_h) = currency_content_size(snapshot);
    let rgba = compose_currency_rgba(atlases, snapshot, content_w, content_h);
    let _ = surface.bind_hosted_scroll_state(UI2_CURRENCY_CONTENT_ID, content_w, content_h);
    let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        surface.tex_id(),
        content_w,
        content_h,
        rgba.as_slice(),
        surface.window_id(),
        "ui2-currency-present",
    );
}

#[embassy_executor::task]
pub async fn ui2_currency_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-currency-demo");
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_CURRENCY_FONT_SIZE_CASE) else {
        return;
    };

    let Some(surface) = ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        UI2_CURRENCY_WINDOW_TITLE,
        Ui2Rect {
            x: UI2_CURRENCY_WINDOW_X,
            y: UI2_CURRENCY_WINDOW_Y,
            w: UI2_CURRENCY_VIEW_W as f32,
            h: UI2_CURRENCY_VIEW_H as f32,
        },
        UI2_CURRENCY_WINDOW_Z,
        UI2_CURRENCY_WINDOW_ALPHA,
        UI2_CURRENCY_TEX_ID,
        true,
        UI2_CURRENCY_VIEW_W,
        UI2_CURRENCY_VIEW_H,
    ) else {
        crate::log!("ui2-currency: window creation failed\n");
        return;
    };
    let _ = surface.bind_spawn_task("ui2-currency-demo");
    let _ = ui2::set_window_title_twemoji(surface.window_id(), '\u{1F4B1}');
    let _ = ui2::set_window_vertical_scrollbar_side(
        surface.window_id(),
        ui2::Ui2WindowVerticalScrollbarSide::Right,
    );

    let loading = build_loading_snapshot();
    present_snapshot(&surface, &atlases, &loading);

    let snapshot = match crate::r::net::json::get_json(FXFEED_URL).await {
        Ok(raw) => build_currency_snapshot(raw.as_str())
            .unwrap_or_else(|| build_error_snapshot("FXFeed parse failed")),
        Err(e) => {
            crate::log!("ui2-currency: request failed: {:?}\n", e);
            build_error_snapshot("FXFeed request failed")
        }
    };
    present_snapshot(&surface, &atlases, &snapshot);

    loop {
        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-currency-demo", 200).await {
            break;
        }
        let _ = ui2::window_content_rect_by_id(surface.window_id());
    }
}
