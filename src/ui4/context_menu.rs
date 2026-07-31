//! One-shot, OS-rendered context menus for trusted UI4 producers.
//!
//! A request contains the complete menu for one invocation. UI4 retains it
//! only until selection, dismissal, replacement, or owner/window teardown.
//! There is deliberately no persistent menu registry and no application-owned
//! slot-4 surface.

use alloc::{
    collections::VecDeque,
    string::{String, ToString},
    vec::Vec,
};
use core::sync::atomic::{AtomicU64, Ordering};

use spin::Mutex;

use super::{Ui4CursorSource, Ui4VisualRect, WindowId, WindowOwner};

pub(crate) const MAX_CONTEXT_MENU_ENTRIES: usize = 16;
pub(super) const MENU_OFFSET_PX: u32 = 14;
pub(super) const MENU_WIDTH_PX: u32 = 196;
pub(super) const MENU_BORDER_PX: u32 = 2;
pub(super) const MENU_ROW_HEIGHT_PX: u32 = 24;
pub(super) const MENU_TEXT_INSET_PX: u32 = 10;
pub(super) const MENU_RENDER_LABEL_CHARS: usize = 20;

static NEXT_MENU_SERIAL: AtomicU64 = AtomicU64::new(1);
static ACTIVE_MENU: Mutex<Option<ActiveContextMenu>> = Mutex::new(None);
static PENDING_CALLBACKS: Mutex<VecDeque<PendingCallback>> = Mutex::new(VecDeque::new());

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ContextMenuEntry {
    pub(crate) label: String,
    pub(crate) action_id: u32,
    pub(crate) enabled: bool,
}

impl ContextMenuEntry {
    pub(crate) fn action(label: impl ToString, action_id: u32) -> Self {
        Self {
            label: label.to_string(),
            action_id,
            enabled: true,
        }
    }

    pub(crate) fn disabled(label: impl ToString) -> Self {
        Self {
            label: label.to_string(),
            action_id: 0,
            enabled: false,
        }
    }
}

pub(crate) type ContextMenuCallback = fn(ContextMenuResult);

pub(crate) struct ContextMenuRequest {
    pub(crate) entries: Vec<ContextMenuEntry>,
    pub(crate) context: u64,
    pub(crate) callback: ContextMenuCallback,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextMenuCloseReason {
    Selected,
    Dismissed,
    Replaced,
    OwnerReleased,
    WindowClosed,
}

pub(crate) struct ContextMenuResult {
    pub(crate) source: Ui4CursorSource,
    pub(crate) owner: WindowOwner,
    pub(crate) window: WindowId,
    pub(crate) anchor: (u32, u32),
    pub(crate) context: u64,
    pub(crate) selected_action: Option<u32>,
    pub(crate) reason: ContextMenuCloseReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ContextMenuError {
    Empty,
    TooManyEntries,
    EmptyLabel,
    NotFocused,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContextMenuVisualEntry {
    pub(super) label: String,
    pub(super) enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContextMenuVisual {
    pub(super) anchor: (u32, u32),
    pub(super) color: crate::graphics::primitives::Rgba8,
    pub(super) entries: Vec<ContextMenuVisualEntry>,
    pub(super) hovered: Option<usize>,
}

struct ActiveContextMenu {
    serial: u64,
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
    anchor: (u32, u32),
    color: crate::graphics::primitives::Rgba8,
    entries: Vec<ContextMenuEntry>,
    context: u64,
    callback: ContextMenuCallback,
    hovered: Option<usize>,
    pressed: Option<usize>,
}

struct PendingCallback {
    callback: ContextMenuCallback,
    result: ContextMenuResult,
}

pub(super) fn validate_request(request: &ContextMenuRequest) -> Result<(), ContextMenuError> {
    if request.entries.is_empty() {
        return Err(ContextMenuError::Empty);
    }
    if request.entries.len() > MAX_CONTEXT_MENU_ENTRIES {
        return Err(ContextMenuError::TooManyEntries);
    }
    if request
        .entries
        .iter()
        .any(|entry| entry.label.trim().is_empty())
    {
        return Err(ContextMenuError::EmptyLabel);
    }
    Ok(())
}

pub(super) fn open(
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
    anchor: (u32, u32),
    color: crate::graphics::primitives::Rgba8,
    request: ContextMenuRequest,
) -> Result<(), ContextMenuError> {
    validate_request(&request)?;
    let serial = next_menu_serial();
    let entry_count = request.entries.len();
    let previous = ACTIVE_MENU.lock().replace(ActiveContextMenu {
        serial,
        source,
        owner,
        window,
        anchor,
        color,
        entries: request.entries,
        context: request.context,
        callback: request.callback,
        hovered: None,
        pressed: None,
    });
    if let Some(previous) = previous {
        queue_close(previous, None, ContextMenuCloseReason::Replaced);
    }
    crate::log_info!(target: "ui4";
        "ui4/context-menu: opened owner={:?} window={} context={} entries={} cursor={}:{}:{} lifetime=one-shot\n",
        owner,
        window.raw(),
        request.context,
        entry_count,
        source.controller_id,
        source.slot_id,
        source.ep_target,
    );
    super::input_broker::notify_slot4_visual_change();
    Ok(())
}

pub(super) fn pointer_moved(
    source: Ui4CursorSource,
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
) {
    let changed = {
        let mut active = ACTIVE_MENU.lock();
        let Some(menu) = active.as_mut().filter(|menu| menu.source == source) else {
            return;
        };
        let hovered = entry_at(menu, x, y, screen_width, screen_height)
            .filter(|index| menu.entries[*index].enabled);
        if hovered == menu.hovered {
            false
        } else {
            menu.hovered = hovered;
            true
        }
    };
    if changed {
        super::input_broker::notify_slot4_visual_change();
    }
}

/// Return the active menu serial when this press belongs to the menu. An
/// outside press dismisses the menu and remains available to ordinary UI4
/// routing.
pub(super) fn pointer_down(
    source: Ui4CursorSource,
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
) -> Option<u64> {
    let mut dismissed = None;
    let owned = {
        let mut active = ACTIVE_MENU.lock();
        let Some(menu) = active.as_mut() else {
            return None;
        };
        let inside = menu.source == source
            && visual_rect_contains(
                menu_rect(menu.anchor, menu.entries.len(), screen_width, screen_height),
                x,
                y,
            );
        if inside {
            menu.pressed = entry_at(menu, x, y, screen_width, screen_height)
                .filter(|index| menu.entries[*index].enabled);
            Some(menu.serial)
        } else {
            dismissed = active.take();
            None
        }
    };
    if let Some(dismissed) = dismissed {
        queue_close(dismissed, None, ContextMenuCloseReason::Dismissed);
        super::input_broker::notify_slot4_visual_change();
    }
    owned
}

pub(super) fn pointer_up(
    source: Ui4CursorSource,
    menu_serial: u64,
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
) {
    let selected = {
        let mut active = ACTIVE_MENU.lock();
        let matches = active
            .as_ref()
            .is_some_and(|menu| menu.source == source && menu.serial == menu_serial);
        if !matches {
            return;
        }
        let menu = active.take().expect("matched active context menu");
        let released = entry_at(&menu, x, y, screen_width, screen_height)
            .filter(|index| menu.entries[*index].enabled);
        let selected = (released == menu.pressed)
            .then(|| released.map(|index| menu.entries[index].action_id))
            .flatten();
        (menu, selected)
    };
    let reason = if selected.1.is_some() {
        ContextMenuCloseReason::Selected
    } else {
        ContextMenuCloseReason::Dismissed
    };
    queue_close(selected.0, selected.1, reason);
    super::input_broker::notify_slot4_visual_change();
}

pub(super) fn dismiss_for_source(source: Ui4CursorSource) -> bool {
    dismiss_matching(|menu| menu.source == source, ContextMenuCloseReason::Dismissed)
}

pub(super) fn dismiss_window(owner: WindowOwner, window: WindowId) -> bool {
    dismiss_matching(
        |menu| menu.owner == owner && menu.window == window,
        ContextMenuCloseReason::WindowClosed,
    )
}

pub(super) fn release_owner(owner: WindowOwner) -> usize {
    usize::from(dismiss_matching(|menu| menu.owner == owner, ContextMenuCloseReason::OwnerReleased))
}

fn dismiss_matching(
    matches: impl FnOnce(&ActiveContextMenu) -> bool,
    reason: ContextMenuCloseReason,
) -> bool {
    let dismissed = {
        let mut active = ACTIVE_MENU.lock();
        if active.as_ref().is_some_and(matches) {
            active.take()
        } else {
            None
        }
    };
    let Some(dismissed) = dismissed else {
        return false;
    };
    queue_close(dismissed, None, reason);
    super::input_broker::notify_slot4_visual_change();
    true
}

pub(super) fn visual() -> Option<ContextMenuVisual> {
    let active = ACTIVE_MENU.lock();
    let menu = active.as_ref()?;
    Some(ContextMenuVisual {
        anchor: menu.anchor,
        color: menu.color,
        entries: menu
            .entries
            .iter()
            .map(|entry| ContextMenuVisualEntry {
                label: entry.label.clone(),
                enabled: entry.enabled,
            })
            .collect(),
        hovered: menu.hovered,
    })
}

pub(super) fn dispatch_pending_callbacks() {
    loop {
        let pending = PENDING_CALLBACKS.lock().pop_front();
        let Some(pending) = pending else {
            return;
        };
        (pending.callback)(pending.result);
    }
}

pub(super) fn menu_rect(
    anchor: (u32, u32),
    entry_count: usize,
    screen_width: u32,
    screen_height: u32,
) -> Ui4VisualRect {
    let height = MENU_BORDER_PX
        .saturating_mul(2)
        .saturating_add(MENU_ROW_HEIGHT_PX.saturating_mul(entry_count as u32))
        .min(screen_height);
    let width = MENU_WIDTH_PX.min(screen_width);
    Ui4VisualRect {
        x: anchor
            .0
            .saturating_add(MENU_OFFSET_PX)
            .min(screen_width.saturating_sub(width)),
        y: anchor
            .1
            .saturating_add(MENU_OFFSET_PX)
            .min(screen_height.saturating_sub(height)),
        width,
        height,
    }
}

pub(super) fn entry_rect(menu: Ui4VisualRect, entry_index: usize) -> Option<Ui4VisualRect> {
    let y = menu
        .y
        .saturating_add(MENU_BORDER_PX)
        .saturating_add(MENU_ROW_HEIGHT_PX.saturating_mul(entry_index as u32));
    let bottom = y.saturating_add(MENU_ROW_HEIGHT_PX);
    (bottom <= menu.y.saturating_add(menu.height)).then_some(Ui4VisualRect {
        x: menu.x.saturating_add(MENU_BORDER_PX),
        y,
        width: menu.width.saturating_sub(MENU_BORDER_PX.saturating_mul(2)),
        height: MENU_ROW_HEIGHT_PX,
    })
}

fn entry_at(
    menu: &ActiveContextMenu,
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
) -> Option<usize> {
    let rect = menu_rect(menu.anchor, menu.entries.len(), screen_width, screen_height);
    if !visual_rect_contains(rect, x, y) {
        return None;
    }
    let local_y = y.saturating_sub(rect.y).saturating_sub(MENU_BORDER_PX);
    let index = (local_y / MENU_ROW_HEIGHT_PX) as usize;
    (index < menu.entries.len()).then_some(index)
}

fn visual_rect_contains(rect: Ui4VisualRect, x: u32, y: u32) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

fn queue_close(
    menu: ActiveContextMenu,
    selected_action: Option<u32>,
    reason: ContextMenuCloseReason,
) {
    PENDING_CALLBACKS.lock().push_back(PendingCallback {
        callback: menu.callback,
        result: ContextMenuResult {
            source: menu.source,
            owner: menu.owner,
            window: menu.window,
            anchor: menu.anchor,
            context: menu.context,
            selected_action,
            reason,
        },
    });
}

fn next_menu_serial() -> u64 {
    loop {
        let serial = NEXT_MENU_SERIAL.fetch_add(1, Ordering::Relaxed);
        if serial != 0 {
            return serial;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ignore_result(_: ContextMenuResult) {}

    fn request(entries: Vec<ContextMenuEntry>) -> ContextMenuRequest {
        ContextMenuRequest {
            entries,
            context: 7,
            callback: ignore_result,
        }
    }

    #[test]
    fn request_is_complete_and_bounded() {
        assert_eq!(validate_request(&request(Vec::new())), Err(ContextMenuError::Empty));
        assert_eq!(
            validate_request(&request(vec![ContextMenuEntry::disabled("  ")])),
            Err(ContextMenuError::EmptyLabel)
        );
        let too_many = (0..=MAX_CONTEXT_MENU_ENTRIES)
            .map(|index| ContextMenuEntry::action("printer", index as u32))
            .collect();
        assert_eq!(validate_request(&request(too_many)), Err(ContextMenuError::TooManyEntries));
    }

    #[test]
    fn geometry_clamps_and_maps_each_visible_row() {
        let rect = menu_rect((95, 75), 3, 100, 80);
        assert_eq!(rect.width, 100);
        assert_eq!(rect.height, 76);
        assert_eq!(rect.x, 0);
        assert_eq!(rect.y, 4);
        assert_eq!(entry_rect(rect, 0).unwrap().y, 6);
        assert_eq!(entry_rect(rect, 2).unwrap().y, 54);
        assert!(entry_rect(rect, 3).is_none());
    }
}
