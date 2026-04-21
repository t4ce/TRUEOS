use alloc::{
    format,
    string::{String, ToString},
    vec,
    vec::Vec,
};


use crate::r::ui2::{self, Ui2FontTier, Ui2Rect};

const UI2_PCI_DEMO_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Pci.get();
const UI2_PCI_DEMO_CONTENT_ID: u32 = crate::tst_ui2_ids::Ui2DemoContentId::Pci.get();
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
const UI2_PCI_DEMO_TOGGLE_ACTIVE_BG_RGBA: [u8; 4] = [0x1A, 0x27, 0x34, 0xFF];
const UI2_PCI_DEMO_TOGGLE_IDLE_BG_RGBA: [u8; 4] = [0x12, 0x19, 0x22, 0xFF];
const UI2_PCI_DEMO_ICON_FALLBACK: char = '>';
const UI2_PCI_DEMO_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_PCI_DEMO_ONE_X_SIZE_CASE: usize = UI2_PCI_DEMO_FONT_TIER.size_case();
const UI2_PCI_DEMO_PAD_X: usize = 10;
const UI2_PCI_DEMO_PAD_Y: usize = 10;
const UI2_PCI_DEMO_ROW_GAP_Y: usize = 2;
const UI2_PCI_DEMO_TOGGLE_ITEM_PCI: u32 = 100;
const UI2_PCI_DEMO_TOGGLE_ITEM_USB: u32 = 101;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DeviceManagerView {
    Pci,
    Usb,
}

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

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsbDemoControllerRow {
    index: usize,
    title: String,
    detail: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsbDemoDeviceRow {
    controller_index: usize,
    port_label: String,
    name: String,
    stats: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UsbDemoSnapshot {
    controllers: Vec<UsbDemoControllerRow>,
    devices: Vec<UsbDemoDeviceRow>,
    probe_error: Option<&'static str>,
    probe_device_count: Option<u32>,
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
        .find(|ch| ui2::ui2_font_has_glyph(UI2_PCI_DEMO_FONT_TIER, *ch))
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

fn usb_device_kind_name(dev: &crate::usb2::TlbUsbDevice) -> &'static str {
    for cfg in dev.configurations.iter() {
        for interface in cfg.interfaces.iter() {
            match (interface.class, interface.subclass, interface.protocol) {
                (0x08, _, _) => return "Mass Storage",
                (0x01, _, _) => return "Audio",
                (0x03, 0x01, 0x01) => return "HID Keyboard",
                (0x03, 0x01, 0x02) => return "HID Mouse",
                (0x03, _, _) => return "HID",
                (0x09, _, _) => return "Hub",
                (0x01, 0x03, _) => return "MIDI",
                _ => {}
            }
        }
    }

    match (dev.class, dev.subclass, dev.protocol) {
        (0x09, _, _) => "Hub",
        (0x08, _, _) => "Mass Storage",
        (0x03, 0x01, 0x01) => "HID Keyboard",
        (0x03, 0x01, 0x02) => "HID Mouse",
        (0x03, _, _) => "HID",
        (0x01, _, _) => "Audio",
        _ => "USB Device",
    }
}

fn usb_device_stats(dev: &crate::usb2::TlbUsbDevice) -> String {
    let interface_count: usize = dev
        .configurations
        .iter()
        .map(|cfg| cfg.interfaces.len())
        .sum();
    let endpoint_count: usize = dev
        .configurations
        .iter()
        .flat_map(|cfg| cfg.interfaces.iter())
        .map(|interface| interface.endpoints.len())
        .sum();
    format!(
        "slot={} speed={} cfgs={} ifs={} eps={} mps0={} vidpid={:04X}:{:04X}",
        dev.slot_id,
        dev.speed,
        dev.num_configurations,
        interface_count,
        endpoint_count,
        dev.max_packet_size_0,
        dev.vendor_id,
        dev.product_id
    )
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
        UI2_PCI_DEMO_FONT_TIER,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

fn render_padded_label_rgba(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    rect: Ui2Rect,
    bg_rgba: [u8; 4],
    fg_rgba: [u8; 4],
    text: &str,
) {
    fill_rect_rgba(
        dst,
        dst_width,
        dst_height,
        rect.x.max(0.0) as usize,
        rect.y.max(0.0) as usize,
        rect.w.max(0.0) as usize,
        rect.h.max(0.0) as usize,
        bg_rgba,
    );
    render_text_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        rect.x.max(0.0) as usize + 6,
        rect.y.max(0.0) as usize + 2,
        text,
        fg_rgba,
    );
}

fn toggle_rects() -> [(u32, Ui2Rect, &'static str); 2] {
    let line_h = pci_demo_line_height() as f32;
    [
        (
            UI2_PCI_DEMO_TOGGLE_ITEM_PCI,
            Ui2Rect {
                x: UI2_PCI_DEMO_PAD_X as f32,
                y: (UI2_PCI_DEMO_PAD_Y + pci_demo_line_height() + UI2_PCI_DEMO_ROW_GAP_Y + 4)
                    as f32,
                w: 64.0,
                h: line_h + 6.0,
            },
            "PCI",
        ),
        (
            UI2_PCI_DEMO_TOGGLE_ITEM_USB,
            Ui2Rect {
                x: (UI2_PCI_DEMO_PAD_X + 72) as f32,
                y: (UI2_PCI_DEMO_PAD_Y + pci_demo_line_height() + UI2_PCI_DEMO_ROW_GAP_Y + 4)
                    as f32,
                w: 96.0,
                h: line_h + 6.0,
            },
            "USB hosts",
        ),
    ]
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

fn usb_demo_header_lines(icon: char, snapshot: &UsbDemoSnapshot) -> Vec<String> {
    let mut lines = Vec::with_capacity(4);
    lines.push(format!("{} USB host manager", icon));
    lines.push(format!(
        "view only  |  hosts: {}  |  devices: {}  |  probe={:?}/count={:?}  |  font: 1x",
        snapshot.controllers.len(),
        snapshot.devices.len(),
        snapshot.probe_error,
        snapshot.probe_device_count
    ));
    lines.push(String::from("Port path        Name           Stats"));
    lines.push(String::from("---------------  -------------  -----"));
    lines
}

fn pci_demo_row_line(icon: char, row: &PciDemoRow) -> String {
    format!("{}  {:8}  {:8}  {}", icon, row.addr, row.vid_pid, row.detail)
}

fn usb_demo_controller_lines(row: &UsbDemoControllerRow) -> [String; 2] {
    [format!("HOST {}", row.title), format!("  {}", row.detail)]
}

fn usb_demo_device_line(row: &UsbDemoDeviceRow) -> String {
    format!("{:15}  {:13}  {}", row.port_label, row.name, row.stats)
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
        .saturating_add(usize::from(snapshot.rows.is_empty()))
        .saturating_add(2);
    let content_w = max_width
        .saturating_add(UI2_PCI_DEMO_PAD_X * 2)
        .max(UI2_PCI_DEMO_VIEW_W as usize);
    let content_h = total_lines
        .saturating_mul(line_height.saturating_add(UI2_PCI_DEMO_ROW_GAP_Y))
        .saturating_add(UI2_PCI_DEMO_PAD_Y * 2)
        .max(UI2_PCI_DEMO_VIEW_H as usize);
    (content_w as u32, content_h as u32)
}

fn usb_demo_content_size(icon: char, snapshot: &UsbDemoSnapshot) -> (u32, u32) {
    let line_height = pci_demo_line_height();
    let lines = usb_demo_header_lines(icon, snapshot);
    let mut max_width = lines
        .iter()
        .map(|line| pci_demo_measure_width(line.as_str()))
        .max()
        .unwrap_or(1);
    let mut total_lines = lines.len();
    for ctrl in snapshot.controllers.iter() {
        for line in usb_demo_controller_lines(ctrl) {
            max_width = max_width.max(pci_demo_measure_width(line.as_str()));
            total_lines = total_lines.saturating_add(1);
        }
        let count = snapshot
            .devices
            .iter()
            .filter(|dev| dev.controller_index == ctrl.index)
            .count();
        total_lines = total_lines.saturating_add(count.max(1));
    }
    if snapshot.controllers.is_empty() {
        total_lines = total_lines.saturating_add(1);
    }
    total_lines = total_lines.saturating_add(2);
    for row in snapshot.devices.iter() {
        max_width = max_width.max(pci_demo_measure_width(usb_demo_device_line(row).as_str()));
    }
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

    for (item_id, rect, label) in toggle_rects() {
        let active = item_id == UI2_PCI_DEMO_TOGGLE_ITEM_PCI;
        render_padded_label_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            rect,
            if active {
                UI2_PCI_DEMO_TOGGLE_ACTIVE_BG_RGBA
            } else {
                UI2_PCI_DEMO_TOGGLE_IDLE_BG_RGBA
            },
            if active {
                UI2_PCI_DEMO_ACCENT_RGBA
            } else {
                UI2_PCI_DEMO_DIM_RGBA
            },
            label,
        );
    }

    y = y.saturating_add(line_step + 6);

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

fn compose_usb_demo_rgba(
    atlases: &ui2::Ui2FontCpuAtlases,
    icon: char,
    snapshot: &UsbDemoSnapshot,
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
    let header_lines = usb_demo_header_lines(icon, snapshot);
    let header_block_h = header_lines
        .len()
        .saturating_mul(line_step)
        .saturating_add(UI2_PCI_DEMO_PAD_Y)
        .saturating_add(line_step + 8);
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
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            line.as_str(),
            if idx == 0 {
                UI2_PCI_DEMO_ACCENT_RGBA
            } else {
                UI2_PCI_DEMO_DIM_RGBA
            },
        );
        y = y.saturating_add(line_step);
    }

    for (item_id, rect, label) in toggle_rects() {
        let active = item_id == UI2_PCI_DEMO_TOGGLE_ITEM_USB;
        render_padded_label_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            rect,
            if active {
                UI2_PCI_DEMO_TOGGLE_ACTIVE_BG_RGBA
            } else {
                UI2_PCI_DEMO_TOGGLE_IDLE_BG_RGBA
            },
            if active {
                UI2_PCI_DEMO_ACCENT_RGBA
            } else {
                UI2_PCI_DEMO_DIM_RGBA
            },
            label,
        );
    }

    y = y.saturating_add(line_step + 6);
    if snapshot.controllers.is_empty() {
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            "No USB hosts detected.",
            UI2_PCI_DEMO_ERROR_RGBA,
        );
        return rgba;
    }

    for ctrl in snapshot.controllers.iter().enumerate() {
        let (ctrl_index, ctrl_row) = ctrl;
        let row_bg = if (ctrl_index & 1) == 0 {
            UI2_PCI_DEMO_ROW_EVEN_BG_RGBA
        } else {
            UI2_PCI_DEMO_ROW_ODD_BG_RGBA
        };
        let ctrl_lines = usb_demo_controller_lines(ctrl_row);
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            0,
            y.saturating_sub(1),
            dst_width,
            line_step.saturating_mul(ctrl_lines.len()).saturating_add(2),
            row_bg,
        );
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            ctrl_lines[0].as_str(),
            UI2_PCI_DEMO_ACCENT_RGBA,
        );
        y = y.saturating_add(line_step);
        render_text_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            UI2_PCI_DEMO_PAD_X,
            y,
            ctrl_lines[1].as_str(),
            UI2_PCI_DEMO_DIM_RGBA,
        );
        y = y.saturating_add(line_step);

        let mut emitted = false;
        for dev in snapshot
            .devices
            .iter()
            .filter(|dev| dev.controller_index == ctrl_row.index)
        {
            emitted = true;
            let line = usb_demo_device_line(dev);
            render_text_rgba(
                rgba.as_mut_slice(),
                dst_width,
                dst_height,
                atlases,
                UI2_PCI_DEMO_PAD_X + 8,
                y,
                line.as_str(),
                UI2_PCI_DEMO_TEXT_RGBA,
            );
            y = y.saturating_add(line_step);
        }
        if !emitted {
            render_text_rgba(
                rgba.as_mut_slice(),
                dst_width,
                dst_height,
                atlases,
                UI2_PCI_DEMO_PAD_X + 8,
                y,
                "no cached devices on this host",
                UI2_PCI_DEMO_DIM_RGBA,
            );
            y = y.saturating_add(line_step);
        }
    }

    rgba
}

fn update_window_title(window_id: u32, view: DeviceManagerView) {
    let suffix = match view {
        DeviceManagerView::Pci => "PCI",
        DeviceManagerView::Usb => "USB hosts",
    };
    let _ = crate::r::ui2::set_window_title(
        window_id,
        format!("{} [{}]", UI2_PCI_DEMO_WINDOW_TITLE, suffix).as_str(),
    );
}
