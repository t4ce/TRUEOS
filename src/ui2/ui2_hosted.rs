use super::*;

pub(super) type HostedContentId = u32;
pub(super) type UiHostedSurfaceState = trueos_qjs::browser_task::HostedBrowserSurfaceState;
pub(super) type UiHostedInteractiveState = trueos_qjs::browser_task::HostedBrowserInteractiveState;
pub(super) type UiHostedGadgetSnapshot = trueos_qjs::browser_task::HostedBrowserGadgetSnapshot;
pub(super) type UiHostedKeyboardEvent = trueos_qjs::browser_task::HostedKeyboardEvent;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

pub(super) const PRIMARY_HOSTED_CONTENT_ID: HostedContentId = 1;
pub(super) const HOSTED_KEYBOARD_MOD_SHIFT: u8 =
    trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_SHIFT;
pub(super) const HOSTED_KEYBOARD_MOD_CTRL: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_CTRL;
pub(super) const HOSTED_KEYBOARD_MOD_ALT: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_ALT;
pub(super) const HOSTED_KEYBOARD_MOD_META: u8 = trueos_qjs::browser_task::HOSTED_KEYBOARD_MOD_META;

const UI2_BROWSER_ADAPTER_ENABLED: bool = true;
pub(super) const HOSTED_BROWSER_DIRTY_CONTENT: u32 = 1 << 0;
pub(super) const HOSTED_BROWSER_DIRTY_INTERACTIVE: u32 = 1 << 1;

#[derive(Copy, Clone, Debug, Default)]
pub(super) struct HostedBrowserDirtyMask {
    pub content: u64,
    pub interactive: u64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct UiHostedBrowserSnapshot {
    pub surface: UiHostedSurfaceState,
    pub interactive: UiHostedInteractiveState,
    pub gadget_snapshot: UiHostedGadgetSnapshot,
}

#[derive(Copy, Clone, Debug, Default)]
struct HostedBrowserFactorySignalState {
    latest_mask: u64,
    seq: u32,
    taken_seq: u32,
}

static HOSTED_BROWSER_FACTORY_SIGNAL: Mutex<HostedBrowserFactorySignalState> =
    Mutex::new(HostedBrowserFactorySignalState {
        latest_mask: 0,
        seq: 0,
        taken_seq: 0,
    });
static UI2_HOSTED_SYNC_STARTED: AtomicBool = AtomicBool::new(false);
static UI2_HOSTED_CONTAINER_SYNC_QUEUED: AtomicBool = AtomicBool::new(false);
static HOSTED_BROWSER_DIRTY_CONTENT_MASK: AtomicU64 = AtomicU64::new(0);
static HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK: AtomicU64 = AtomicU64::new(0);

pub(super) trait UiHostedSurfaceProvider {
    fn surface_seq(&self, content_id: HostedContentId) -> u32;
    fn interactive_seq(&self, content_id: HostedContentId) -> u32;
    fn surface_state(&self, content_id: HostedContentId) -> UiHostedSurfaceState;
    fn interactive_state(&self, content_id: HostedContentId) -> UiHostedInteractiveState;
    fn gadget_snapshot(&self, content_id: HostedContentId) -> UiHostedGadgetSnapshot;
}

pub(super) trait UiHostedViewportSink {
    fn set_viewport(
        &self,
        content_id: HostedContentId,
        viewport_width: u32,
        viewport_height: u32,
        content_x: i32,
        content_y: i32,
        content_width: u32,
        content_height: u32,
    ) -> bool;
}

pub(super) enum UiHostedInput<'a> {
    Scroll { scroll_x: u32, scroll_y: u32 },
    Keyboard { events: &'a [UiHostedKeyboardEvent] },
}

pub(super) trait UiHostedInputSink {
    fn send_input(&self, content_id: HostedContentId, input: UiHostedInput<'_>) -> bool;
}

pub(super) trait UiHostedWindowBinder {
    fn bind_window(&self, content_id: HostedContentId, window_id: u32) -> bool;
    fn primary_window_id(&self) -> u32;
    fn window_id_for_content(&self, content_id: HostedContentId) -> u32;
}

struct BrowserUiHostedAdapter;

impl UiHostedSurfaceProvider for BrowserUiHostedAdapter {
    fn surface_seq(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::hosted_surface_seq_for_browser(content_id)
    }

    fn interactive_seq(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::hosted_interactive_seq_for_browser(content_id)
    }

    fn surface_state(&self, content_id: HostedContentId) -> UiHostedSurfaceState {
        trueos_qjs::browser_task::hosted_surface_state_for_browser(content_id)
    }

    fn interactive_state(&self, content_id: HostedContentId) -> UiHostedInteractiveState {
        trueos_qjs::browser_task::hosted_interactive_state_for_browser(content_id)
    }

    fn gadget_snapshot(&self, content_id: HostedContentId) -> UiHostedGadgetSnapshot {
        trueos_qjs::browser_task::hosted_gadget_snapshot_for_browser(content_id)
    }
}

impl UiHostedViewportSink for BrowserUiHostedAdapter {
    fn set_viewport(
        &self,
        content_id: HostedContentId,
        viewport_width: u32,
        viewport_height: u32,
        content_x: i32,
        content_y: i32,
        content_width: u32,
        content_height: u32,
    ) -> bool {
        trueos_qjs::browser_task::set_hosted_viewport_for_browser(
            content_id,
            viewport_width,
            viewport_height,
            content_x,
            content_y,
            content_width,
            content_height,
        )
    }
}

impl UiHostedInputSink for BrowserUiHostedAdapter {
    fn send_input(&self, content_id: HostedContentId, input: UiHostedInput<'_>) -> bool {
        match input {
            UiHostedInput::Scroll { scroll_x, scroll_y } => {
                trueos_qjs::browser_task::set_hosted_scroll_for_browser(
                    content_id, scroll_x, scroll_y,
                )
            }
            UiHostedInput::Keyboard { events } => {
                let window_id =
                    trueos_qjs::browser_task::browser_window_id_for_instance(content_id);
                if window_id == 0 {
                    return false;
                }
                trueos_qjs::browser_task::queue_hosted_keyboard_events(window_id, events)
            }
        }
    }
}

impl UiHostedWindowBinder for BrowserUiHostedAdapter {
    fn bind_window(&self, content_id: HostedContentId, window_id: u32) -> bool {
        trueos_qjs::browser_task::bind_browser_window_to_instance(content_id, window_id)
    }

    fn primary_window_id(&self) -> u32 {
        self.window_id_for_content(PRIMARY_HOSTED_CONTENT_ID)
    }

    fn window_id_for_content(&self, content_id: HostedContentId) -> u32 {
        trueos_qjs::browser_task::browser_window_id_for_instance(content_id)
    }
}

#[inline]
fn hosted_adapter() -> BrowserUiHostedAdapter {
    BrowserUiHostedAdapter
}

#[inline]
pub(super) fn hosted_bind_window(content_id: HostedContentId, window_id: u32) -> bool {
    if !UI2_BROWSER_ADAPTER_ENABLED {
        return false;
    }
    hosted_adapter().bind_window(content_id, window_id)
}

#[inline]
pub(super) fn hosted_primary_window_id() -> u32 {
    hosted_adapter().primary_window_id()
}

#[inline]
pub(super) fn hosted_window_id_for_content(content_id: HostedContentId) -> u32 {
    hosted_adapter().window_id_for_content(content_id)
}

#[inline]
pub(super) fn hosted_surface_state(content_id: HostedContentId) -> UiHostedSurfaceState {
    hosted_adapter().surface_state(content_id)
}

#[inline]
pub(super) fn hosted_surface_seq(content_id: HostedContentId) -> u32 {
    hosted_adapter().surface_seq(content_id)
}

#[inline]
pub(super) fn hosted_interactive_state(content_id: HostedContentId) -> UiHostedInteractiveState {
    hosted_adapter().interactive_state(content_id)
}

#[inline]
pub(super) fn hosted_interactive_seq(content_id: HostedContentId) -> u32 {
    hosted_adapter().interactive_seq(content_id)
}

#[inline]
pub(super) fn hosted_gadget_snapshot(content_id: HostedContentId) -> UiHostedGadgetSnapshot {
    hosted_adapter().gadget_snapshot(content_id)
}

pub(super) fn hosted_browser_snapshot(content_id: HostedContentId) -> UiHostedBrowserSnapshot {
    let mut surface = hosted_surface_state(content_id);
    if surface.content_width == 0 {
        surface.content_width = surface.viewport_width.max(1);
    }
    if surface.content_height == 0 {
        surface.content_height = surface.viewport_height.max(1);
    }
    UiHostedBrowserSnapshot {
        surface,
        interactive: hosted_interactive_state(content_id),
        gadget_snapshot: hosted_gadget_snapshot(content_id),
    }
}

#[inline]
pub(super) fn hosted_set_viewport(
    content_id: HostedContentId,
    viewport_width: u32,
    viewport_height: u32,
    content_x: i32,
    content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    if !UI2_BROWSER_ADAPTER_ENABLED {
        let _ = (
            content_id,
            viewport_width,
            viewport_height,
            content_x,
            content_y,
            content_width,
            content_height,
        );
        return false;
    }
    hosted_adapter().set_viewport(
        content_id,
        viewport_width,
        viewport_height,
        content_x,
        content_y,
        content_width,
        content_height,
    )
}

#[inline]
pub(super) fn hosted_set_scroll(content_id: HostedContentId, scroll_x: u32, scroll_y: u32) -> bool {
    if !UI2_BROWSER_ADAPTER_ENABLED {
        let _ = (content_id, scroll_x, scroll_y);
        return false;
    }
    hosted_adapter().send_input(content_id, UiHostedInput::Scroll { scroll_x, scroll_y })
}

#[inline]
pub(super) fn hosted_set_scroll_y(content_id: HostedContentId, scroll_y: u32) -> bool {
    let current_x = hosted_surface_state(content_id).scroll_x;
    hosted_set_scroll(content_id, current_x, scroll_y)
}

#[inline]
pub(super) fn hosted_queue_keyboard_events(
    content_id: HostedContentId,
    events: &[UiHostedKeyboardEvent],
) -> bool {
    if !UI2_BROWSER_ADAPTER_ENABLED {
        let _ = (content_id, events);
        return false;
    }
    hosted_adapter().send_input(content_id, UiHostedInput::Keyboard { events })
}

pub(crate) fn signal_hosted_browser_factory_mask(mask: u64) {
    let mut signal = HOSTED_BROWSER_FACTORY_SIGNAL.lock();
    signal.latest_mask = mask;
    signal.seq = signal.seq.wrapping_add(1).max(1);
}

#[inline]
pub(super) fn queue_hosted_container_sync() {
    UI2_HOSTED_CONTAINER_SYNC_QUEUED.store(true, Ordering::Release);
}

#[inline]
fn hosted_browser_bit(content_id: HostedContentId) -> Option<u64> {
    if !(1..=64).contains(&content_id) {
        return None;
    }
    Some(1u64 << content_id.saturating_sub(1))
}

pub(crate) fn signal_hosted_browser_dirty(content_id: HostedContentId, flags: u32) {
    let Some(bit) = hosted_browser_bit(content_id) else {
        return;
    };
    if (flags & HOSTED_BROWSER_DIRTY_CONTENT) != 0 {
        HOSTED_BROWSER_DIRTY_CONTENT_MASK.fetch_or(bit, Ordering::Release);
    }
    if (flags & HOSTED_BROWSER_DIRTY_INTERACTIVE) != 0 {
        HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK.fetch_or(bit, Ordering::Release);
    }
}

pub(super) fn take_hosted_browser_dirty_mask() -> HostedBrowserDirtyMask {
    HostedBrowserDirtyMask {
        content: HOSTED_BROWSER_DIRTY_CONTENT_MASK.swap(0, Ordering::AcqRel),
        interactive: HOSTED_BROWSER_DIRTY_INTERACTIVE_MASK.swap(0, Ordering::AcqRel),
    }
}

#[inline]
pub(super) fn take_hosted_browser_factory_mask() -> Option<u64> {
    let mut signal = HOSTED_BROWSER_FACTORY_SIGNAL.lock();
    if signal.seq == signal.taken_seq {
        return None;
    }
    signal.taken_seq = signal.seq;
    Some(signal.latest_mask)
}

fn hosted_browser_factory_content_rect_for_view(
    view_w: u32,
    view_h: u32,
    slot: u32,
    total: u32,
) -> Ui2Rect {
    let cols = if total >= 2 { 2u32 } else { 1u32 };
    let rows = total.div_ceil(cols).max(1);
    let margin_x = 48.0f32;
    let margin_y = 84.0f32;
    let gutter = 18.0f32;
    let bottom_margin = 36.0f32;
    let usable_w = (view_w as f32) - margin_x * 2.0 - gutter * (cols.saturating_sub(1) as f32);
    let usable_h =
        (view_h as f32) - margin_y - bottom_margin - gutter * (rows.saturating_sub(1) as f32);
    let width = (usable_w / cols as f32).clamp(520.0, 960.0);
    let height = (usable_h / rows as f32).clamp(320.0, 640.0);
    let col = slot % cols;
    let row = slot / cols;
    Ui2Rect::new(
        margin_x + col as f32 * (width + gutter),
        margin_y + row as f32 * (height + gutter),
        width,
        height,
    )
}

pub(super) fn sync_hosted_browser_factory_windows(active_mask: u64) -> usize {
    if active_mask == 0 {
        return 0;
    }

    let active_ids: Vec<u32> = (1..=trueos_qjs::browser_task::MAX_BROWSER_INSTANCE_ID)
        .filter(|browser_instance_id| {
            let bit = 1u64 << browser_instance_id.saturating_sub(1);
            (active_mask & bit) != 0
        })
        .collect();
    if active_ids.is_empty() {
        return 0;
    }

    let (view_w, view_h) = {
        let state_lock = init_state();
        let state = state_lock.lock();
        (state.view_w, state.view_h)
    };

    let total = active_ids.len() as u32;
    let mut created = 0usize;
    for (slot, browser_instance_id) in active_ids.into_iter().enumerate() {
        if hosted_window_id_for_content(browser_instance_id) != 0 {
            continue;
        }
        let title = format!("Truesurfer {}", browser_instance_id);
        let content_rect =
            hosted_browser_factory_content_rect_for_view(view_w, view_h, slot as u32, total);
        let tex_id =
            trueos_qjs::browser_task::render_tex_id_for_browser_instance(browser_instance_id);
        let window_id = create_hosted_browser_content_window(
            title.as_str(),
            content_rect,
            40i16.saturating_add(slot as i16),
            255,
            browser_instance_id,
            tex_id,
        );
        crate::log!(
            "ui2: hosted-browser-factory window={} browser={} tex={} slot={} total={}\n",
            window_id,
            browser_instance_id,
            tex_id,
            slot,
            total
        );
        created = created.saturating_add(1);
    }
    created
}

#[inline]
pub(super) fn snap_browser_content_rect(content: Ui2Rect) -> (i32, i32, u32, u32) {
    (
        libm::roundf(content.x) as i32,
        libm::roundf(content.y) as i32,
        round_to_u32(content.w, 1),
        round_to_u32(content.h, 1),
    )
}

pub(super) fn queue_browser_window_viewport(content_id: HostedContentId, content: Ui2Rect) -> bool {
    let (content_x, content_y, viewport_w, viewport_h) = snap_browser_content_rect(content);
    hosted_set_viewport(
        content_id, viewport_w, viewport_h, content_x, content_y, viewport_w, viewport_h,
    )
}

fn ensure_window_texture_size(
    tex_id: u32,
    width: u32,
    height: u32,
    repaint_window_id: u32,
    repaint_reason: &'static str,
) -> bool {
    if tex_id == 0 || width == 0 || height == 0 {
        return false;
    }

    let mut existing_w = 0u32;
    let mut existing_h = 0u32;
    let already_sized = unsafe {
        crate::r::io::cabi::trueos_cabi_gfx_texture_dimensions(
            tex_id,
            &mut existing_w as *mut u32,
            &mut existing_h as *mut u32,
        ) == 0
    } && existing_w == width
        && existing_h == height;
    if already_sized {
        return true;
    }

    let pixels =
        alloc::vec![0u8; (width as usize).saturating_mul(height as usize).saturating_mul(4)];
    crate::r::io::cabi::queue_texture_rgba_image_upload_copy(
        tex_id,
        width,
        height,
        pixels.as_slice(),
        repaint_window_id,
        repaint_reason,
    )
}

fn sync_window_container(
    window_id: u32,
    renderable: bool,
    kind: Ui2WindowKind,
    content_id: HostedContentId,
    content_tex_id: u32,
    content: Option<Ui2Rect>,
) -> bool {
    if !renderable {
        return true;
    }
    match kind {
        Ui2WindowKind::HostedBrowser => {
            let Some(content) = content else {
                return true;
            };
            let (_, _, viewport_w, viewport_h) = snap_browser_content_rect(content);
            if !ensure_window_texture_size(
                content_tex_id,
                viewport_w,
                viewport_h,
                window_id,
                "browser-tab-texture-resize",
            ) {
                return false;
            }
            queue_browser_window_viewport(content_id, content)
        }
        Ui2WindowKind::HostedSurface => {
            let Some(content) = content else {
                return true;
            };
            if content_id == 0 {
                return true;
            }
            let (content_x, content_y, viewport_w, viewport_h) = snap_browser_content_rect(content);
            let snapshot = hosted_surface_state(content_id);
            hosted_set_viewport(
                content_id,
                viewport_w,
                viewport_h,
                content_x,
                content_y,
                snapshot.content_width.max(viewport_w),
                snapshot.content_height.max(viewport_h),
            )
        }
        Ui2WindowKind::Hosted3d => true,
    }
}

pub(super) fn sync_pending_window_containers(state: &mut Ui2State) {
    let pending: Vec<(u32, bool, Ui2WindowKind, HostedContentId, u32, Option<Ui2Rect>)> = state
        .windows
        .iter()
        .filter(|window| window.container_sync_needed)
        .map(|window| {
            let renderable = window_is_renderable(window);
            let content = if renderable {
                window_content_rect(state, window)
            } else {
                None
            };
            (
                window.id,
                renderable,
                window.kind,
                window_hosted_content_id(window),
                window.content_tex_id,
                content,
            )
        })
        .collect();

    let mut synced_ids = Vec::new();
    for (id, renderable, kind, content_id, content_tex_id, content) in pending {
        if sync_window_container(id, renderable, kind, content_id, content_tex_id, content) {
            synced_ids.push(id);
        }
    }
    for id in synced_ids {
        if let Some(window) = window_mut(state, id) {
            window.container_sync_needed = false;
        }
    }
}

#[embassy_executor::task]
pub async fn ui2_hosted_task() {
    if UI2_HOSTED_SYNC_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }

    crate::log!("boot-probe: ui2-hosted task start ms={}\n", boot_probe_ms());
    queue_hosted_container_sync();

    loop {
        let queued = UI2_HOSTED_CONTAINER_SYNC_QUEUED.swap(false, Ordering::AcqRel);
        let mut pending_after = false;

        if queued {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            sync_pending_window_containers(&mut state);
            pending_after = state
                .windows
                .iter()
                .any(|window| window.container_sync_needed);
        }

        if pending_after {
            queue_hosted_container_sync();
        }

        Timer::after(EmbassyDuration::from_millis(if queued { 4 } else { 12 })).await;
    }
}
