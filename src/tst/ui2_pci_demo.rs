use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_PCI_DEMO_TEX_ID: u32 = 4_714;
const UI2_PCI_DEMO_CONTENT_ID: u32 = 44;
const UI2_PCI_DEMO_WINDOW_TITLE: &str = "Device Manager";
const UI2_PCI_DEMO_VIEW_W: u32 = 700;
const UI2_PCI_DEMO_VIEW_H: u32 = 420;
const UI2_PCI_DEMO_WINDOW_X: f32 = 340.0;
const UI2_PCI_DEMO_WINDOW_Y: f32 = 110.0;
const UI2_PCI_DEMO_WINDOW_Z: i16 = 35;
const UI2_PCI_DEMO_WINDOW_ALPHA: u8 = 224;
const UI2_PCI_DEMO_BG_RGBA: [u8; 4] = [0x0B, 0x0F, 0x14, 0xFF];
const UI2_PCI_DEMO_HEADER_BG_RGBA: [u8; 4] = [0x12, 0x19, 0x22, 0xFF];
const UI2_PCI_DEMO_ROW_EVEN_BG_RGBA: [u8; 4] = [0x0E, 0x13, 0x1A, 0xFF];
const UI2_PCI_DEMO_ROW_ODD_BG_RGBA: [u8; 4] = [0x0B, 0x0F, 0x14, 0xFF];
const UI2_PCI_DEMO_TEXT_RGBA: [u8; 4] = [0xEE, 0xF3, 0xF9, 0xFF];
const UI2_PCI_DEMO_DIM_RGBA: [u8; 4] = [0x96, 0xA4, 0xB6, 0xFF];
const UI2_PCI_DEMO_ACCENT_RGBA: [u8; 4] = [0x7F, 0xD1, 0xAE, 0xFF];
const UI2_PCI_DEMO_ERROR_RGBA: [u8; 4] = [0xF4, 0x9B, 0x9B, 0xFF];
const UI2_PCI_DEMO_ICON_FALLBACK: char = '>';
const UI2_PCI_DEMO_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_PCI_DEMO_ONE_X_SIZE_CASE: usize = UI2_PCI_DEMO_FONT_TIER.size_case();
const UI2_PCI_DEMO_PAD_X: usize = 10;
const UI2_PCI_DEMO_PAD_Y: usize = 10;
const UI2_PCI_DEMO_ROW_GAP_Y: usize = 2;

#[derive(Clone, Debug, Eq, PartialEq)]
struct PciDemoRow {
    addr: String,
    vid_pid: String,
    detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PciDemoSnapshot {
    db_loaded: bool,
    rows: Vec<PciDemoRow>,
}

fn ensure_pci_devices_enumerated() {
    let mut len = 0usize;
    crate::pci::with_devices(|list| {
        len = list.len();
    });
    if len == 0 {
        crate::pci::enumerate_impl();
    }
}

fn pci_demo_icon_char() -> char {
    const CANDIDATES: &[char] = &['󰒓', '󰟀', '󰈀', '󰻀', '󰘚', '󰌢'];

    CANDIDATES
        .iter()
        .copied()
        .find(|ch| ui2::ui2_font_resolve_glyph(UI2_PCI_DEMO_FONT_TIER, *ch).is_some())
        .unwrap_or(UI2_PCI_DEMO_ICON_FALLBACK)
}

fn pci_demo_line_height() -> usize {
    usize::from(ui2::ui2_font_native_line_height_px(UI2_PCI_DEMO_FONT_TIER).max(1))
}

fn pci_demo_measure_width(text: &str) -> usize {
    ui2::ui2_font_measure_text(UI2_PCI_DEMO_FONT_TIER, text)
        .width_px
        .max(1) as usize
}

fn pci_demo_class_text(dev: &crate::pci::PciDevice) -> String {
    format!("class {:02X}/{:02X}/{:02X}", dev.class, dev.subclass, dev.prog_if)
}

fn pci_demo_snapshot() -> PciDemoSnapshot {
    ensure_pci_devices_enumerated();

    let db = if crate::r::readiness::is_set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED) {
        crate::pci::pciids::load_sanitized_from_root_blocking()
            .ok()
            .flatten()
    } else {
        None
    };

    let mut rows = Vec::new();
    crate::pci::with_devices(|list| {
        rows.reserve(list.len());
        for dev in list.iter() {
            let detail = if let Some(db) = db.as_deref() {
                if let Some((vendor, device)) =
                    crate::pci::pciids::lookup_vendor_device_from_db(db, dev.vendor, dev.device)
                {
                    let vendor_s = String::from_utf8_lossy(vendor).trim().to_string();
                    let device_s = String::from_utf8_lossy(device).trim().to_string();
                    format!("{} {}", vendor_s, device_s).trim().to_string()
                } else {
                    pci_demo_class_text(dev)
                }
            } else {
                pci_demo_class_text(dev)
            };

            rows.push(PciDemoRow {
                addr: format!("{:02X}:{:02X}.{}", dev.bus, dev.slot, dev.function),
                vid_pid: format!("{:04X}:{:04X}", dev.vendor, dev.device),
                detail,
            });
        }
    });

    PciDemoSnapshot {
        db_loaded: db.is_some(),
        rows,
    }
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
    let line_height = pci_demo_line_height() as f32;
    let mut pen_x = x;
    for ch in text.chars() {
        let Some(glyph) = ui2::ui2_font_resolve_glyph(UI2_PCI_DEMO_FONT_TIER, ch)
            .or_else(|| ui2::ui2_font_resolve_glyph(UI2_PCI_DEMO_FONT_TIER, '?'))
        else {
            continue;
        };
        let advance_px = usize::from(glyph.advance_px.max(1));
        let _ = ui2::ui2_font_blit_glyph_rgba(
            dst,
            dst_width,
            dst_height,
            atlases,
            &glyph,
            Ui2Rect {
                x: pen_x as f32,
                y: y as f32,
                w: advance_px as f32,
                h: line_height,
            },
            rgba,
        );
        pen_x = pen_x.saturating_add(advance_px);
        if pen_x >= dst_width {
            break;
        }
    }
}

fn pci_demo_header_lines(icon: char, snapshot: &PciDemoSnapshot) -> Vec<String> {
    let mut lines = Vec::with_capacity(4);
    lines.push(format!("{} PCI device manager", icon));
    lines.push(format!(
        "view only  |  {} devices  |  names: {}  |  font: 1x",
        snapshot.rows.len(),
        if snapshot.db_loaded {
            "pci.ids"
        } else {
            "raw ids"
        }
    ));
    lines.push(String::from("Icon  BDF       VID:PID   Description"));
    lines.push(String::from("----  --------  --------  -----------"));
    lines
}

fn pci_demo_row_line(icon: char, row: &PciDemoRow) -> String {
    format!("{}  {:8}  {:8}  {}", icon, row.addr, row.vid_pid, row.detail)
}

fn pci_demo_content_size(icon: char, snapshot: &PciDemoSnapshot) -> (u32, u32) {
    let line_height = pci_demo_line_height();
    let lines = pci_demo_header_lines(icon, snapshot);
    let mut max_width = lines
        .iter()
        .map(|line| pci_demo_measure_width(line.as_str()))
        .max()
        .unwrap_or(1);
    for row in snapshot.rows.iter() {
        max_width = max_width.max(pci_demo_measure_width(pci_demo_row_line(icon, row).as_str()));
    }
    let total_lines = lines
        .len()
        .saturating_add(snapshot.rows.len())
        .saturating_add(usize::from(snapshot.rows.is_empty()));
    let content_w = max_width
        .saturating_add(UI2_PCI_DEMO_PAD_X * 2)
        .max(UI2_PCI_DEMO_VIEW_W as usize);
    let content_h = total_lines
        .saturating_mul(line_height.saturating_add(UI2_PCI_DEMO_ROW_GAP_Y))
        .saturating_add(UI2_PCI_DEMO_PAD_Y * 2)
        .max(UI2_PCI_DEMO_VIEW_H as usize);
    (content_w as u32, content_h as u32)
}

fn compose_pci_demo_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    icon: char,
    snapshot: &PciDemoSnapshot,
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
        UI2_PCI_DEMO_BG_RGBA,
    );

    let line_height = pci_demo_line_height();
    let line_step = line_height.saturating_add(UI2_PCI_DEMO_ROW_GAP_Y);
    let header_lines = pci_demo_header_lines(icon, snapshot);
    let header_block_h = header_lines
        .len()
        .saturating_mul(line_step)
        .saturating_add(UI2_PCI_DEMO_PAD_Y);
    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        header_block_h.min(dst_height),
        UI2_PCI_DEMO_HEADER_BG_RGBA,
    );

    let mut y = UI2_PCI_DEMO_PAD_Y;
    for (idx, line) in header_lines.iter().enumerate() {
        let color = if idx == 0 {
            UI2_PCI_DEMO_ACCENT_RGBA
        } else {
            UI2_PCI_DEMO_DIM_RGBA
        };
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            line.as_str(),
            color,
        );
        y = y.saturating_add(line_step);
    }

    if snapshot.rows.is_empty() {
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            "No PCI devices enumerated.",
            UI2_PCI_DEMO_ERROR_RGBA,
        );
        return rgba;
    }

    for (row_idx, row) in snapshot.rows.iter().enumerate() {
        let row_y = y;
        let row_bg = if (row_idx & 1) == 0 {
            UI2_PCI_DEMO_ROW_EVEN_BG_RGBA
        } else {
            UI2_PCI_DEMO_ROW_ODD_BG_RGBA
        };
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            0,
            row_y.saturating_sub(1),
            dst_width,
            line_height.saturating_add(2),
            row_bg,
        );
        let line = pci_demo_row_line(icon, row);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            row_y,
            line.as_str(),
            UI2_PCI_DEMO_TEXT_RGBA,
        );
        y = y.saturating_add(line_step);
    }

    rgba
}

#[embassy_executor::task]
pub async fn ui2_pci_demo_task() {
    Timer::after(EmbassyDuration::from_millis(250)).await;

    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_PCI_DEMO_ONE_X_SIZE_CASE) else {
        crate::log!(
            "ui2-pci-demo: atlas decode failed size_case={}\n",
            UI2_PCI_DEMO_ONE_X_SIZE_CASE
        );
        return;
    };

    let icon = pci_demo_icon_char();
    let snapshot = pci_demo_snapshot();
    let (content_w, content_h) = pci_demo_content_size(icon, &snapshot);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        UI2_PCI_DEMO_WINDOW_TITLE,
        crate::r::ui2::Ui2Rect {
            x: UI2_PCI_DEMO_WINDOW_X,
            y: UI2_PCI_DEMO_WINDOW_Y,
            w: UI2_PCI_DEMO_VIEW_W as f32,
            h: UI2_PCI_DEMO_VIEW_H as f32,
        },
        UI2_PCI_DEMO_WINDOW_Z,
        UI2_PCI_DEMO_WINDOW_ALPHA,
        UI2_PCI_DEMO_TEX_ID,
        true,
        content_w,
        content_h,
    ) else {
        crate::log!("ui2-pci-demo: window creation failed tex={}\n", UI2_PCI_DEMO_TEX_ID);
        return;
    };

    let rgba = compose_pci_demo_rgba(&atlases, icon, &snapshot, content_w, content_h);
    if !surface.upload_rgba(rgba.as_slice(), "ui2-pci-demo-upload") {
        crate::log!(
            "ui2-pci-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            content_w,
            content_h
        );
        return;
    }

    let _ = surface.bind_hosted_scroll_state(UI2_PCI_DEMO_CONTENT_ID, content_w, content_h);
    let _ = crate::r::ui2::set_window_icon(surface.window_id(), 11);
    crate::log!(
        "ui2-pci-demo: window={} tex={} viewport={}x{} content={}x{} devices={} names={} icon=U+{:04X}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_PCI_DEMO_VIEW_W,
        UI2_PCI_DEMO_VIEW_H,
        content_w,
        content_h,
        snapshot.rows.len(),
        snapshot.db_loaded as u8,
        icon as u32
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}
