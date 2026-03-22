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
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedSurface,
        title,
        rect,
        z,
        alpha,
    );
    state.compose_reason = "create-window";
    refresh_window_hit_entries(&mut state, id);
    UI2_DIRTY.store(true, Ordering::Release);
    id
}

pub fn create_empty_ui2_window(title: &str, content_rect: Ui2Rect, z: i16, alpha: u8) -> u32 {
    let rect = window_rect_for_content(Ui2WindowDecorationMode::System, content_rect);
    create_window(title, rect, z, alpha)
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
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedBrowser,
        title,
        rect,
        z,
        alpha,
    );
    if let Some(window) = window_mut(&mut state, id) {
        window.browser_instance_id = browser_instance_id;
        window.content_tex_id = tex_id;
        window.content_tex_blend = true;
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
    let id = alloc_window(
        &mut state,
        Ui2WindowKind::HostedSurface,
        title,
        rect,
        z,
        alpha,
    );
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
    state.compose_reason = "texture-window";
    let noted = note_window_dirty(&mut state, id, "texture-window");
    if noted {
        let _ = note_window_viewport_sync_needed(&mut state, id);
    }
    noted
}

#[allow(dead_code)]
pub fn create_texture_window(
    title: &str,
    rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    create_hosted_surface_window(title, rect, z, alpha, tex_id, blend_enabled)
}

#[allow(dead_code)]
pub fn create_texture_content_window(
    title: &str,
    content_rect: Ui2Rect,
    z: i16,
    alpha: u8,
    tex_id: u32,
    blend_enabled: bool,
) -> u32 {
    create_hosted_surface_content_window(title, content_rect, z, alpha, tex_id, blend_enabled)
}

#[allow(dead_code)]
pub fn set_window_texture_content(id: u32, tex_id: u32, blend_enabled: bool) -> bool {
    set_window_hosted_surface_content(id, tex_id, blend_enabled)
}

pub struct Ui2SurfaceWindow {
    window_id: u32,
    tex_id: u32,
    width: u32,
    height: u32,
}

impl Ui2SurfaceWindow {
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

        let window_id = create_hosted_surface_content_window(
            title,
            Ui2Rect {
                x: content_rect.x,
                y: content_rect.y,
                w: width as f32,
                h: height as f32,
            },
            z,
            alpha,
            tex_id,
            blend_enabled,
        );
        Some(Self {
            window_id,
            tex_id,
            width,
            height,
        })
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
        if state.move_drag.window_id == id {
            state.move_drag = Ui2WindowMoveDrag::default();
        }
        if state.resize_drag.window_id == id {
            state.resize_drag = Ui2WindowResizeDrag::default();
        }
        if state.scroll_drag.window_id == id {
            state.scroll_drag = Ui2WindowScrollDrag::default();
        }
        if state.scroll_pan_drag.window_id == id {
            state.scroll_pan_drag = Ui2WindowScrollPanDrag::default();
        }
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
    state.move_drag = Ui2WindowMoveDrag {
        active: true,
        window_id: id,
        cursor_slot_id,
        grab_dx: cursor.x - window.rect.x,
        grab_dy: cursor.y - window.rect.y,
        edge_actions_armed: window_edge_drop_action(&state, cursor.x, cursor.y).is_none(),
    };
    state.resize_drag = Ui2WindowResizeDrag::default();
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
