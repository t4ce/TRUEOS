use super::*;

pub(super) type HostedContentId = u32;
pub(super) type UiHostedSurfaceState = trueos_qjs::browser_task::HostedBrowserSurfaceState;
pub(super) type UiHostedInteractiveState = trueos_qjs::browser_task::HostedBrowserInteractiveState;
pub(super) type UiHostedGadgetSnapshot = trueos_qjs::browser_task::HostedBrowserGadgetSnapshot;
pub(super) type UiHostedKeyboardEvent = trueos_qjs::browser_task::HostedKeyboardEvent;

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use embassy_executor::Spawner;
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
const TITLE_ICON_FETCH_POLL_MS: u64 = 8;

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
static UI2_VM1_START_REQUESTED: AtomicBool = AtomicBool::new(false);
static UI2_VM1_RUNNING_RENDERED: AtomicBool = AtomicBool::new(false);

pub(crate) fn request_vm1_resume() {
    UI2_VM1_START_REQUESTED.store(true, Ordering::Release);
    queue_hosted_container_sync();
}

fn sync_vm_window_runtime_state(state: &mut Ui2State) {
    let hv_status = crate::hv::status();
    let vm_running = hv_status.vm1_running || hv_status.vm1_starting;
    let previous = UI2_VM1_RUNNING_RENDERED.swap(vm_running, Ordering::AcqRel);
    if previous == vm_running {
        return;
    }

    if vm_running {
        state.compose_reason = "vm-window-runtime-state";
        for window_id in state
            .windows
            .iter()
            .filter(|window| window.vm_origin_hint)
            .map(|window| window.id)
            .collect::<Vec<_>>()
        {
            let _ = note_window_dirty(state, window_id, "vm-window-runtime-state");
        }
        return;
    }

    let removed = teardown_stopped_vm_windows_in_state(state);
    if removed != 0 {
        crate::hv::hvlogf(format_args!(
            "ui2: removed {} stale vm window(s) after vm stop",
            removed
        ));
    }
}

fn service_vm_resume_request(spawner: &Spawner) {
    if !UI2_VM1_START_REQUESTED.swap(false, Ordering::AcqRel) {
        return;
    }

    let hv_status = crate::hv::status();
    if hv_status.vm1_running || hv_status.vm1_starting {
        return;
    }

    match crate::hv::restore_snapshot(0) {
        Ok(bytes) => crate::hv::hvlogf(format_args!(
            "ui2: vm window resume restored snapshot bytes={}",
            bytes
        )),
        Err(crate::hv::RestoreError::MissingFile) => {}
        Err(err) => {
            crate::hv::hvlogf(format_args!("ui2: vm window resume restore failed: {:?}", err))
        }
    }

    if let Err(err) = crate::hv::start(0, spawner, &crate::shell2::UI2_SHELL_BACKEND, None) {
        crate::hv::hvlogf(format_args!("ui2: vm window resume start failed: {:?}", err));
    }
}

pub(super) trait UiHostedSurfaceProvider {
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

fn bytes_look_like_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[0..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}

fn bytes_look_like_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF
}

fn bytes_look_like_svg(bytes: &[u8]) -> bool {
    let mut start = 0usize;
    while start < bytes.len() && matches!(bytes[start], b' ' | b'\t' | b'\r' | b'\n') {
        start += 1;
    }
    let trimmed = &bytes[start..];
    trimmed.starts_with(b"<svg")
        || trimmed.starts_with(b"<?xml")
        || trimmed.windows(4).any(|window| window == b"<svg")
}

fn bytes_look_like_ico(bytes: &[u8]) -> bool {
    bytes.len() >= 6
        && bytes[0] == 0
        && bytes[1] == 0
        && bytes[2] == 1
        && bytes[3] == 0
        && (u16::from_le_bytes([bytes[4], bytes[5]]) as usize) > 0
}

fn ico_dir_dim(byte: u8) -> u32 {
    if byte == 0 { 256 } else { byte as u32 }
}

fn ico_best_png_payload(bytes: &[u8]) -> Option<&[u8]> {
    if !bytes_look_like_ico(bytes) {
        return None;
    }
    let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
    let dir_bytes = count.checked_mul(16)?;
    let table_end = 6usize.checked_add(dir_bytes)?;
    if bytes.len() < table_end {
        return None;
    }

    let mut best: Option<(u32, u32, usize, usize)> = None;
    for index in 0..count {
        let entry = 6 + (index * 16);
        let width = ico_dir_dim(bytes[entry]);
        let height = ico_dir_dim(bytes[entry + 1]);
        let size = u32::from_le_bytes([
            bytes[entry + 8],
            bytes[entry + 9],
            bytes[entry + 10],
            bytes[entry + 11],
        ]) as usize;
        let offset = u32::from_le_bytes([
            bytes[entry + 12],
            bytes[entry + 13],
            bytes[entry + 14],
            bytes[entry + 15],
        ]) as usize;
        let end = offset.checked_add(size)?;
        if size < 8 || end > bytes.len() {
            continue;
        }
        let payload = &bytes[offset..end];
        if !bytes_look_like_png(payload) {
            continue;
        }

        let side = width.max(height);
        let side_delta = side.abs_diff(16);
        match best {
            Some((best_delta, best_side, _, _))
                if side_delta > best_delta || (side_delta == best_delta && side >= best_side) =>
            {
                continue;
            }
            _ => {
                best = Some((side_delta, side, offset, end));
            }
        }
    }

    best.map(|(_, _, offset, end)| &bytes[offset..end])
}

fn fetch_url_bytes_started(url: &str) -> Result<u32, i32> {
    if url.starts_with('/') {
        trueos_qjs::async_fs::start_read_file(url.as_bytes())
    } else {
        trueos_qjs::async_fs::start_net_fetch_bytes(url.as_bytes())
    }
}

async fn wait_for_fetch_bytes(op_id: u32) -> Result<Vec<u8>, i32> {
    loop {
        let rc_or_done = trueos_qjs::async_fs::result_len(op_id);
        if rc_or_done == trueos_qjs::async_fs::FS_ERR_NOT_FOUND as isize {
            Timer::after(EmbassyDuration::from_millis(TITLE_ICON_FETCH_POLL_MS)).await;
            continue;
        }
        if rc_or_done < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            return Err(rc_or_done as i32);
        }
        let mut bytes = vec![0u8; rc_or_done as usize];
        let got = trueos_qjs::async_fs::read_result(op_id, bytes.as_mut_ptr(), bytes.len());
        if got < 0 {
            let _ = trueos_qjs::async_fs::discard(op_id);
            return Err(got as i32);
        }
        bytes.truncate(got as usize);
        return Ok(bytes);
    }
}

async fn wait_for_texture_ready(tex_id: u32) -> Result<(), i32> {
    loop {
        let status = crate::r::io::cabi::trueos_cabi_gfx_texture_status(tex_id);
        if status == 2 {
            return Ok(());
        }
        if status < 0 {
            return Err(status);
        }
        Timer::after(EmbassyDuration::from_millis(TITLE_ICON_FETCH_POLL_MS)).await;
    }
}

fn sync_browser_title_metadata(state: &mut Ui2State, window_id: u32, title: &str) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    let changed = {
        let Some(window) = window_mut(state, window_id) else {
            return;
        };
        if window.title == title {
            false
        } else {
            window.title = String::from(title);
            true
        }
    };
    if !changed {
        return;
    }
    state.compose_reason = "browser-title";
    let _ = note_window_dirty(state, window_id, "browser-title");
}

fn schedule_browser_title_icon_fetch(
    state: &mut Ui2State,
    spawner: &Spawner,
    window_id: u32,
    favicon_url: &str,
) {
    let favicon_url = favicon_url.trim();
    let (changed, load_seq, url) = {
        let Some(window) = window_mut(state, window_id) else {
            return;
        };
        if window.title_icon_url == favicon_url {
            (false, 0, String::new())
        } else {
            window.title_icon_url = String::from(favicon_url);
            window.title_icon_tex_id = 0;
            window.title_icon_load_seq = window.title_icon_load_seq.wrapping_add(1).max(1);
            (true, window.title_icon_load_seq, window.title_icon_url.clone())
        }
    };
    if !changed {
        return;
    }
    state.compose_reason = "browser-title-icon";
    let _ = note_window_dirty(state, window_id, "browser-title-icon");
    if favicon_url.is_empty() {
        return;
    }
    let tex_id = window_title_icon_tex_id(window_id);
    if let Ok(token) = browser_title_icon_fetch_task(window_id, load_seq, tex_id, url) {
        spawner.spawn(token);
    }
}

fn sync_hosted_browser_window_metadata(state: &mut Ui2State, spawner: &Spawner) {
    let browser_windows: Vec<(u32, u32)> = state
        .windows
        .iter()
        .filter(|window| window.kind == Ui2WindowKind::HostedBrowser)
        .map(|window| (window.id, window_browser_instance_id(window)))
        .collect();
    for (window_id, browser_instance_id) in browser_windows {
        let Some(parse_result) =
            trueos_qjs::browser_task::latest_parse_result_for_browser(browser_instance_id)
        else {
            continue;
        };
        if !parse_result.ok {
            continue;
        }
        sync_browser_title_metadata(state, window_id, parse_result.title.as_str());
        schedule_browser_title_icon_fetch(
            state,
            spawner,
            window_id,
            parse_result.favicon_url.as_str(),
        );
    }
}

#[embassy_executor::task(pool_size = 8)]
async fn browser_title_icon_fetch_task(window_id: u32, load_seq: u32, tex_id: u32, url: String) {
    let op_id = match fetch_url_bytes_started(url.as_str()) {
        Ok(op_id) => op_id,
        Err(_) => return,
    };
    let bytes = match wait_for_fetch_bytes(op_id).await {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let ico_png = ico_best_png_payload(bytes.as_slice());
    let upload_bytes = ico_png.unwrap_or(bytes.as_slice());

    let upload_rc = if bytes_look_like_svg(upload_bytes) {
        unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_upload_texture_svg_async(
                tex_id,
                upload_bytes.as_ptr(),
                upload_bytes.len(),
            )
        }
    } else if bytes_look_like_jpeg(upload_bytes) {
        unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_upload_texture_jpeg_async(
                tex_id,
                upload_bytes.as_ptr(),
                upload_bytes.len(),
            )
        }
    } else if bytes_look_like_png(upload_bytes) {
        unsafe {
            crate::r::io::cabi::trueos_cabi_gfx_upload_texture_png_async(
                tex_id,
                upload_bytes.as_ptr(),
                upload_bytes.len(),
            )
        }
    } else {
        return;
    };
    if upload_rc != 0 || wait_for_texture_ready(tex_id).await.is_err() {
        return;
    }

    let state_lock = init_state();
    let mut state = state_lock.lock();
    let applied = {
        let Some(window) = window_mut(&mut state, window_id) else {
            return;
        };
        if window.title_icon_load_seq != load_seq || window.title_icon_url != url {
            false
        } else {
            window.title_icon_tex_id = tex_id;
            true
        }
    };
    if !applied {
        return;
    }
    state.compose_reason = "browser-title-icon-ready";
    let _ = note_window_dirty(&mut state, window_id, "browser-title-icon-ready");
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
            let renderable = window_content_participates_in_composition(window);
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

    let spawner: Spawner = unsafe { Spawner::for_current_executor().await };
    crate::log!("boot-probe: ui2-hosted task start ms={}\n", boot_probe_ms());
    queue_hosted_container_sync();

    loop {
        let queued = UI2_HOSTED_CONTAINER_SYNC_QUEUED.swap(false, Ordering::AcqRel);
        let pending_after;

        {
            let state_lock = init_state();
            let mut state = state_lock.lock();
            sync_vm_window_runtime_state(&mut state);
            sync_hosted_browser_window_metadata(&mut state, &spawner);
            if queued {
                sync_pending_window_containers(&mut state);
            }
            pending_after = state
                .windows
                .iter()
                .any(|window| window.container_sync_needed);
        }

        if pending_after {
            queue_hosted_container_sync();
        }

        service_vm_resume_request(&spawner);
        Timer::after(EmbassyDuration::from_millis(if queued { 4 } else { 12 })).await;
    }
}
