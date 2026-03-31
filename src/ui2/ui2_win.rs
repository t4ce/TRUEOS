#![allow(dead_code)]

use super::ui2_hid::note_selection_change;
use super::*;

#[repr(u32)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum Ui2WindowStateKind {
    Normal = 0,
    Minimized = 1,
    Maximized = 2,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TrueosUi2WindowInfo {
    pub id: u32,
    pub kind: u32,
    pub state: u32,
    pub decoration_mode: u32,
    pub icon_id: u32,
    pub visible: u32,
    pub hit_test_visible: u32,
    pub selected: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub content_x: i32,
    pub content_y: i32,
    pub content_width: u32,
    pub content_height: u32,
    pub decoration_x: i32,
    pub decoration_y: i32,
    pub decoration_width: u32,
    pub decoration_height: u32,
}

pub(super) fn alloc_window(
    state: &mut Ui2State,
    kind: Ui2WindowKind,
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
) -> u32 {
    let id = state.next_window_id;
    state.next_window_id = state.next_window_id.wrapping_add(1).max(1);
    state.windows.push(Ui2Window {
        id,
        kind,
        browser_instance_id: if kind == Ui2WindowKind::HostedBrowser {
            PRIMARY_HOSTED_CONTENT_ID
        } else {
            0
        },
        hosted_browser_snapshot: UiHostedBrowserSnapshot::default(),
        title: String::from(title),
        icon_id: 0,
        rect,
        restore_rect: rect,
        z,
        visible: true,
        hit_test_visible: true,
        alpha,
        decoration_mode: Ui2WindowDecorationMode::System,
        titlebar_visible: true,
        bottom_bar_visible: true,
        left_scrollbar_visible: true,
        bottom_scrollbar_visible: true,
        vertical_scrollbar_side: Ui2WindowVerticalScrollbarSide::Left,
        horizontal_scrollbar_side: Ui2WindowHorizontalScrollbarSide::Bottom,
        state: Ui2WindowStateKind::Normal,
        content_tex_id: 0,
        content_tex_blend: false,
        hosted_surface_bg_rgba: [0, 0, 0, 0],
        hosted_surface_tiles: Vec::new(),
        title_tex_id: window_title_tex_id(id),
        title_tex_w: 0,
        title_tex_h: 0,
        title_tex_alpha: alpha,
        container_sync_needed: true,
        selected_cursor_slots: Vec::new(),
        dirty: true,
        dirty_seq: 0,
        last_reason: "create",
        last_logged_dirty_seq: 0,
        last_logged_reason: "",
    });
    queue_hosted_container_sync();
    id
}

fn minimized_window_strip_rect(state: &Ui2State, window_id: u32) -> Option<Ui2Rect> {
    let mut slot = 0usize;
    for idx in sorted_window_indices(state) {
        let window = &state.windows[idx];
        if !window.visible || window.state != Ui2WindowStateKind::Minimized {
            continue;
        }
        if window.id == window_id {
            let total_w = UI2_MINIMIZED_STRIP_W + UI2_MINIMIZED_STRIP_GAP;
            let cols_f = libm::floorf(
                (((state.view_w as f32) - (UI2_MINIMIZED_STRIP_PAD * 2.0)
                    + UI2_MINIMIZED_STRIP_GAP)
                    / total_w),
            );
            let cols = cols_f.max(1.0) as usize;
            let col = slot % cols;
            let row = slot / cols;
            let x = UI2_MINIMIZED_STRIP_PAD + (col as f32 * total_w);
            let y =
                UI2_MINIMIZED_STRIP_PAD + (row as f32 * (UI2_TITLE_H + UI2_MINIMIZED_STRIP_GAP));
            let max_w = ((state.view_w as f32) - x - UI2_MINIMIZED_STRIP_PAD).max(96.0);
            return Some(Ui2Rect::new(x, y, UI2_MINIMIZED_STRIP_W.min(max_w), UI2_TITLE_H));
        }
        slot = slot.saturating_add(1);
    }
    None
}

pub(super) fn effective_window_rect(state: &Ui2State, window: &Ui2Window) -> Ui2Rect {
    if window.state == Ui2WindowStateKind::Minimized {
        minimized_window_strip_rect(state, window.id).unwrap_or(Ui2Rect::new(
            UI2_MINIMIZED_STRIP_PAD,
            UI2_MINIMIZED_STRIP_PAD,
            UI2_MINIMIZED_STRIP_W,
            UI2_TITLE_H,
        ))
    } else {
        window.rect
    }
}

fn window_kind_id(kind: Ui2WindowKind) -> u32 {
    match kind {
        Ui2WindowKind::HostedBrowser => 1,
        Ui2WindowKind::HostedSurface => 2,
        Ui2WindowKind::Hosted3d => 4,
    }
}

fn window_info(state: &Ui2State, window: &Ui2Window) -> TrueosUi2WindowInfo {
    let rect = effective_window_rect(state, window);
    let content = window_content_rect(state, window).unwrap_or(Ui2Rect::new(0.0, 0.0, 0.0, 0.0));
    let decoration =
        window_decoration_rect(state, window).unwrap_or(Ui2Rect::new(0.0, 0.0, 0.0, 0.0));
    TrueosUi2WindowInfo {
        id: window.id,
        kind: window_kind_id(window.kind),
        state: window.state as u32,
        decoration_mode: window.decoration_mode as u32,
        icon_id: window.icon_id,
        visible: if window.visible { 1 } else { 0 },
        hit_test_visible: if window.hit_test_visible { 1 } else { 0 },
        selected: if window.selected_cursor_slots.is_empty() {
            0
        } else {
            1
        },
        x: libm::roundf(rect.x) as i32,
        y: libm::roundf(rect.y) as i32,
        width: round_to_u32(rect.w, 0),
        height: round_to_u32(rect.h, 0),
        content_x: libm::roundf(content.x) as i32,
        content_y: libm::roundf(content.y) as i32,
        content_width: round_to_u32(content.w, 0),
        content_height: round_to_u32(content.h, 0),
        decoration_x: libm::roundf(decoration.x) as i32,
        decoration_y: libm::roundf(decoration.y) as i32,
        decoration_width: round_to_u32(decoration.w, 0),
        decoration_height: round_to_u32(decoration.h, 0),
    }
}

pub(super) fn normalized_window_rect(state: &Ui2State, rect: Ui2Rect) -> Ui2Rect {
    normalized_window_rect_for_view(state.view_w, state.view_h, rect)
}

pub(super) fn normalized_window_rect_for_view(view_w: u32, view_h: u32, rect: Ui2Rect) -> Ui2Rect {
    let min_extent = ui2_window_min_extent();
    let max_w = (view_w as f32).max(min_extent);
    let max_h = (view_h as f32).max(min_extent);
    Ui2Rect::new(
        rect.x,
        rect.y,
        rect.w.max(min_extent).min(max_w),
        rect.h.max(min_extent).min(max_h),
    )
}

pub(super) fn maximize_window_rect(state: &Ui2State) -> Ui2Rect {
    Ui2Rect::new(0.0, 0.0, (state.view_w as f32).max(1.0), (state.view_h as f32).max(1.0))
}

pub(super) fn left_half_window_rect(state: &Ui2State) -> Ui2Rect {
    let view_w = (state.view_w as f32).max(1.0);
    let view_h = (state.view_h as f32).max(1.0);
    Ui2Rect::new(0.0, 0.0, (view_w * 0.5).max(1.0), view_h)
}

pub(super) fn right_half_window_rect(state: &Ui2State) -> Ui2Rect {
    let view_w = (state.view_w as f32).max(1.0);
    let view_h = (state.view_h as f32).max(1.0);
    let half_w = (view_w * 0.5).max(1.0);
    Ui2Rect::new(view_w - half_w, 0.0, half_w, view_h)
}

pub(super) fn set_window_rect_in_state(
    state: &mut Ui2State,
    id: u32,
    rect: Ui2Rect,
    reason: &'static str,
) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    let next_rect = normalized_window_rect_for_view(view_w, view_h, rect);
    if window.state == Ui2WindowStateKind::Normal && window.rect == next_rect {
        return true;
    }
    if window.state != Ui2WindowStateKind::Normal {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.rect = next_rect;
    window.restore_rect = next_rect;
    window.state = Ui2WindowStateKind::Normal;
    state.compose_reason = reason;
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, reason)
}

pub(super) fn commit_window_geometry_change(
    state: &mut Ui2State,
    id: u32,
    reason: &'static str,
) -> bool {
    let noted = note_window_dirty(state, id, reason);
    if noted {
        let _ = note_window_viewport_sync_needed(state, id);
        refresh_window_hit_entries(state, id);
    }
    noted
}

pub(super) fn minimize_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Minimized {
        return true;
    }
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.state = Ui2WindowStateKind::Minimized;
    state.compose_reason = "minimize-window";
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, "minimize-window")
}

pub(super) fn maximize_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let next_rect = maximize_window_rect(state);
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Maximized && window.rect == next_rect {
        return true;
    }
    if window.state != Ui2WindowStateKind::Maximized {
        window.restore_rect = normalized_window_rect_for_view(view_w, view_h, window.rect);
    }
    window.rect = next_rect;
    window.state = Ui2WindowStateKind::Maximized;
    state.compose_reason = "maximize-window";
    clear_window_drag_claims(state, id);
    commit_window_geometry_change(state, id, "maximize-window")
}

pub(super) fn restore_window_in_state(state: &mut Ui2State, id: u32) -> bool {
    let view_w = state.view_w;
    let view_h = state.view_h;
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    if window.state == Ui2WindowStateKind::Normal {
        return true;
    }
    if window.restore_rect.w > 0.0 && window.restore_rect.h > 0.0 {
        window.rect = normalized_window_rect_for_view(view_w, view_h, window.restore_rect);
    }
    window.state = Ui2WindowStateKind::Normal;
    state.compose_reason = "restore-window";
    commit_window_geometry_change(state, id, "restore-window")
}

pub(super) fn set_window_visible_in_state(state: &mut Ui2State, id: u32, visible: bool) -> bool {
    let Some(window) = window_mut(state, id) else {
        return false;
    };
    window.visible = visible;
    let reason = if visible {
        "show-window"
    } else {
        "hide-window"
    };
    state.compose_reason = reason;
    if !visible {
        clear_window_drag_claims(state, id);
    }
    let noted = note_window_dirty(state, id, reason);
    if noted {
        let _ = note_window_viewport_sync_needed(state, id);
        refresh_window_hit_entries(state, id);
    }
    noted
}

pub(super) fn handle_system_button_action(
    state: &mut Ui2State,
    window_id: u32,
    action: Ui2SystemButtonAction,
) -> bool {
    match action {
        Ui2SystemButtonAction::Fork => fork_window_in_state(state, window_id),
        Ui2SystemButtonAction::Minimize => minimize_window_in_state(state, window_id),
        Ui2SystemButtonAction::ToggleMaximize => {
            let is_maximized = state
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.state == Ui2WindowStateKind::Maximized)
                .unwrap_or(false);
            if is_maximized {
                restore_window_in_state(state, window_id)
            } else {
                maximize_window_in_state(state, window_id)
            }
        }
        Ui2SystemButtonAction::Close => set_window_visible_in_state(state, window_id, false),
    }
}

pub(super) fn fork_window_in_state(state: &mut Ui2State, source_window_id: u32) -> bool {
    let Some(source_window) = state
        .windows
        .iter()
        .find(|window| window.id == source_window_id)
    else {
        return false;
    };

    let source_rect = if source_window.state == Ui2WindowStateKind::Normal {
        source_window.rect
    } else if source_window.restore_rect.w > 0.0 && source_window.restore_rect.h > 0.0 {
        source_window.restore_rect
    } else {
        source_window.rect
    };
    let next_rect = normalized_window_rect_for_view(
        state.view_w,
        state.view_h,
        Ui2Rect::new(
            source_rect.x + UI2_BROWSER_FORK_WINDOW_OFFSET_PX,
            source_rect.y + UI2_BROWSER_FORK_WINDOW_OFFSET_PX,
            source_rect.w,
            source_rect.h,
        ),
    );
    let next_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(source_window.z)
        .saturating_add(1);
    let next_title = source_window.title.clone();
    let next_icon_id = source_window.icon_id;
    let next_alpha = source_window.alpha;
    let next_hit_test_visible = source_window.hit_test_visible;
    let next_decoration_mode = source_window.decoration_mode;
    let next_titlebar_visible = source_window.titlebar_visible;
    let next_bottom_bar_visible = source_window.bottom_bar_visible;
    let next_left_scrollbar_visible = source_window.left_scrollbar_visible;
    let next_bottom_scrollbar_visible = source_window.bottom_scrollbar_visible;
    let next_vertical_scrollbar_side = source_window.vertical_scrollbar_side;
    let next_horizontal_scrollbar_side = source_window.horizontal_scrollbar_side;
    let next_content_tex_blend = source_window.content_tex_blend;
    let next_hosted_surface_bg_rgba = source_window.hosted_surface_bg_rgba;
    let next_hosted_surface_tiles = source_window.hosted_surface_tiles.clone();
    let next_kind = source_window.kind;

    let (next_browser_instance_id, next_tex_id, fork_reason) = match next_kind {
        Ui2WindowKind::HostedBrowser => {
            let source_browser_instance_id = window_browser_instance_id(source_window);
            let target_browser_instance_id = trueos_qjs::browser_task::BOOT_BROWSER_INSTANCE_IDS
                .iter()
                .copied()
                .find(|browser_instance_id| {
                    *browser_instance_id != source_browser_instance_id
                        && state.windows.iter().all(|window| {
                            window.kind != Ui2WindowKind::HostedBrowser
                                || window_browser_instance_id(window) != *browser_instance_id
                        })
                });
            let Some(target_browser_instance_id) = target_browser_instance_id else {
                crate::log!(
                    "ui2: browser-fork no-target window={} source_browser={}\n",
                    source_window_id,
                    source_browser_instance_id
                );
                return false;
            };
            (
                target_browser_instance_id,
                trueos_qjs::browser_task::render_tex_id_for_browser_instance(
                    target_browser_instance_id,
                ),
                "fork-browser-window",
            )
        }
        Ui2WindowKind::HostedSurface => (0, source_window.content_tex_id, "fork-surface-window"),
        Ui2WindowKind::Hosted3d => (0, source_window.content_tex_id, "fork-3d-window"),
    };

    let id = alloc_window(state, next_kind, next_title.as_str(), next_rect, next_z, next_alpha);
    if let Some(window) = window_mut(state, id) {
        window.browser_instance_id = next_browser_instance_id;
        window.icon_id = next_icon_id;
        window.content_tex_id = next_tex_id;
        window.content_tex_blend = next_content_tex_blend;
        window.hosted_surface_bg_rgba = next_hosted_surface_bg_rgba;
        window.hosted_surface_tiles = next_hosted_surface_tiles;
        window.hit_test_visible = next_hit_test_visible;
        window.decoration_mode = next_decoration_mode;
        window.titlebar_visible = next_titlebar_visible;
        window.bottom_bar_visible = next_bottom_bar_visible;
        window.left_scrollbar_visible = next_left_scrollbar_visible;
        window.bottom_scrollbar_visible = next_bottom_scrollbar_visible;
        window.vertical_scrollbar_side = next_vertical_scrollbar_side;
        window.horizontal_scrollbar_side = next_horizontal_scrollbar_side;
        window.state = Ui2WindowStateKind::Normal;
        window.rect = next_rect;
        window.restore_rect = next_rect;
    }

    let initial_content = state
        .windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window_content_rect(state, window))
        .map(|content| {
            let (_, _, width, height) = snap_browser_content_rect(content);
            (width, height)
        });

    if next_kind == Ui2WindowKind::HostedBrowser {
        let _ = hosted_bind_window(next_browser_instance_id, id);
        let _ = trueos_qjs::browser_task::set_browser_render_target_tex_id_for_browser(
            next_browser_instance_id,
            next_tex_id,
        );
    }
    state.compose_reason = fork_reason;
    let _ = note_window_dirty(state, id, fork_reason);
    let _ = note_window_viewport_sync_needed(state, id);
    refresh_window_hit_entries(state, id);
    match next_kind {
        Ui2WindowKind::HostedBrowser => {
            crate::log!(
                "ui2: browser-fork window={} browser={} from_window={}\n",
                id,
                next_browser_instance_id,
                source_window_id
            );
        }
        Ui2WindowKind::HostedSurface => {
            crate::log!(
                "ui2: surface-fork window={} tex={} from_window={}\n",
                id,
                next_tex_id,
                source_window_id
            );
        }
        Ui2WindowKind::Hosted3d => {
            crate::log!(
                "ui2: 3d-fork window={} tex={} from_window={}\n",
                id,
                next_tex_id,
                source_window_id
            );
        }
    }

    if let Some((width, height)) = initial_content
        && next_kind == Ui2WindowKind::HostedBrowser
    {
        let pixels =
            alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
        let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            next_tex_id,
            width,
            height,
            pixels.as_slice(),
            id,
            fork_reason,
        );
    }

    true
}

#[inline]
pub(super) fn is_valid_resize_edge_mask(edge_mask: u32) -> bool {
    if edge_mask == 0 {
        return false;
    }
    if (edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 && (edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
        return false;
    }
    if (edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 && (edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
        return false;
    }
    true
}

pub fn browser_window_id() -> Option<u32> {
    let id = UI2_BROWSER_WINDOW_ID.load(Ordering::Acquire);
    if id != 0 {
        return Some(id);
    }
    let hosted_id = hosted_primary_window_id();
    if hosted_id == 0 {
        None
    } else {
        Some(hosted_id)
    }
}

pub fn browser_window_id_for_instance(browser_instance_id: u32) -> Option<u32> {
    let browser_instance_id = if browser_instance_id == 0 {
        PRIMARY_HOSTED_CONTENT_ID
    } else {
        browser_instance_id
    };
    let window_id = hosted_window_id_for_content(browser_instance_id);
    if window_id == 0 {
        None
    } else {
        Some(window_id)
    }
}

pub fn window_info_by_id(id: u32) -> Option<TrueosUi2WindowInfo> {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .map(|window| window_info(&state, window))
}

pub fn is_window_minimized(id: u32) -> bool {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .map(|window| window.state == Ui2WindowStateKind::Minimized)
        .unwrap_or(false)
}

pub fn create_window(title: &str, rect: Ui2Rect, z: i16, alpha: u8) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(&mut state, Ui2WindowKind::HostedSurface, title, rect, z, alpha);
    state.compose_reason = "create-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_browser_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    browser_instance_id: u32,
    tex_id: u32,
) -> u32 {
    let browser_instance_id = if browser_instance_id == 0 {
        PRIMARY_HOSTED_CONTENT_ID
    } else {
        browser_instance_id
    };
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(&mut state, Ui2WindowKind::HostedBrowser, title, rect, z, alpha);
    if let Some(window) = window_mut(&mut state, id) {
        window.browser_instance_id = browser_instance_id;
        window.content_tex_id = tex_id;
        window.content_tex_blend = true;
        refresh_hosted_browser_snapshot(window);
    }
    let initial_content = state
        .windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window_content_rect(&state, window))
        .map(|content| {
            let (_, _, width, height) = snap_browser_content_rect(content);
            (width, height)
        });
    if browser_instance_id == PRIMARY_HOSTED_CONTENT_ID {
        UI2_BROWSER_WINDOW_ID.store(id, Ordering::Release);
    }
    let _ = hosted_bind_window(browser_instance_id, id);
    let _ = trueos_qjs::browser_task::set_browser_render_target_tex_id_for_browser(
        browser_instance_id,
        tex_id,
    );
    state.compose_reason = "create-browser-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    drop(state);

    if let Some((width, height)) = initial_content {
        let pixels =
            alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
        let _ = crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            tex_id,
            width,
            height,
            pixels.as_slice(),
            id,
            "create-browser-window",
        );
    }
    id
}

pub fn create_hosted_browser_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    browser_instance_id: u32,
    tex_id: u32,
) -> u32 {
    let rect = window_rect_for_content(Ui2WindowDecorationMode::System, content_rect);
    create_hosted_browser_window(title, rect, z, alpha, browser_instance_id, tex_id)
}

pub fn create_hosted_surface_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(&mut state, Ui2WindowKind::HostedSurface, title, rect, z, alpha);
    if let Some(window) = window_mut(&mut state, id) {
        window.content_tex_id = tex_id;
        window.content_tex_blend = blend_enabled;
    }
    state.compose_reason = "create-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_surface_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let rect = window_rect_for_content(Ui2WindowDecorationMode::System, content_rect);
    create_hosted_surface_window(title, rect, z, alpha, tex_id, blend_enabled)
}

pub fn create_hosted_3d_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let id = alloc_window(&mut state, Ui2WindowKind::Hosted3d, title, rect, z, alpha);
    if let Some(window) = window_mut(&mut state, id) {
        window.content_tex_id = tex_id;
        window.content_tex_blend = blend_enabled;
    }
    state.compose_reason = "create-3d-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_hosted_3d_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    let rect = window_rect_for_content(Ui2WindowDecorationMode::System, content_rect);
    create_hosted_3d_window(title, rect, z, alpha, tex_id, blend_enabled)
}

pub fn set_window_hosted_surface_content(id: u32, tex_id: u32, blend_enabled: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.content_tex_id == tex_id && window.content_tex_blend == blend_enabled {
        return true;
    }
    window.content_tex_id = tex_id;
    window.content_tex_blend = blend_enabled;
    window.hosted_surface_tiles.clear();
    state.compose_reason = "texture-window";
    let noted = note_window_dirty(&mut state, id, "texture-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
    }
    noted
}

pub fn set_window_hosted_surface_tiles(
    id: u32,
    bg_rgba: [u8; 4],
    tiles: &[Ui2HostedSurfaceTile],
) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.kind != Ui2WindowKind::HostedSurface {
        return false;
    }
    window.hosted_surface_bg_rgba = bg_rgba;
    window.hosted_surface_tiles.clear();
    window.hosted_surface_tiles.extend_from_slice(tiles);
    state.compose_reason = "surface-tiles-window";
    let noted = note_window_dirty(&mut state, id, "surface-tiles-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
    }
    noted
}

pub fn bind_window_hosted_surface_state(
    id: u32,
    content_id: u32,
    content_width: u32,
    content_height: u32,
) -> bool {
    if content_id == 0 {
        return false;
    }

    let state_lock = init_state();
    let mut state = state_lock.lock();
    let content = state
        .windows
        .iter()
        .find(|window| window.id == id && window.kind == Ui2WindowKind::HostedSurface)
        .and_then(|window| window_content_rect(&state, window));
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.kind != Ui2WindowKind::HostedSurface {
        return false;
    }
    window.browser_instance_id = content_id;
    state.compose_reason = "bind-hosted-surface-state";
    let noted = note_window_dirty(&mut state, id, "bind-hosted-surface-state");
    let _ = note_window_viewport_sync_needed(&mut state, id);
    drop(state);

    let _ = hosted_bind_window(content_id, id);
    if let Some(content) = content {
        let (content_x, content_y, viewport_w, viewport_h) = snap_browser_content_rect(content);
        let _ = hosted_set_viewport(
            content_id,
            viewport_w,
            viewport_h,
            content_x,
            content_y,
            content_width.max(viewport_w),
            content_height.max(viewport_h),
        );
    }
    noted
}

pub struct Ui2SurfaceWindow {
    window_id: u32,
    tex_id: u32,
    width: u32,
    height: u32,
}

impl Ui2SurfaceWindow {
    fn attach_tiled_content(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        bg_rgba: [u8; 4],
    ) -> Option<Self> {
        let window_id =
            create_hosted_surface_content_window(title, content_rect, z, alpha, 0, false);
        if !set_window_hosted_surface_tiles(window_id, bg_rgba, &[]) {
            return None;
        }
        Some(Self {
            window_id,
            tex_id: 0,
            width: (content_rect.w.max(1.0) + 0.5) as u32,
            height: (content_rect.h.max(1.0) + 0.5) as u32,
        })
    }

    fn attach_existing_texture_with_size(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
        width: u32,
        height: u32,
    ) -> Self {
        let window_id = create_hosted_surface_content_window(
            title,
            content_rect,
            z,
            alpha,
            tex_id,
            blend_enabled,
        );
        Self {
            window_id,
            tex_id,
            width,
            height,
        }
    }

    fn attach_existing_texture(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
    ) -> Self {
        let width = (content_rect.w.max(1.0) + 0.5) as u32;
        let height = (content_rect.h.max(1.0) + 0.5) as u32;
        Self::attach_existing_texture_with_size(
            title,
            content_rect,
            z,
            alpha,
            tex_id,
            blend_enabled,
            width,
            height,
        )
    }

    pub fn from_existing_texture(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
    ) -> Option<Self> {
        Some(Self::attach_existing_texture(title, content_rect, z, alpha, tex_id, blend_enabled))
    }

    pub fn from_existing_texture_with_size(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
        tex_width: u32,
        tex_height: u32,
    ) -> Option<Self> {
        Some(Self::attach_existing_texture_with_size(
            title,
            content_rect,
            z,
            alpha,
            tex_id,
            blend_enabled,
            tex_width.max(1),
            tex_height.max(1),
        ))
    }

    pub fn from_tiled_content(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        bg_rgba: [u8; 4],
    ) -> Option<Self> {
        Self::attach_tiled_content(title, content_rect, z, alpha, bg_rgba)
    }

    pub fn new(
        title: &str,
        content_rect: Ui2Rect,
        z: i16,
        alpha: u8,
        tex_id: u32,
        blend_enabled: bool,
        clear_rgba: [u8; 4],
    ) -> Option<Self> {
        let width = (content_rect.w.max(1.0) + 0.5) as u32;
        let height = (content_rect.h.max(1.0) + 0.5) as u32;
        let pixels = alloc::vec![clear_rgba; (width as usize) * (height as usize)]
            .into_iter()
            .flatten()
            .collect::<Vec<u8>>();
        if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            tex_id,
            width,
            height,
            &pixels,
            0,
            "ui2-surface-init",
        ) {
            crate::log!(
                "ui2-surface-window: init upload queue failed tex={} size={}x{}\n",
                tex_id,
                width,
                height
            );
            return None;
        }

        Some(Self::attach_existing_texture(title, content_rect, z, alpha, tex_id, blend_enabled))
    }

    #[inline]
    pub fn window_id(&self) -> u32 {
        self.window_id
    }

    #[inline]
    pub fn tex_id(&self) -> u32 {
        self.tex_id
    }

    #[inline]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    pub fn bind_hosted_scroll_state(
        &self,
        content_id: u32,
        content_width: u32,
        content_height: u32,
    ) -> bool {
        bind_window_hosted_surface_state(self.window_id, content_id, content_width, content_height)
    }

    pub fn set_tiles(&self, bg_rgba: [u8; 4], tiles: &[Ui2HostedSurfaceTile]) -> bool {
        set_window_hosted_surface_tiles(self.window_id, bg_rgba, tiles)
    }

    pub fn render_rgb_triangles(
        &self,
        clear_rgb: u32,
        verts: &[u8],
        repaint_reason: &'static str,
    ) -> bool {
        let repaint_window_id = if is_window_minimized(self.window_id) {
            0
        } else {
            self.window_id
        };
        if !crate::r::io::cabi::queue_render_rgb_triangles_to_texture_copy(
            self.tex_id,
            clear_rgb,
            verts,
            repaint_window_id,
            repaint_reason,
        ) {
            crate::log!(
                "ui2-surface-window: rgb render queue failed window={} tex={}\n",
                self.window_id,
                self.tex_id
            );
            return false;
        }
        true
    }

    pub fn render_mandelbrot(
        &self,
        ticks: u64,
        tick_hz: u64,
        repaint_reason: &'static str,
    ) -> bool {
        let repaint_window_id = if is_window_minimized(self.window_id) {
            0
        } else {
            self.window_id
        };
        if !crate::r::io::cabi::queue_render_mandelbrot_to_texture(
            self.tex_id,
            ticks,
            tick_hz,
            repaint_window_id,
            repaint_reason,
        ) {
            crate::log!(
                "ui2-surface-window: mandelbrot render queue failed window={} tex={}\n",
                self.window_id,
                self.tex_id
            );
            return false;
        }
        true
    }

    #[allow(dead_code)]
    pub fn upload_rgba(&self, pixels: &[u8], repaint_reason: &'static str) -> bool {
        let expected = self.width as usize * self.height as usize * 4;
        if pixels.len() != expected {
            crate::log!(
                "ui2-surface-window: upload size mismatch tex={} got={} expected={}\n",
                self.tex_id,
                pixels.len(),
                expected
            );
            return false;
        }
        let repaint_window_id = if is_window_minimized(self.window_id) {
            0
        } else {
            self.window_id
        };
        if !crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
            self.tex_id,
            self.width,
            self.height,
            pixels,
            repaint_window_id,
            repaint_reason,
        ) {
            crate::log!(
                "ui2-surface-window: rgba upload queue failed window={} tex={}\n",
                self.window_id,
                self.tex_id
            );
            return false;
        }
        true
    }

    #[allow(dead_code)]
    pub fn upload_rgba_region(
        &self,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
        repaint_reason: &'static str,
    ) -> bool {
        let expected = width as usize * height as usize * 4;
        if pixels.len() != expected {
            crate::log!(
                "ui2-surface-window: region upload size mismatch tex={} got={} expected={} rect={}x{}@{},{}\n",
                self.tex_id,
                pixels.len(),
                expected,
                width,
                height,
                x,
                y
            );
            return false;
        }
        let repaint_window_id = if is_window_minimized(self.window_id) {
            0
        } else {
            self.window_id
        };
        if !crate::r::io::cabi::queue_texture_rgba_image_region_upload_copy(
            self.tex_id,
            self.width,
            self.height,
            x,
            y,
            width,
            height,
            pixels,
            repaint_window_id,
            repaint_reason,
        ) {
            crate::log!(
                "ui2-surface-window: rgba region upload queue failed window={} tex={} rect={}x{}@{},{}\n",
                self.window_id,
                self.tex_id,
                width,
                height,
                x,
                y
            );
            return false;
        }
        true
    }
}

pub fn window_content_rect_by_id(id: u32) -> Option<Ui2Rect> {
    let state_lock = init_state();
    let state = state_lock.lock();
    state
        .windows
        .iter()
        .find(|window| window.id == id)
        .and_then(|window| window_content_rect(&state, window))
}

pub fn move_window(id: u32, x: f32, y: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        window.state = Ui2WindowStateKind::Normal;
    }
    window.rect.x = x;
    window.rect.y = y;
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = window.rect;
    }
    state.compose_reason = "move-window";
    let noted = note_window_dirty(&mut state, id, "move-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn resize_window(id: u32, w: f32, h: f32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        window.state = Ui2WindowStateKind::Normal;
    }
    let min_extent = ui2_window_min_extent();
    window.rect.w = w.max(min_extent);
    window.rect.h = h.max(min_extent);
    if window.state == Ui2WindowStateKind::Normal {
        window.restore_rect = window.rect;
    }
    state.compose_reason = "resize-window";
    let noted = note_window_dirty(&mut state, id, "resize-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn set_window_title(id: u32, title: &str) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.title == title {
        return true;
    }
    window.title = String::from(title);
    state.compose_reason = "title-window";
    note_window_dirty(&mut state, id, "title-window")
}

pub fn set_window_icon(id: u32, icon_id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.icon_id == icon_id {
        return true;
    }
    window.icon_id = icon_id;
    state.compose_reason = "icon-window";
    note_window_dirty(&mut state, id, "icon-window")
}

pub fn raise_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    window.z = top_z.saturating_add(1);
    state.compose_reason = "raise-window";
    let noted = note_window_dirty(&mut state, id, "raise-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn focus_window(id: u32) -> bool {
    raise_window(id)
}

pub fn set_window_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    set_window_visible_in_state(&mut state, id, visible)
}

pub fn set_window_hit_test_visible(id: u32, visible: bool) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = window_mut(&mut state, id) else {
        return false;
    };
    if window.hit_test_visible == visible {
        return true;
    }
    window.hit_test_visible = visible;
    state.compose_reason = "hit-test-window";
    if !visible {
        clear_window_drag_claims(&mut state, id);
        for cursor in &mut state.cursors {
            if cursor.selected_window_id == id {
                cursor.selected_window_id = 0;
            }
            if cursor.press_window_id == id {
                cursor.press_window_id = 0;
                cursor.press_armed = false;
            }
        }
        let active_selected: Vec<(u32, u32)> = state
            .cursors
            .iter()
            .map(|cursor| (cursor.slot_id, cursor.selected_window_id))
            .collect();
        for window in &mut state.windows {
            let before_len = window.selected_cursor_slots.len();
            if before_len == 0 {
                continue;
            }
            window.selected_cursor_slots.retain(|slot_id| {
                active_selected
                    .iter()
                    .any(|(selected_slot_id, selected_window_id)| {
                        *selected_slot_id == *slot_id && *selected_window_id == window.id
                    })
            });
            if window.selected_cursor_slots.len() != before_len {
                note_selection_change(window);
            }
        }
    }
    let noted = note_window_dirty(&mut state, id, "hit-test-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn close_window(id: u32) -> bool {
    set_window_visible(id, false)
}

pub fn minimize_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    minimize_window_in_state(&mut state, id)
}

pub fn maximize_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    maximize_window_in_state(&mut state, id)
}

pub fn restore_window(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    restore_window_in_state(&mut state, id)
}

pub fn begin_window_move(id: u32) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    let Some(cursor_slot_id) = super::ui2_hid::pick_drag_cursor_slot(&state, &window) else {
        return false;
    };
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == cursor_slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }
    let edge_actions_armed = window_edge_drop_action(&state, cursor.x, cursor.y).is_none();
    clear_window_drag_claims(&mut state, id);
    clear_other_drag_modes_for_slot(&mut state, cursor_slot_id);
    upsert_move_drag(
        &mut state,
        Ui2WindowMoveDrag {
            active: true,
            window_id: id,
            cursor_slot_id,
            grab_dx: cursor.x - window.rect.x,
            grab_dy: cursor.y - window.rect.y,
            edge_actions_armed,
        },
    );
    state.compose_reason = "begin-window-move";
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    if let Some(window_mut) = window_mut(&mut state, id) {
        window_mut.z = top_z.saturating_add(1);
    }
    let noted = note_window_dirty(&mut state, id, "begin-window-move");
    if noted {
        refresh_window_hit_entries(&mut state, id);
    }
    noted
}

pub fn begin_window_resize(id: u32, edge_mask: u32) -> bool {
    let edge_mask = edge_mask
        & (UI2_WINDOW_RESIZE_LEFT
            | UI2_WINDOW_RESIZE_TOP
            | UI2_WINDOW_RESIZE_RIGHT
            | UI2_WINDOW_RESIZE_BOTTOM);
    if !is_valid_resize_edge_mask(edge_mask) {
        return false;
    }

    let state_lock = init_state();
    let mut state = state_lock.lock();
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        return false;
    }
    let Some(cursor_slot_id) = super::ui2_hid::pick_drag_cursor_slot(&state, &window) else {
        return false;
    };
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == cursor_slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }

    super::ui2_hid::begin_window_resize_for_cursor(&mut state, cursor_slot_id, id, edge_mask)
}

fn request_window_composite(id: u32, reason: &'static str) -> bool {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    state.compose_reason = reason;
    note_window_dirty(&mut state, id, reason)
}

pub fn request_window_content_present(id: u32, reason: &'static str) -> bool {
    request_window_composite(id, reason)
}

pub fn request_window_repaint(id: u32, reason: &'static str) -> bool {
    request_window_content_present(id, reason)
}

pub fn request_browser_repaint(reason: &'static str) -> bool {
    let Some(id) = browser_window_id() else {
        return false;
    };
    request_window_content_present(id, reason)
}

pub fn request_full_recompose(reason: &'static str) {
    let state_lock = init_state();
    let mut state = state_lock.lock();
    state.compose_reason = reason;
    for window in &mut state.windows {
        window.dirty = true;
        window.last_reason = reason;
    }
    UI2_DIRTY.store(true, Ordering::Release);
}
