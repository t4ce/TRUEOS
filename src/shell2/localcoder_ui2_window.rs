use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use localcoder::localcoder_service as cursor_service;
use localcoder::ui2_window_controller as controller_tool;
use localcoder::ui2_window_observer as observer_tool;
use serde_json::{Value, json};

use super::localcoder_service;

const WINDOW_ACTION_APPROACH_MS: u32 = 120;
const WINDOW_ACTION_CLICK_DELAY_MS: u32 = 90;

pub(crate) fn ensure_registered() {
    observer_tool::register_handler(handle_observer_request);
    controller_tool::register_handler(handle_controller_command);
}

fn handle_observer_request(
    request: observer_tool::Ui2WindowObserverRequest,
) -> observer_tool::Ui2WindowObserverResult {
    let title_filter = request
        .title_contains
        .as_ref()
        .map(|value| value.to_ascii_lowercase());

    let windows: Vec<Value> = crate::r::ui2::window_shell_snapshots()
        .into_iter()
        .filter(|window| request.include_hidden || window.visible)
        .filter(|window| request.window_id.is_none_or(|id| window.id == id))
        .filter(|window| {
            title_filter
                .as_ref()
                .is_none_or(|needle| window.title.to_ascii_lowercase().contains(needle.as_str()))
        })
        .map(window_snapshot_to_json)
        .collect();

    Ok(json!({
        "count": windows.len(),
        "windows": windows,
    })
    .to_string())
}

fn handle_controller_command(
    command: controller_tool::Ui2WindowControllerCommand,
) -> controller_tool::Ui2WindowControllerResult {
    match command {
        controller_tool::Ui2WindowControllerCommand::Focus(selector) => {
            let window = resolve_window(selector)?;
            direct_window_action(window.id, "focus", || unsafe {
                crate::r::ui2::trueos_cabi_ui2_window_focus(window.id)
            })
        }
        controller_tool::Ui2WindowControllerCommand::Minimize(selector) => {
            let window = resolve_window(selector)?;
            direct_window_action(window.id, "minimize", || unsafe {
                crate::r::ui2::trueos_cabi_ui2_window_minimize(window.id)
            })
        }
        controller_tool::Ui2WindowControllerCommand::Maximize(selector) => {
            let window = resolve_window(selector)?;
            direct_window_action(window.id, "maximize", || unsafe {
                crate::r::ui2::trueos_cabi_ui2_window_maximize(window.id)
            })
        }
        controller_tool::Ui2WindowControllerCommand::Restore(selector) => {
            let window = resolve_window(selector)?;
            direct_window_action(window.id, "restore", || unsafe {
                crate::r::ui2::trueos_cabi_ui2_window_restore(window.id)
            })
        }
        controller_tool::Ui2WindowControllerCommand::Close(selector) => {
            let window = resolve_window(selector)?;
            direct_window_action(window.id, "close", || unsafe {
                crate::r::ui2::trueos_cabi_ui2_window_close(window.id)
            })
        }
        controller_tool::Ui2WindowControllerCommand::Move {
            selector,
            x_px,
            y_px,
            x_norm,
            y_norm,
            duration_ms,
        } => {
            let window = resolve_window(selector)?;
            let titlebar = window.titlebar_rect.ok_or_else(|| {
                controller_tool::controller_error(format!(
                    "window {} has no draggable titlebar",
                    window.id
                ))
            })?;
            let drag_anchor = titlebar.center();
            let target_top_left = resolve_destination_top_left(x_px, y_px, x_norm, y_norm)?;
            let target_cursor_x =
                target_top_left.0 + (drag_anchor.x - window.frame_rect.x).round() as i32;
            let target_cursor_y =
                target_top_left.1 + (drag_anchor.y - window.frame_rect.y).round() as i32;

            let mut summary = Vec::new();
            summary.push(localcoder_service::enqueue_command(
                cursor_service::LocalcoderServiceCommand::MoveAbs {
                    cursor_slot_id: None,
                    x_px: Some(drag_anchor.x.round() as i32),
                    y_px: Some(drag_anchor.y.round() as i32),
                    x_norm: None,
                    y_norm: None,
                    duration_ms: WINDOW_ACTION_APPROACH_MS,
                },
            )?);
            summary.push(localcoder_service::enqueue_command(
                cursor_service::LocalcoderServiceCommand::ButtonDown {
                    cursor_slot_id: None,
                    buttons_down: 1,
                },
            )?);
            summary.push(localcoder_service::enqueue_command(
                cursor_service::LocalcoderServiceCommand::MoveAbs {
                    cursor_slot_id: None,
                    x_px: Some(target_cursor_x),
                    y_px: Some(target_cursor_y),
                    x_norm: None,
                    y_norm: None,
                    duration_ms,
                },
            )?);
            summary.push(localcoder_service::enqueue_command(
                cursor_service::LocalcoderServiceCommand::ButtonUp {
                    cursor_slot_id: None,
                    buttons_up: 1,
                },
            )?);
            Ok(format!(
                "window {} drag queued to top-left {},{} ({})",
                window.id,
                target_top_left.0,
                target_top_left.1,
                summary.join("; ")
            ))
        }
    }
}

fn direct_window_action(
    window_id: u32,
    action_name: &str,
    invoke: impl FnOnce() -> i32,
) -> controller_tool::Ui2WindowControllerResult {
    let rc = invoke();
    if rc == 0 {
        Ok(format!("window {} {} applied via UI2 kernel service", window_id, action_name))
    } else {
        Err(controller_tool::controller_error(format!(
            "window {} {} failed via UI2 kernel service (rc={})",
            window_id, action_name, rc
        )))
    }
}

fn resolve_window(
    selector: controller_tool::Ui2WindowSelector,
) -> Result<crate::r::ui2::Ui2WindowShellSnapshot, controller_tool::Ui2WindowControllerError> {
    let windows = crate::r::ui2::window_shell_snapshots();

    if let Some(window_id) = selector.window_id {
        return windows
            .into_iter()
            .find(|window| window.id == window_id)
            .ok_or_else(|| {
                controller_tool::controller_error(format!("window {} not found", window_id))
            });
    }

    let title_contains = selector
        .title_contains
        .as_ref()
        .map(|value| value.to_ascii_lowercase())
        .ok_or_else(|| {
            controller_tool::controller_error(
                "title_contains selector is required when window_id is absent",
            )
        })?;

    let mut matches: Vec<_> = windows
        .into_iter()
        .filter(|window| {
            window
                .title
                .to_ascii_lowercase()
                .contains(title_contains.as_str())
        })
        .collect();

    if matches.is_empty() {
        return Err(controller_tool::controller_error(format!(
            "no window title matched '{}'",
            title_contains
        )));
    }

    matches.sort_by(|left, right| {
        right
            .selected
            .cmp(&left.selected)
            .then(right.visible.cmp(&left.visible))
            .then(right.z.cmp(&left.z))
            .then(left.id.cmp(&right.id))
    });
    Ok(matches.remove(0))
}

fn queue_rect_click(
    window_id: u32,
    action_name: &str,
    rect: Option<crate::r::ui2::Ui2Rect>,
) -> controller_tool::Ui2WindowControllerResult {
    let rect = rect.ok_or_else(|| {
        controller_tool::controller_error(format!(
            "window {} has no {} shell target",
            window_id, action_name
        ))
    })?;
    let point = rect.center();
    let summary = queue_move_click(point.x, point.y, WINDOW_ACTION_APPROACH_MS)?;
    Ok(format!("window {} {} queued ({})", window_id, action_name, summary))
}

fn queue_move_click(
    x: f32,
    y: f32,
    duration_ms: u32,
) -> controller_tool::Ui2WindowControllerResult {
    let mut summary = Vec::new();
    summary.push(localcoder_service::enqueue_command(
        cursor_service::LocalcoderServiceCommand::MoveAbs {
            cursor_slot_id: None,
            x_px: Some(x.round() as i32),
            y_px: Some(y.round() as i32),
            x_norm: None,
            y_norm: None,
            duration_ms,
        },
    )?);
    summary.push(localcoder_service::enqueue_command(
        cursor_service::LocalcoderServiceCommand::Click {
            cursor_slot_id: None,
            buttons_down: 1,
            repeat: 1,
            delay_ms: WINDOW_ACTION_CLICK_DELAY_MS,
        },
    )?);
    Ok(summary.join("; "))
}

fn resolve_destination_top_left(
    x_px: Option<i32>,
    y_px: Option<i32>,
    x_norm: Option<f64>,
    y_norm: Option<f64>,
) -> Result<(i32, i32), controller_tool::Ui2WindowControllerError> {
    match (x_px, y_px, x_norm, y_norm) {
        (Some(x_px), Some(y_px), None, None) => Ok((x_px, y_px)),
        (None, None, Some(x_norm), Some(y_norm)) => {
            let (vp_w, vp_h) = crate::r::io::cabi::localcoder_cursor_viewport_dimensions_px();
            Ok((norm_to_px(x_norm, vp_w), norm_to_px(y_norm, vp_h)))
        }
        _ => Err(controller_tool::controller_error(
            "move requires either x_px/y_px or x_norm/y_norm",
        )),
    }
}

fn norm_to_px(value: f64, extent_px: i32) -> i32 {
    let clamped = value.clamp(0.0, 1.0);
    let max = extent_px.saturating_sub(1).max(1) as f64;
    (clamped * max).round() as i32
}

fn window_snapshot_to_json(window: crate::r::ui2::Ui2WindowShellSnapshot) -> Value {
    json!({
        "window_id": window.id,
        "title": window.title,
        "kind": window.kind,
        "state": window.state,
        "visible": window.visible,
        "selected": window.selected,
        "hit_test_visible": window.hit_test_visible,
        "z": window.z,
        "frame_rect": rect_to_json(window.frame_rect),
        "content_rect": window.content_rect.map(rect_to_json),
        "titlebar_rect": window.titlebar_rect.map(rect_to_json),
        "targets": {
            "minimize": window.minimize_rect.map(rect_to_json),
            "maximize": window.maximize_rect.map(rect_to_json),
            "restore": window.restore_rect.map(rect_to_json),
            "close": window.close_rect.map(rect_to_json),
            "resize": window.resize_rect.map(rect_to_json),
        },
        "available_actions": {
            "focus": true,
            "move": window.titlebar_rect.is_some(),
            "minimize": window.minimize_rect.is_some(),
            "maximize": window.maximize_rect.is_some(),
            "restore": window.restore_rect.is_some(),
            "close": window.close_rect.is_some(),
        }
    })
}

fn rect_to_json(rect: crate::r::ui2::Ui2Rect) -> Value {
    json!({
        "x": rect.x,
        "y": rect.y,
        "w": rect.w,
        "h": rect.h,
        "center_x": rect.x + (rect.w * 0.5),
        "center_y": rect.y + (rect.h * 0.5),
    })
}

trait RectCenterExt {
    fn center(self) -> RectCenter;
}

impl RectCenterExt for crate::r::ui2::Ui2Rect {
    fn center(self) -> RectCenter {
        RectCenter {
            x: self.x + (self.w * 0.5),
            y: self.y + (self.h * 0.5),
        }
    }
}

struct RectCenter {
    x: f32,
    y: f32,
}
