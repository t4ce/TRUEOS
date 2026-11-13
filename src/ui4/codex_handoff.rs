//! UI4's half of Spirit's Codex handoff. Producer geometry is never mutated.
use super::*;
use crate::spirit::codex_handoff::{self as arrival, Key, Visual};
use arrival::geometry::{self, Point};

struct Pending {
    key: Key,
    identity: (u8, u64),
    settled: Option<u64>,
}
struct State {
    pending: Option<Pending>,
    visual: Option<Visual>,
    frame: Option<FrameHandle>,
}
static STATE: spin::Mutex<State> = spin::Mutex::new(State {
    pending: None,
    visual: None,
    frame: None,
});

fn identity(owner: WindowOwner, window: WindowId) -> Option<(u8, u64)> {
    let vm = match owner {
        WindowOwner::Vm(vm) => vm,
        WindowOwner::SHELL3_SERVICE => crate::shell3::tui::vm_for_window(window)?,
        _ => return None,
    };
    let run = crate::hv::vm_run_generation(vm)?;
    (crate::hv::blueprint_current_run_archive(vm, run).as_deref() == Some("codex.bp"))
        .then_some((vm, run))
}
pub(super) fn canonical_icon(owner: WindowOwner, window: WindowId) -> bool {
    identity(owner, window).is_some()
}
pub(super) fn queue(owner: WindowOwner, window: WindowId, icon: FrameHandle) {
    let Some(identity) = identity(owner, window) else {
        return;
    };
    let mut s = STATE.lock();
    if s.pending.is_some() || arrival::active() {
        return;
    }
    s.pending = Some(Pending {
        key: Key {
            owner,
            window,
            icon,
        },
        identity,
        settled: None,
    });
}
pub(super) fn cancel(owner: WindowOwner, window: WindowId) {
    arrival::cancel(owner, window);
    let old = {
        let mut s = STATE.lock();
        if s.pending
            .as_ref()
            .is_some_and(|p| p.key.owner == owner && p.key.window == window)
        {
            s.pending = None;
        }
        if s.visual
            .is_some_and(|v| v.key.owner == owner && v.key.window == window)
        {
            s.visual = None;
            s.frame.take()
        } else {
            None
        }
    };
    if let Some(frame) = old {
        window_broker::retire_frame_when_released(frame);
    }
}
pub(super) fn active() -> bool {
    STATE.lock().pending.is_some() || arrival::active()
}

pub(super) fn folded_target() -> Option<(WindowOwner, WindowId)> {
    let key = arrival::folded_target()?;
    if identity(key.owner, key.window).is_none()
        || window_broker::collapse_state(key.owner, key.window) != Some(true)
        || window_broker::window_snapshot(key.owner, key.window)?.frame != key.icon
    {
        return None;
    }
    Some((key.owner, key.window))
}

pub(super) fn advance() {
    let now = trueos_time::Instant::now().as_millis();
    // Copy observations first: no Spirit or frame-pool work under the broker.
    let pending = STATE.lock().pending.as_ref().map(|p| (p.key, p.identity));
    if let Some((key, expected)) = pending {
        if identity(key.owner, key.window) != Some(expected)
            || window_broker::collapse_state(key.owner, key.window) != Some(true)
        {
            cancel(key.owner, key.window);
        } else if !super::collapse_animation::active_for(key.owner, key.window) {
            let ready = {
                let mut s = STATE.lock();
                s.pending
                    .as_mut()
                    .is_some_and(|p| now.saturating_sub(*p.settled.get_or_insert(now)) >= 500)
            };
            if ready {
                if let Some(window) = window_broker::window_snapshot(key.owner, key.window) {
                    let start = Point {
                        x: window.placement.x,
                        y: window.placement.y,
                    };
                    if arrival::begin(key, start, window.placement.opacity) {
                        STATE.lock().pending = None;
                    }
                }
            }
        }
    }
    let Some(visual) = arrival::visual() else {
        return;
    };
    if STATE.lock().visual == Some(visual) {
        return;
    }
    let Some(window) = window_broker::window_snapshot(visual.key.owner, visual.key.window) else {
        return;
    };
    if arrival::absorbed(visual.key) || visual.hidden {
        let old = {
            let mut s = STATE.lock();
            s.visual = Some(visual);
            s.frame.take()
        };
        if let Some(old) = old {
            window_broker::retire_frame_when_released(old);
        }
        window_broker::collapse_animation_changed(visual.key.owner, visual.key.window);
        return;
    }
    let Ok(frame) = create_frame(FrameSpec {
        output: window.output,
        content: FrameContent::Image,
        cadence: FrameCadence::Immutable,
        buffering: FrameBuffering::Single,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width: 64,
        height: 64,
        base_color: None,
    }) else {
        return;
    };
    let result = (|| {
        let lease = acquire_frame_buffer(frame)?;
        let result = (|| {
            let view = writable_rgba_view(lease)?;
            let bytes = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
            geometry::ui4_part(
                visual.pose,
                arrival::ICON,
                visual.opacity,
                bytes,
                view.pitch as usize,
            );
            crate::intel::dma_flush(view.virt, view.byte_len);
            publish_frame_buffer(lease)
        })();
        if result.is_err() {
            let _ = cancel_frame_buffer(lease);
        }
        result
    })();
    if result.is_err() {
        window_broker::retire_frame_when_released(frame);
        return;
    }
    let old = {
        let mut s = STATE.lock();
        s.visual = Some(visual);
        s.frame.replace(frame)
    };
    if let Some(old) = old {
        window_broker::retire_frame_when_released(old);
    }
    window_broker::collapse_animation_changed(visual.key.owner, visual.key.window);
}

pub(super) fn project(mut window: WindowSnapshot) -> WindowSnapshot {
    let key = Key {
        owner: window.owner,
        window: window.id,
        icon: window.frame,
    };
    if arrival::absorbed(key) {
        window.placement.visible = false;
        window.presentation_placement.visible = false;
        return window;
    }
    let s = STATE.lock();
    let Some(visual) = s.visual.filter(|v| {
        v.key.owner == window.owner && v.key.window == window.id && v.key.icon == window.frame
    }) else {
        return window;
    };
    if visual.hidden {
        window.placement.visible = false;
        window.presentation_placement.visible = false;
    } else if let Some(frame) = s.frame {
        window.frame = frame;
        window.placement.x = visual.pose.icon.x;
        window.placement.y = visual.pose.icon.y;
        // The original window opacity is already baked into both tile halves.
        window.placement.opacity = 255;
        window.presentation_placement = window.placement;
        window.buffering = FrameBuffering::Single;
    }
    window
}
