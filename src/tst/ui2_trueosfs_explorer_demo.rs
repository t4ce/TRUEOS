use alloc::{format, string::String, vec, vec::Vec};

use crate::disc::block::{self, DiscId};
use crate::r::ui2::{self, Ui2FontTier, Ui2HostedInteractiveRect, Ui2Rect};

const UI2_TRUEOSFS_EXPLORER_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::TrueosfsExplorer.get();
const UI2_TRUEOSFS_EXPLORER_CONTENT_ID: u32 =
    crate::tst_ui2_ids::Ui2DemoContentId::TrueosfsExplorer.get();
const UI2_TRUEOSFS_EXPLORER_TEX_W: u32 = 2_048;
const UI2_TRUEOSFS_EXPLORER_TEX_H: u32 = 2_048;
const UI2_TRUEOSFS_EXPLORER_VIEW_W: f32 = 760.0;
const UI2_TRUEOSFS_EXPLORER_VIEW_H: f32 = 520.0;
const UI2_TRUEOSFS_EXPLORER_X: f32 = 180.0;
const UI2_TRUEOSFS_EXPLORER_Y: f32 = 120.0;
const UI2_TRUEOSFS_EXPLORER_Z: i16 = 34;
const UI2_TRUEOSFS_EXPLORER_ALPHA: u8 = 220;
const UI2_TRUEOSFS_EXPLORER_FONT_TIER: Ui2FontTier = Ui2FontTier::OneX;
const UI2_TRUEOSFS_EXPLORER_FONT_SIZE_CASE: usize = UI2_TRUEOSFS_EXPLORER_FONT_TIER.size_case();
const UI2_TRUEOSFS_BG_RGBA: [u8; 4] = [0xF6, 0xF0, 0xE5, 0xFF];
const UI2_TRUEOSFS_HEADER_RGBA: [u8; 4] = [0xE4, 0xD5, 0xBF, 0xFF];
const UI2_TRUEOSFS_CARD_RGBA: [u8; 4] = [0xFF, 0xFC, 0xF6, 0xFF];
const UI2_TRUEOSFS_CARD_BORDER_RGBA: [u8; 4] = [0x9A, 0x83, 0x63, 0xFF];
const UI2_TRUEOSFS_TEXT_RGBA: [u8; 4] = [0x26, 0x20, 0x16, 0xFF];
const UI2_TRUEOSFS_MUTED_RGBA: [u8; 4] = [0x6D, 0x5C, 0x44, 0xFF];
const UI2_TRUEOSFS_FOLDER_RGBA: [u8; 4] = [0xBF, 0x79, 0x26, 0xFF];
const UI2_TRUEOSFS_FILE_RGBA: [u8; 4] = [0x37, 0x69, 0x8D, 0xFF];
const UI2_TRUEOSFS_ROOT_RGBA: [u8; 4] = [0x4F, 0x63, 0x28, 0xFF];
const UI2_TRUEOSFS_ITEM_W: u32 = 140;
const UI2_TRUEOSFS_ITEM_H: u32 = 110;
const UI2_TRUEOSFS_GRID_GAP_X: u32 = 14;
const UI2_TRUEOSFS_GRID_GAP_Y: u32 = 18;
const UI2_TRUEOSFS_PAD_X: u32 = 20;
const UI2_TRUEOSFS_PAD_Y: u32 = 16;
const UI2_TRUEOSFS_HEADER_H: u32 = 52;
const UI2_TRUEOSFS_STATUS_H: u32 = 28;

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExplorerLocation {
    Roots,
    Dir { disk_id: DiscId, path: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum ExplorerEntryKind {
    Parent,
    Root,
    Dir,
    File,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ExplorerAction {
    OpenRoots,
    OpenDir { disk_id: DiscId, path: String },
    SelectFile { path: String },
}

#[derive(Clone, Debug)]
struct ExplorerEntry {
    item_id: u32,
    kind: ExplorerEntryKind,
    label: String,
    icon: char,
    accent_rgba: [u8; 4],
    action: ExplorerAction,
}

#[derive(Clone, Debug)]
struct ExplorerScene {
    title: String,
    subtitle: String,
    status: String,
    entries: Vec<ExplorerEntry>,
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
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
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

fn draw_text_line_clipped(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: u32,
    y: u32,
    max_width: u32,
    text: &str,
    fg_rgba: [u8; 4],
) {
    if max_width == 0 || text.is_empty() {
        return;
    }

    let ellipsis = ui2::ui2_font_measure_text(UI2_TRUEOSFS_EXPLORER_FONT_TIER, "...").width_px;
    let mut clipped = String::new();
    if ui2::ui2_font_measure_text(UI2_TRUEOSFS_EXPLORER_FONT_TIER, text).width_px <= max_width {
        clipped.push_str(text);
    } else {
        for ch in text.chars() {
            let mut candidate = clipped.clone();
            candidate.push(ch);
            let reserve = if candidate.len() < text.len() {
                ellipsis
            } else {
                0
            };
            if ui2::ui2_font_measure_text(UI2_TRUEOSFS_EXPLORER_FONT_TIER, candidate.as_str())
                .width_px
                .saturating_add(reserve)
                > max_width
            {
                break;
            }
            clipped.push(ch);
        }
        if clipped != text {
            clipped.push_str("...");
        }
    }

    let _ = ui2::ui2_font_blit_text_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        UI2_TRUEOSFS_EXPLORER_FONT_TIER,
        x as usize,
        y as usize,
        max_width as usize,
        clipped.as_str(),
        fg_rgba,
    );
}

fn draw_icon_centered(
    dst: &mut [u8],
    dst_width: usize,
    dst_height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    icon: char,
    rect: Ui2Rect,
    fg_rgba: [u8; 4],
) {
    let _ = ui2::ui2_font_blit_char_rgba(
        dst,
        dst_width,
        dst_height,
        atlases,
        UI2_TRUEOSFS_EXPLORER_FONT_TIER,
        icon,
        rect,
        fg_rgba,
    );
}

fn child_path(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        return String::from(name);
    }
    let mut out = String::from(parent);
    out.push('/');
    out.push_str(name);
    out
}

fn parent_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rsplit_once('/') {
        Some((parent, _)) => Some(String::from(parent)),
        None => Some(String::new()),
    }
}

fn root_label(root: crate::r::fs::trueosfs::RootInfo) -> String {
    let Some(handle) = block::device_handle(root.disk_id) else {
        return format!("root {}", root.disk_id.raw());
    };
    let info = handle.info();
    let label = info
        .label
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("TRUEOSFS");
    format!("{} [{}]", label, root.disk_id.raw())
}

async fn directory_entries(disk_id: DiscId, path: &str) -> Vec<ExplorerEntry> {
    let Some(disk) = block::device_handle(disk_id) else {
        return Vec::new();
    };
    let Ok(Some(listing)) = crate::r::fs::trueosfs::list_dir_async(disk, path).await else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for line in listing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        let child = child_path(path, line);
        let kind = match crate::r::fs::trueosfs::file_info_async(disk, child.as_str()).await {
            Ok(Some(_)) => ExplorerEntryKind::File,
            _ => match crate::r::fs::trueosfs::list_dir_async(disk, child.as_str()).await {
                Ok(Some(_)) => ExplorerEntryKind::Dir,
                _ => continue,
            },
        };
        let (icon, accent_rgba, action) = match kind {
            ExplorerEntryKind::Dir => (
                '📁',
                UI2_TRUEOSFS_FOLDER_RGBA,
                ExplorerAction::OpenDir {
                    disk_id,
                    path: child.clone(),
                },
            ),
            ExplorerEntryKind::File => (
                '📄',
                UI2_TRUEOSFS_FILE_RGBA,
                ExplorerAction::SelectFile {
                    path: child.clone(),
                },
            ),
            ExplorerEntryKind::Parent | ExplorerEntryKind::Root => continue,
        };
        out.push(ExplorerEntry {
            item_id: 0,
            kind,
            label: String::from(line),
            icon,
            accent_rgba,
            action,
        });
    }

    out.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then_with(|| left.label.as_str().cmp(right.label.as_str()))
    });
    out
}

async fn build_scene(location: &ExplorerLocation, status: &str) -> ExplorerScene {
    match location {
        ExplorerLocation::Roots => {
            let mut entries = Vec::new();
            for root in crate::r::fs::trueosfs::list_roots() {
                entries.push(ExplorerEntry {
                    item_id: 0,
                    kind: ExplorerEntryKind::Root,
                    label: root_label(root),
                    icon: '🗂',
                    accent_rgba: UI2_TRUEOSFS_ROOT_RGBA,
                    action: ExplorerAction::OpenDir {
                        disk_id: root.disk_id,
                        path: String::new(),
                    },
                });
            }
            ExplorerScene {
                title: String::from("TRUEOSFS Explorer"),
                subtitle: String::from("Mounted roots"),
                status: if status.is_empty() {
                    String::from("Click a root to browse")
                } else {
                    String::from(status)
                },
                entries,
            }
        }
        ExplorerLocation::Dir { disk_id, path } => {
            let mut entries = Vec::new();
            entries.push(ExplorerEntry {
                item_id: 0,
                kind: ExplorerEntryKind::Parent,
                label: if path.is_empty() {
                    String::from("Back to roots")
                } else {
                    String::from("Up")
                },
                icon: '↩',
                accent_rgba: UI2_TRUEOSFS_MUTED_RGBA,
                action: parent_path(path)
                    .map(|parent| ExplorerAction::OpenDir {
                        disk_id: *disk_id,
                        path: parent,
                    })
                    .unwrap_or(ExplorerAction::OpenRoots),
            });
            entries.extend(directory_entries(*disk_id, path.as_str()).await);
            ExplorerScene {
                title: format!("TRUEOSFS [{}]", disk_id.raw()),
                subtitle: if path.is_empty() {
                    String::from("/")
                } else {
                    format!("/{}", path)
                },
                status: if status.is_empty() {
                    String::from("Folders open. Files only select for now.")
                } else {
                    String::from(status)
                },
                entries,
            }
        }
    }
}

fn render_scene(
    surface: &crate::r::ui2::Ui2SurfaceWindow,
    viewport_w: u32,
    viewport_h: u32,
    atlases: &ui2::Ui2FontCpuAtlases,
    scene: &mut ExplorerScene,
) -> (Vec<u8>, Vec<Ui2HostedInteractiveRect>, u32, u32) {
    let (tex_w, tex_h) = surface.size();
    let dst_width = tex_w as usize;
    let dst_height = tex_h as usize;
    let mut rgba = vec![0u8; dst_width.saturating_mul(dst_height).saturating_mul(4)];
    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        dst_width,
        dst_height,
        UI2_TRUEOSFS_BG_RGBA,
    );

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        0,
        tex_w as usize,
        UI2_TRUEOSFS_HEADER_H as usize,
        UI2_TRUEOSFS_HEADER_RGBA,
    );

    draw_text_line_clipped(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_TRUEOSFS_PAD_X,
        10,
        tex_w.saturating_sub(UI2_TRUEOSFS_PAD_X * 2),
        scene.title.as_str(),
        UI2_TRUEOSFS_TEXT_RGBA,
    );
    draw_text_line_clipped(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_TRUEOSFS_PAD_X,
        30,
        tex_w.saturating_sub(UI2_TRUEOSFS_PAD_X * 2),
        scene.subtitle.as_str(),
        UI2_TRUEOSFS_MUTED_RGBA,
    );

    let usable_w = viewport_w.max(UI2_TRUEOSFS_ITEM_W + (UI2_TRUEOSFS_PAD_X * 2));
    let cell_span = UI2_TRUEOSFS_ITEM_W + UI2_TRUEOSFS_GRID_GAP_X;
    let cols = (((usable_w.saturating_sub(UI2_TRUEOSFS_PAD_X * 2)) + UI2_TRUEOSFS_GRID_GAP_X)
        / cell_span)
        .max(1);
    let layout_w = UI2_TRUEOSFS_PAD_X * 2
        + cols * UI2_TRUEOSFS_ITEM_W
        + cols.saturating_sub(1) * UI2_TRUEOSFS_GRID_GAP_X;
    let rows = ((scene.entries.len() as u32)
        .saturating_add(cols)
        .saturating_sub(1)
        / cols)
        .max(1);
    let grid_h = rows * UI2_TRUEOSFS_ITEM_H + rows.saturating_sub(1) * UI2_TRUEOSFS_GRID_GAP_Y;
    let content_w = layout_w.max(viewport_w).min(tex_w);
    let content_h = (UI2_TRUEOSFS_HEADER_H
        + UI2_TRUEOSFS_PAD_Y
        + grid_h
        + UI2_TRUEOSFS_STATUS_H
        + UI2_TRUEOSFS_PAD_Y)
        .max(viewport_h)
        .min(tex_h);

    let mut interactives = Vec::with_capacity(scene.entries.len());
    let icon_line_h =
        u32::from(ui2::ui2_font_native_line_height_px(UI2_TRUEOSFS_EXPLORER_FONT_TIER));
    for (idx, entry) in scene.entries.iter_mut().enumerate() {
        let col = (idx as u32) % cols;
        let row = (idx as u32) / cols;
        let x = UI2_TRUEOSFS_PAD_X + col * (UI2_TRUEOSFS_ITEM_W + UI2_TRUEOSFS_GRID_GAP_X);
        let y = UI2_TRUEOSFS_HEADER_H
            + UI2_TRUEOSFS_PAD_Y
            + row * (UI2_TRUEOSFS_ITEM_H + UI2_TRUEOSFS_GRID_GAP_Y);
        if y >= tex_h {
            break;
        }

        entry.item_id = idx as u32 + 1;
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            x as usize,
            y as usize,
            UI2_TRUEOSFS_ITEM_W as usize,
            UI2_TRUEOSFS_ITEM_H as usize,
            UI2_TRUEOSFS_CARD_RGBA,
        );
        stroke_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            x as usize,
            y as usize,
            UI2_TRUEOSFS_ITEM_W as usize,
            UI2_TRUEOSFS_ITEM_H as usize,
            UI2_TRUEOSFS_CARD_BORDER_RGBA,
        );
        fill_rect_rgba(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            x as usize,
            y as usize,
            6,
            UI2_TRUEOSFS_ITEM_H as usize,
            entry.accent_rgba,
        );

        draw_icon_centered(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            entry.icon,
            Ui2Rect {
                x: x as f32 + 12.0,
                y: y as f32 + 12.0,
                w: (UI2_TRUEOSFS_ITEM_W - 24) as f32,
                h: (icon_line_h.max(32) + 12) as f32,
            },
            entry.accent_rgba,
        );
        draw_text_line_clipped(
            rgba.as_mut_slice(),
            dst_width,
            dst_height,
            atlases,
            x + 12,
            y + icon_line_h.max(32) + 28,
            UI2_TRUEOSFS_ITEM_W.saturating_sub(24),
            entry.label.as_str(),
            UI2_TRUEOSFS_TEXT_RGBA,
        );

        interactives.push(Ui2HostedInteractiveRect {
            item_id: entry.item_id,
            x,
            y,
            width: UI2_TRUEOSFS_ITEM_W,
            height: UI2_TRUEOSFS_ITEM_H,
        });
    }

    fill_rect_rgba(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        0,
        content_h.saturating_sub(UI2_TRUEOSFS_STATUS_H) as usize,
        tex_w as usize,
        UI2_TRUEOSFS_STATUS_H as usize,
        UI2_TRUEOSFS_HEADER_RGBA,
    );
    draw_text_line_clipped(
        rgba.as_mut_slice(),
        dst_width,
        dst_height,
        atlases,
        UI2_TRUEOSFS_PAD_X,
        content_h
            .saturating_sub(UI2_TRUEOSFS_STATUS_H)
            .saturating_add(6),
        tex_w.saturating_sub(UI2_TRUEOSFS_PAD_X * 2),
        scene.status.as_str(),
        UI2_TRUEOSFS_MUTED_RGBA,
    );

    (rgba, interactives, content_w, content_h)
}

fn apply_click(
    item_id: u32,
    scene: &ExplorerScene,
    location: &mut ExplorerLocation,
    status: &mut String,
) -> bool {
    let Some(entry) = scene.entries.iter().find(|entry| entry.item_id == item_id) else {
        return false;
    };
    match &entry.action {
        ExplorerAction::OpenRoots => {
            *location = ExplorerLocation::Roots;
            status.clear();
            true
        }
        ExplorerAction::OpenDir { disk_id, path } => {
            *location = ExplorerLocation::Dir {
                disk_id: *disk_id,
                path: path.clone(),
            };
            status.clear();
            true
        }
        ExplorerAction::SelectFile { path } => {
            *status = format!("selected /{}", path);
            true
        }
    }
}

fn roots_hash() -> u32 {
    crate::r::fs::trueosfs::list_roots()
        .into_iter()
        .fold(0u32, |acc, root| {
            acc.wrapping_mul(16777619)
                .wrapping_add(root.disk_id.raw())
                .wrapping_add(root.seq)
        })
}

#[embassy_executor::task]
pub async fn ui2_trueosfs_explorer_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-trueosfs-explorer-demo");
    if crate::r::fs::trueosfs::primary_root_handle().is_none() {
        return;
    }

    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_TRUEOSFS_EXPLORER_FONT_SIZE_CASE)
    else {
        return;
    };

    let clear_rgba = vec![
        UI2_TRUEOSFS_BG_RGBA[0],
        UI2_TRUEOSFS_BG_RGBA[1],
        UI2_TRUEOSFS_BG_RGBA[2],
        UI2_TRUEOSFS_BG_RGBA[3],
    ];
    let mut clear_pixels = vec![
        0u8;
        (UI2_TRUEOSFS_EXPLORER_TEX_W as usize)
            .saturating_mul(UI2_TRUEOSFS_EXPLORER_TEX_H as usize)
            .saturating_mul(4)
    ];
    for px in clear_pixels.chunks_exact_mut(4) {
        px.copy_from_slice(clear_rgba.as_slice());
    }
    if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        UI2_TRUEOSFS_EXPLORER_TEX_ID,
        UI2_TRUEOSFS_EXPLORER_TEX_W,
        UI2_TRUEOSFS_EXPLORER_TEX_H,
        clear_pixels.as_slice(),
        0,
        "ui2-trueosfs-explorer-clear",
    ) {
        return;
    }

    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::from_existing_texture_with_size(
        "TRUEOSFS Explorer",
        Ui2Rect {
            x: UI2_TRUEOSFS_EXPLORER_X,
            y: UI2_TRUEOSFS_EXPLORER_Y,
            w: UI2_TRUEOSFS_EXPLORER_VIEW_W,
            h: UI2_TRUEOSFS_EXPLORER_VIEW_H,
        },
        UI2_TRUEOSFS_EXPLORER_Z,
        UI2_TRUEOSFS_EXPLORER_ALPHA,
        UI2_TRUEOSFS_EXPLORER_TEX_ID,
        true,
        UI2_TRUEOSFS_EXPLORER_TEX_W,
        UI2_TRUEOSFS_EXPLORER_TEX_H,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-trueosfs-explorer-demo");

    crate::r::ui2::set_window_title_twemoji(surface.window_id(), '\u{1F4C1}');

    let mut location = ExplorerLocation::Roots;
    let mut status = String::new();
    let mut last_roots_hash = 0u32;
    let mut last_viewport = (0u32, 0u32);
    let mut last_click_seq = 0u32;
    let mut needs_render = true;

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-trueosfs-explorer-demo") {
            break;
        }
        let viewport = crate::r::ui2::window_content_rect_by_id(surface.window_id())
            .map(|rect| (rect.w.max(1.0) as u32, rect.h.max(1.0) as u32))
            .unwrap_or((UI2_TRUEOSFS_EXPLORER_VIEW_W as u32, UI2_TRUEOSFS_EXPLORER_VIEW_H as u32));
        if viewport != last_viewport {
            last_viewport = viewport;
            needs_render = true;
        }

        let current_roots_hash = roots_hash();
        if current_roots_hash != last_roots_hash {
            last_roots_hash = current_roots_hash;
            if current_roots_hash == 0 {
                break;
            }
            needs_render = true;
        }

        let mut scene = if needs_render {
            Some(build_scene(&location, status.as_str()).await)
        } else {
            None
        };

        if let Some((seq, item_id)) =
            crate::r::ui2::take_window_last_clicked_item(surface.window_id())
            && seq != last_click_seq
        {
            last_click_seq = seq;
            let current_scene = match scene.take() {
                Some(scene) => scene,
                None => build_scene(&location, status.as_str()).await,
            };
            if apply_click(item_id, &current_scene, &mut location, &mut status) {
                needs_render = true;
                scene = Some(build_scene(&location, status.as_str()).await);
            } else {
                scene = Some(current_scene);
            }
        }

        if needs_render {
            let mut scene = scene.unwrap_or_else(|| ExplorerScene {
                title: String::from("TRUEOSFS Explorer"),
                subtitle: String::new(),
                status: String::new(),
                entries: Vec::new(),
            });
            let (pixels, interactives, content_w, content_h) =
                render_scene(&surface, last_viewport.0, last_viewport.1, &atlases, &mut scene);
            let _ = crate::r::ui2::set_window_title(surface.window_id(), scene.title.as_str());
            let _ = surface.bind_hosted_scroll_state(
                UI2_TRUEOSFS_EXPLORER_CONTENT_ID,
                content_w,
                content_h,
            );
            let _ = surface.set_interactives(interactives.as_slice());
            if !surface.upload_rgba(pixels.as_slice(), "ui2-trueosfs-explorer-present") {
                break;
            }
            needs_render = false;
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms("ui2-trueosfs-explorer-demo", 80).await
        {
            break;
        }
    }
}
