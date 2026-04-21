use alloc::{format, string::String, vec, vec::Vec};

use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect};

const UI2_SWARM_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Swarm.get();
const UI2_SWARM_CONTENT_ID: u32 = crate::tst_ui2_ids::Ui2DemoContentId::Swarm.get();
const UI2_SWARM_VIEW_W: u32 = 820;
const UI2_SWARM_VIEW_H: u32 = 560;
const UI2_SWARM_WINDOW_X: f32 = 420.0;
const UI2_SWARM_WINDOW_Y: f32 = 72.0;
const UI2_SWARM_WINDOW_Z: i16 = 37;
const UI2_SWARM_WINDOW_ALPHA: u8 = 255;
const UI2_SWARM_BG_RGBA: [u8; 4] = [0x11, 0x16, 0x1C, 0xFF];
const UI2_SWARM_PANEL_RGBA: [u8; 4] = [0x18, 0x20, 0x2A, 0xFF];
const UI2_SWARM_PANEL_SELECTED_RGBA: [u8; 4] = [0x1D, 0x2C, 0x39, 0xFF];
const UI2_SWARM_PANEL_BORDER_RGBA: [u8; 4] = [0x2E, 0x42, 0x55, 0xFF];
const UI2_SWARM_ACCENT_RGBA: [u8; 4] = [0x73, 0xD5, 0xA5, 0xFF];
const UI2_SWARM_TEXT_RGBA: [u8; 4] = [0xED, 0xF2, 0xF7, 0xFF];
const UI2_SWARM_DIM_RGBA: [u8; 4] = [0x94, 0xA4, 0xB7, 0xFF];
const UI2_SWARM_WARN_RGBA: [u8; 4] = [0xF3, 0xBE, 0x74, 0xFF];
const UI2_SWARM_ERROR_RGBA: [u8; 4] = [0xF2, 0x8D, 0x8D, 0xFF];
const UI2_SWARM_OK_RGBA: [u8; 4] = [0x7E, 0xD9, 0xB2, 0xFF];
const UI2_SWARM_BODY_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;
const UI2_SWARM_BODY_FONT_SIZE_CASE: usize = UI2_SWARM_BODY_FONT_TIER.size_case();
const UI2_SWARM_EMPH_FONT_TIER: Ui2FontTier = Ui2FontTier::Half;
const UI2_SWARM_EMPH_FONT_SIZE_CASE: usize = UI2_SWARM_EMPH_FONT_TIER.size_case();
const UI2_SWARM_PAD: usize = 12;
const UI2_SWARM_TILE_W: usize = 184;
const UI2_SWARM_TILE_H: usize = 58;
const UI2_SWARM_TILE_GAP: usize = 10;
const UI2_SWARM_HEADER_H: usize = 96;
const UI2_SWARM_ITEM_RESTART: u32 = 1;
const UI2_SWARM_ITEM_NEXT_SKETCH: u32 = 2;
const UI2_SWARM_ITEM_UPLOAD: u32 = 3;
const UI2_SWARM_TILE_ITEM_BASE: u32 = 1_000;
const UI2_SWARM_TARGET_FILENAME: &str = "app.py";
const UI2_SWARM_BUTTON_W: usize = 64;
const UI2_SWARM_BUTTON_H: usize = 28;
const UI2_SWARM_BUTTON_GAP: usize = 8;
const UI2_SWARM_LABEL_RESTART: &str = "rst";
const UI2_SWARM_LABEL_NEXT: &str = "next";
const UI2_SWARM_LABEL_UPLOAD: &str = "up";

struct SketchSpec {
    source_name: &'static str,
    body: &'static [u8],
}

const SWARM_SKETCHES: &[SketchSpec] = &[
    SketchSpec {
        source_name: "led.py",
        body: include_bytes!("../../crates/trueos-esp/iot/led.py"),
    },
    SketchSpec {
        source_name: "led2.py",
        body: include_bytes!("../../crates/trueos-esp/iot/led2.py"),
    },
    SketchSpec {
        source_name: "led4.py",
        body: include_bytes!("../../crates/trueos-esp/iot/led4.py"),
    },
    SketchSpec {
        source_name: "led_pinkfade.py",
        body: include_bytes!("../../crates/trueos-esp/iot/led_pinkfade.py"),
    },
    SketchSpec {
        source_name: "1OktavePianoLed.py",
        body: include_bytes!("../../crates/trueos-esp/iot/1OktavePianoLed.py"),
    },
];

#[derive(Clone)]
struct SwarmScene {
    title: String,
    selected_handle: Option<v::vnet::NetHandle>,
    selected_lines: Vec<String>,
    action_text: String,
    sketch_name: &'static str,
    sketches_total: usize,
    sketch_index: usize,
    devices: Vec<trueos_esp::gate::DeviceSnapshot>,
}

fn swarm_line_height(tier: Ui2FontTier) -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(tier).max(1))
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

fn stroke_rect_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    rect: Ui2Rect,
    rgba: [u8; 4],
) {
    let x = rect.x.max(0.0) as usize;
    let y = rect.y.max(0.0) as usize;
    let w = rect.w.max(0.0) as usize;
    let h = rect.h.max(0.0) as usize;
    if w == 0 || h == 0 {
        return;
    }
    fill_rect_rgba(dst, dst_width, dst_height, x, y, w, 1, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x,
        y.saturating_add(h.saturating_sub(1)),
        w,
        1,
        rgba,
    );
    fill_rect_rgba(dst, dst_width, dst_height, x, y, 1, h, rgba);
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        x.saturating_add(w.saturating_sub(1)),
        y,
        1,
        h,
        rgba,
    );
}

fn render_text_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    tier: Ui2FontTier,
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
        tier,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

fn elide_text_to_width(tier: Ui2FontTier, max_width_px: usize, text: &str) -> String {
    if ui2::ui2_font_measure_text(tier, text).width_px as usize <= max_width_px {
        return String::from(text);
    }

    let ellipsis = "...";
    let ellipsis_w = ui2::ui2_font_measure_text(tier, ellipsis).width_px as usize;
    if ellipsis_w >= max_width_px {
        return String::from(ellipsis);
    }

    let mut out = String::new();
    for ch in text.chars() {
        let mut candidate = out.clone();
        candidate.push(ch);
        candidate.push_str(ellipsis);
        if ui2::ui2_font_measure_text(tier, candidate.as_str()).width_px as usize > max_width_px {
            break;
        }
        out.push(ch);
    }
    out.push_str(ellipsis);
    out
}

fn button_rect(x: usize, y: usize, w: usize) -> Ui2Rect {
    Ui2Rect {
        x: x as f32,
        y: y as f32,
        w: w as f32,
        h: UI2_SWARM_BUTTON_H as f32,
    }
}

fn draw_button(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    rect: Ui2Rect,
    label: &str,
    accent: [u8; 4],
) {
    let label_w = ui2::ui2_font_measure_text(UI2_SWARM_EMPH_FONT_TIER, label).width_px as usize;
    let label_x =
        rect.x.max(0.0) as usize + ((rect.w.max(0.0) as usize).saturating_sub(label_w) / 2);
    let label_h = swarm_line_height(UI2_SWARM_EMPH_FONT_TIER);
    let label_y =
        rect.y.max(0.0) as usize + ((rect.h.max(0.0) as usize).saturating_sub(label_h) / 2);
    render_text_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        UI2_SWARM_EMPH_FONT_TIER,
        label_x,
        label_y,
        label,
        accent,
    );
}

fn device_endpoint_text(snapshot: &trueos_esp::gate::DeviceSnapshot) -> String {
    match snapshot.ip {
        Some(trueos_esp::gate::DeviceIp::V4(addr)) => {
            format!("{}.{}.{}.{}:{}", addr[0], addr[1], addr[2], addr[3], snapshot.service_port)
        }
        Some(trueos_esp::gate::DeviceIp::V6(addr)) => format!(
            "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{}",
            u16::from_be_bytes([addr[0], addr[1]]),
            u16::from_be_bytes([addr[2], addr[3]]),
            u16::from_be_bytes([addr[4], addr[5]]),
            u16::from_be_bytes([addr[6], addr[7]]),
            u16::from_be_bytes([addr[8], addr[9]]),
            u16::from_be_bytes([addr[10], addr[11]]),
            u16::from_be_bytes([addr[12], addr[13]]),
            u16::from_be_bytes([addr[14], addr[15]]),
            snapshot.service_port
        ),
        None => format!("pending:{}", snapshot.service_port),
    }
}

fn selected_device_lines(snapshot: Option<&trueos_esp::gate::DeviceSnapshot>) -> Vec<String> {
    let mut lines = Vec::new();
    let Some(snapshot) = snapshot else {
        lines.push(String::from("selected: none"));
        lines.push(String::from("waiting for device"));
        return lines;
    };

    lines.push(format!("selected={}  {}", snapshot.handle.0, snapshot.tag.as_str()));
    lines.push(format!("endpoint={}", device_endpoint_text(snapshot)));

    if let Some(status) = snapshot.status.as_ref() {
        lines.push(format!(
            "run={} app={} hb={} {}",
            if status.running { "y" } else { "n" },
            if status.app_exists { "y" } else { "n" },
            status.heartbeat_count,
            status.last_status.as_str()
        ));
    } else {
        lines.push(String::from("status=pending"));
    }

    lines
}

fn build_scene(
    devices: &[trueos_esp::gate::DeviceSnapshot],
    selected_handle: Option<v::vnet::NetHandle>,
    sketch_index: usize,
    action_text: &str,
) -> SwarmScene {
    let selected_snapshot = selected_handle
        .and_then(|handle| devices.iter().find(|snapshot| snapshot.handle == handle));

    SwarmScene {
        title: format!("ESP32 swarm  [{} devices]", devices.len()),
        selected_handle,
        selected_lines: selected_device_lines(selected_snapshot),
        action_text: String::from(action_text),
        sketch_name: SWARM_SKETCHES
            .get(sketch_index)
            .map(|entry| entry.source_name)
            .unwrap_or("missing"),
        sketches_total: SWARM_SKETCHES.len(),
        sketch_index,
        devices: devices.to_vec(),
    }
}

fn tile_color(snapshot: &trueos_esp::gate::DeviceSnapshot, selected: bool) -> ([u8; 4], [u8; 4]) {
    let bg = if selected {
        UI2_SWARM_PANEL_SELECTED_RGBA
    } else {
        UI2_SWARM_PANEL_RGBA
    };
    let border = match snapshot.status.as_ref() {
        Some(status) if status.running => UI2_SWARM_OK_RGBA,
        Some(status) if status.app_exists => UI2_SWARM_WARN_RGBA,
        Some(_) => UI2_SWARM_PANEL_BORDER_RGBA,
        None => UI2_SWARM_DIM_RGBA,
    };
    (bg, border)
}

fn render_scene(
    surface: &crate::r::ui2::Ui2SurfaceWindow,
    viewport_w: u32,
    viewport_h: u32,
    body_atlases: &ui2::Ui2FontCpuAtlases,
    emph_atlases: &ui2::Ui2FontCpuAtlases,
    scene: &SwarmScene,
) -> (Vec<u8>, Vec<Ui2HostedInteractiveRect>, u32, u32) {
    let width = viewport_w.max(UI2_SWARM_VIEW_W) as usize;
    let columns = ((width.saturating_sub(UI2_SWARM_PAD * 2) + UI2_SWARM_TILE_GAP)
        / (UI2_SWARM_TILE_W + UI2_SWARM_TILE_GAP))
        .max(1);
    let rows = ((scene
        .devices
        .len()
        .saturating_add(columns.saturating_sub(1)))
        / columns)
        .max(1);
    let content_h = (UI2_SWARM_HEADER_H
        + UI2_SWARM_PAD
        + rows * (UI2_SWARM_TILE_H + UI2_SWARM_TILE_GAP)
        + UI2_SWARM_PAD) as u32;
    let height = viewport_h.max(content_h).max(UI2_SWARM_VIEW_H) as usize;

    let mut pixels = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(UI2_SWARM_BG_RGBA.as_slice());
    }

    fill_rect_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        UI2_SWARM_PAD,
        UI2_SWARM_PAD,
        width.saturating_sub(UI2_SWARM_PAD * 2),
        UI2_SWARM_HEADER_H,
        UI2_SWARM_PANEL_RGBA,
    );
    stroke_rect_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        Ui2Rect {
            x: UI2_SWARM_PAD as f32,
            y: UI2_SWARM_PAD as f32,
            w: width.saturating_sub(UI2_SWARM_PAD * 2) as f32,
            h: UI2_SWARM_HEADER_H as f32,
        },
        UI2_SWARM_PANEL_BORDER_RGBA,
    );

    render_text_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        emph_atlases,
        UI2_SWARM_EMPH_FONT_TIER,
        UI2_SWARM_PAD + 8,
        UI2_SWARM_PAD + 8,
        scene.title.as_str(),
        UI2_SWARM_ACCENT_RGBA,
    );
    let emph_h = swarm_line_height(UI2_SWARM_EMPH_FONT_TIER);
    let body_h = swarm_line_height(UI2_SWARM_BODY_FONT_TIER);
    let title_y = UI2_SWARM_PAD + 8;
    let subtitle_y = title_y + emph_h + 4;
    let detail_y = subtitle_y + body_h + 6;
    let right_col_w = 236usize.min(width.saturating_sub(UI2_SWARM_PAD * 2 + 140));
    let right_col_x = width.saturating_sub(UI2_SWARM_PAD + right_col_w);
    let left_col_x = UI2_SWARM_PAD + 8;
    let left_col_w = right_col_x.saturating_sub(left_col_x + 12);
    render_text_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        body_atlases,
        UI2_SWARM_BODY_FONT_TIER,
        left_col_x,
        subtitle_y,
        "manual controls only",
        UI2_SWARM_DIM_RGBA,
    );

    let mut y = detail_y;
    for line in scene.selected_lines.iter().take(2) {
        let text = elide_text_to_width(UI2_SWARM_BODY_FONT_TIER, left_col_w, line.as_str());
        render_text_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            body_atlases,
            UI2_SWARM_BODY_FONT_TIER,
            left_col_x,
            y,
            text.as_str(),
            UI2_SWARM_TEXT_RGBA,
        );
        y = y.saturating_add(swarm_line_height(UI2_SWARM_BODY_FONT_TIER) + 1);
    }

    let button_y = title_y;
    let button_total_w = UI2_SWARM_BUTTON_W * 3 + UI2_SWARM_BUTTON_GAP * 2;
    let button_row_x = right_col_x + right_col_w.saturating_sub(button_total_w) / 2;
    let restart_rect = button_rect(button_row_x, button_y, UI2_SWARM_BUTTON_W);
    let next_rect = button_rect(
        button_row_x + UI2_SWARM_BUTTON_W + UI2_SWARM_BUTTON_GAP,
        button_y,
        UI2_SWARM_BUTTON_W,
    );
    let upload_rect = button_rect(
        button_row_x + (UI2_SWARM_BUTTON_W + UI2_SWARM_BUTTON_GAP) * 2,
        button_y,
        UI2_SWARM_BUTTON_W,
    );
    draw_button(
        pixels.as_mut_slice(),
        width,
        height,
        emph_atlases,
        restart_rect,
        UI2_SWARM_LABEL_RESTART,
        UI2_SWARM_ERROR_RGBA,
    );
    draw_button(
        pixels.as_mut_slice(),
        width,
        height,
        emph_atlases,
        next_rect,
        UI2_SWARM_LABEL_NEXT,
        UI2_SWARM_WARN_RGBA,
    );
    draw_button(
        pixels.as_mut_slice(),
        width,
        height,
        emph_atlases,
        upload_rect,
        UI2_SWARM_LABEL_UPLOAD,
        UI2_SWARM_OK_RGBA,
    );

    let sketch_text = format!(
        "sketch {}/{}: {}",
        scene.sketch_index.saturating_add(1),
        scene.sketches_total,
        scene.sketch_name
    );
    let action_width = right_col_w;
    let action_y = button_y + UI2_SWARM_BUTTON_H + 6;
    render_text_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        body_atlases,
        UI2_SWARM_BODY_FONT_TIER,
        right_col_x,
        action_y,
        elide_text_to_width(UI2_SWARM_BODY_FONT_TIER, action_width, sketch_text.as_str()).as_str(),
        UI2_SWARM_TEXT_RGBA,
    );

    let action_color = if scene.action_text.contains("failed") {
        UI2_SWARM_ERROR_RGBA
    } else if scene.action_text.contains("uploaded")
        || scene.action_text.contains("restart")
        || scene.action_text.contains("Restart")
        || scene.action_text.contains("removed")
    {
        UI2_SWARM_OK_RGBA
    } else {
        UI2_SWARM_DIM_RGBA
    };
    render_text_rgba(
        pixels.as_mut_slice(),
        width,
        height,
        body_atlases,
        UI2_SWARM_BODY_FONT_TIER,
        right_col_x,
        action_y + body_h + 2,
        elide_text_to_width(UI2_SWARM_BODY_FONT_TIER, action_width, scene.action_text.as_str())
            .as_str(),
        action_color,
    );

    let mut interactives = Vec::new();
    interactives.push(Ui2HostedInteractiveRect {
        item_id: UI2_SWARM_ITEM_RESTART,
        x: restart_rect.x.max(0.0) as u32,
        y: restart_rect.y.max(0.0) as u32,
        width: restart_rect.w.max(0.0) as u32,
        height: restart_rect.h.max(0.0) as u32,
    });
    interactives.push(Ui2HostedInteractiveRect {
        item_id: UI2_SWARM_ITEM_NEXT_SKETCH,
        x: next_rect.x.max(0.0) as u32,
        y: next_rect.y.max(0.0) as u32,
        width: next_rect.w.max(0.0) as u32,
        height: next_rect.h.max(0.0) as u32,
    });
    interactives.push(Ui2HostedInteractiveRect {
        item_id: UI2_SWARM_ITEM_UPLOAD,
        x: upload_rect.x.max(0.0) as u32,
        y: upload_rect.y.max(0.0) as u32,
        width: upload_rect.w.max(0.0) as u32,
        height: upload_rect.h.max(0.0) as u32,
    });

    let tile_origin_y = UI2_SWARM_PAD + UI2_SWARM_HEADER_H + UI2_SWARM_PAD;
    for (index, snapshot) in scene.devices.iter().enumerate() {
        let row = index / columns;
        let col = index % columns;
        let tile_x = UI2_SWARM_PAD + col * (UI2_SWARM_TILE_W + UI2_SWARM_TILE_GAP);
        let tile_y = tile_origin_y + row * (UI2_SWARM_TILE_H + UI2_SWARM_TILE_GAP);
        let rect = Ui2Rect {
            x: tile_x as f32,
            y: tile_y as f32,
            w: UI2_SWARM_TILE_W as f32,
            h: UI2_SWARM_TILE_H as f32,
        };
        let selected = scene.selected_handle == Some(snapshot.handle);
        let (bg, border) = tile_color(snapshot, selected);
        fill_rect_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            tile_x,
            tile_y,
            UI2_SWARM_TILE_W,
            UI2_SWARM_TILE_H,
            bg,
        );
        stroke_rect_rgba(pixels.as_mut_slice(), width, height, rect, border);

        let line0 = format!("ESP {}", snapshot.handle.0);
        let line1 = elide_text_to_width(
            UI2_SWARM_BODY_FONT_TIER,
            UI2_SWARM_TILE_W.saturating_sub(16),
            device_endpoint_text(snapshot).as_str(),
        );
        let line2 = if let Some(status) = snapshot.status.as_ref() {
            format!(
                "run={} app={} hb={}",
                if status.running { "y" } else { "n" },
                if status.app_exists { "y" } else { "n" },
                status.heartbeat_count
            )
        } else {
            String::from("status=pending")
        };
        let title_color = if selected {
            UI2_SWARM_ACCENT_RGBA
        } else {
            UI2_SWARM_TEXT_RGBA
        };
        let tile_title_y = tile_y + 6;
        let tile_line1_y = tile_title_y + emph_h;
        let tile_line2_y = tile_line1_y + body_h + 2;

        render_text_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            emph_atlases,
            UI2_SWARM_EMPH_FONT_TIER,
            tile_x + 8,
            tile_title_y,
            line0.as_str(),
            title_color,
        );
        render_text_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            body_atlases,
            UI2_SWARM_BODY_FONT_TIER,
            tile_x + 8,
            tile_line1_y,
            line1.as_str(),
            UI2_SWARM_TEXT_RGBA,
        );
        render_text_rgba(
            pixels.as_mut_slice(),
            width,
            height,
            body_atlases,
            UI2_SWARM_BODY_FONT_TIER,
            tile_x + 8,
            tile_line2_y,
            line2.as_str(),
            UI2_SWARM_DIM_RGBA,
        );

        interactives.push(Ui2HostedInteractiveRect {
            item_id: UI2_SWARM_TILE_ITEM_BASE.saturating_add(index as u32),
            x: tile_x as u32,
            y: tile_y as u32,
            width: UI2_SWARM_TILE_W as u32,
            height: UI2_SWARM_TILE_H as u32,
        });
    }

    let _ = surface.bind_hosted_scroll_state(UI2_SWARM_CONTENT_ID, width as u32, content_h);
    (pixels, interactives, width as u32, content_h)
}

fn select_default_device(
    devices: &[trueos_esp::gate::DeviceSnapshot],
    selected_handle: Option<v::vnet::NetHandle>,
) -> Option<v::vnet::NetHandle> {
    if let Some(handle) = selected_handle {
        if devices.iter().any(|snapshot| snapshot.handle == handle) {
            return Some(handle);
        }
    }
    devices.first().map(|snapshot| snapshot.handle)
}

fn click_selected_device(item_id: u32, scene: &SwarmScene) -> Option<v::vnet::NetHandle> {
    let tile_index = item_id.checked_sub(UI2_SWARM_TILE_ITEM_BASE)? as usize;
    scene
        .devices
        .get(tile_index)
        .map(|snapshot| snapshot.handle)
}

#[embassy_executor::task]
pub async fn ui2_swarm_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-swarm-demo");
    let Some(body_atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_SWARM_BODY_FONT_SIZE_CASE) else {
        return;
    };
    let Some(emph_atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_SWARM_EMPH_FONT_SIZE_CASE) else {
        return;
    };

    let mut clear_pixels = vec![
        0u8;
        (UI2_SWARM_VIEW_W as usize)
            .saturating_mul(UI2_SWARM_VIEW_H as usize)
            .saturating_mul(4)
    ];
    for px in clear_pixels.chunks_exact_mut(4) {
        px.copy_from_slice(UI2_SWARM_BG_RGBA.as_slice());
    }
    if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        UI2_SWARM_TEX_ID,
        UI2_SWARM_VIEW_W,
        UI2_SWARM_VIEW_H,
        clear_pixels.as_slice(),
        0,
        "ui2-swarm-clear",
    ) {
        return;
    }

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        "ESP32 Swarm",
        Ui2Rect {
            x: UI2_SWARM_WINDOW_X,
            y: UI2_SWARM_WINDOW_Y,
            w: UI2_SWARM_VIEW_W as f32,
            h: UI2_SWARM_VIEW_H as f32,
        },
        UI2_SWARM_WINDOW_Z,
        UI2_SWARM_WINDOW_ALPHA,
        UI2_SWARM_TEX_ID,
        false,
        UI2_SWARM_VIEW_W,
        UI2_SWARM_VIEW_H,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-swarm-demo");

    let mut selected_handle: Option<v::vnet::NetHandle> = None;
    let mut sketch_index = 0usize;
    let mut action_text = String::from("waiting for devices");
    let mut last_registry_seq = 0u32;
    let mut last_viewport = (0u32, 0u32);
    let mut last_click_seq = 0u32;
    let mut needs_render = true;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-swarm-demo") {
            break;
        }
        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((UI2_SWARM_VIEW_W, UI2_SWARM_VIEW_H));
        if viewport != last_viewport {
            last_viewport = viewport;
            needs_render = true;
        }

        let registry_seq = crate::r::net::esp::registry_change_seq();
        let devices = crate::r::net::esp::device_snapshot();
        let next_selected = select_default_device(devices.as_slice(), selected_handle);
        if registry_seq != last_registry_seq || next_selected != selected_handle {
            last_registry_seq = registry_seq;
            selected_handle = next_selected;
            if devices.is_empty() {
                action_text = String::from("no devices discovered yet");
            }
            needs_render = true;
        }

        let mut scene = if needs_render {
            Some(build_scene(
                devices.as_slice(),
                selected_handle,
                sketch_index,
                action_text.as_str(),
            ))
        } else {
            None
        };

        if let Some((seq, item_id)) =
            crate::r::ui2::take_window_last_clicked_item(surface.window_id())
        {
            if seq != last_click_seq {
                last_click_seq = seq;
                let current_scene = match scene.take() {
                    Some(scene) => scene,
                    None => build_scene(
                        devices.as_slice(),
                        selected_handle,
                        sketch_index,
                        action_text.as_str(),
                    ),
                };

                match item_id {
                    UI2_SWARM_ITEM_RESTART => {
                        if let Some(handle) = selected_handle {
                            match crate::r::net::esp::restart_device(handle).await {
                                Ok(result) => {
                                    action_text = if result.restart_requested {
                                        String::from("restart requested via /restart")
                                    } else if result.removed_from_registry {
                                        String::from("removed locally; waiting for rediscovery")
                                    } else {
                                        String::from("restart request finished")
                                    };
                                }
                                Err(_) => {
                                    action_text = String::from("restart failed on /restart");
                                }
                            }
                        } else {
                            action_text = String::from("select a device first");
                        }
                        needs_render = true;
                    }
                    UI2_SWARM_ITEM_NEXT_SKETCH => {
                        if !SWARM_SKETCHES.is_empty() {
                            sketch_index = (sketch_index + 1) % SWARM_SKETCHES.len();
                            action_text = format!(
                                "sketch ready: {}",
                                SWARM_SKETCHES[sketch_index].source_name
                            );
                            needs_render = true;
                        }
                    }
                    UI2_SWARM_ITEM_UPLOAD => {
                        if let Some(handle) = selected_handle {
                            let sketch = &SWARM_SKETCHES[sketch_index % SWARM_SKETCHES.len()];
                            match crate::r::net::esp::upload_app_to_device(
                                handle,
                                sketch.source_name,
                                sketch.body,
                                UI2_SWARM_TARGET_FILENAME,
                            )
                            .await
                            {
                                Ok(()) => {
                                    action_text =
                                        format!("uploaded {} to {}", sketch.source_name, handle.0);
                                }
                                Err(_) => {
                                    action_text = format!(
                                        "upload failed for {} on {}",
                                        sketch.source_name, handle.0
                                    );
                                }
                            }
                        } else {
                            action_text = String::from("select a device first");
                        }
                        needs_render = true;
                    }
                    _ => {
                        if let Some(handle) = click_selected_device(item_id, &current_scene) {
                            selected_handle = Some(handle);
                            action_text = format!("selected device {}", handle.0);
                            needs_render = true;
                        }
                    }
                }
            }
        }

        if needs_render {
            let scene = scene.unwrap_or_else(|| {
                build_scene(devices.as_slice(), selected_handle, sketch_index, action_text.as_str())
            });
            let (pixels, interactives, content_w, content_h) = render_scene(
                &surface,
                last_viewport.0,
                last_viewport.1,
                &body_atlases,
                &emph_atlases,
                &scene,
            );
            let _ = crate::r::ui2::set_window_title(surface.window_id(), scene.title.as_str());
            let _ = surface.bind_hosted_scroll_state(UI2_SWARM_CONTENT_ID, content_w, content_h);
            let _ = surface.set_interactives(interactives.as_slice());
            if !surface.upload_rgba(pixels.as_slice(), "ui2-swarm-present") {
                break;
            }
            needs_render = false;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-swarm-demo", 80).await {
            break;
        }
    }
}
