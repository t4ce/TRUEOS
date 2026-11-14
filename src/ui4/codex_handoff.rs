//! Alternative Codex collapse: retain a restore ticket, never a flying window.
use super::*;
use crate::spirit::codex_handoff::{self as arrival, Key};
use arrival::geometry::Point;

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Shrinking,
    Queued,
    Sprite,
}
#[derive(Clone, Copy)]
struct Ticket {
    key: Key,
    identity: (u8, u64),
    start: Point,
    opacity: u8,
    phase: Phase,
}
static TICKETS: spin::Mutex<alloc::vec::Vec<Ticket>> = spin::Mutex::new(alloc::vec::Vec::new());

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

/// Called once, after the collapse transaction releases the broker lock.
/// Everything needed by the sprite is captured here; flight never queries it.
pub(super) fn queue(
    owner: WindowOwner,
    window: WindowId,
    icon: FrameHandle,
    start: Point,
    opacity: u8,
) {
    let Some(identity) = identity(owner, window) else {
        return;
    };
    {
        let mut tickets = TICKETS.lock();
        if tickets
            .iter()
            .any(|t| t.key.owner == owner && t.key.window == window)
        {
            return;
        }
        tickets.push(Ticket {
            key: Key {
                owner,
                window,
                icon,
            },
            identity,
            start,
            opacity,
            phase: Phase::Shrinking,
        });
    }
    super::cursor_frame_inout::frame_parked(owner, window);
    super::input_broker::notify_slot4_visual_change();
}
pub(super) fn parked(owner: WindowOwner, window: WindowId) -> bool {
    TICKETS
        .lock()
        .iter()
        .any(|t| t.key.owner == owner && t.key.window == window)
}
pub(super) fn has_parked() -> bool {
    !TICKETS.lock().is_empty()
}
pub(super) fn cancel(owner: WindowOwner, window: WindowId) {
    {
        let mut tickets = TICKETS.lock();
        tickets.retain(|t| t.key.owner != owner || t.key.window != window);
        // Same ticket -> sprite lock order as the ownership transfer below.
        // A concurrent restore cannot be followed by an orphan sprite start.
        arrival::cancel(owner, window);
    }
    super::input_broker::notify_slot4_visual_change();
}
pub(super) fn active() -> bool {
    TICKETS.lock().iter().any(|t| t.phase != Phase::Sprite) || arrival::active()
}
pub(super) fn folded_target() -> Option<(WindowOwner, WindowId)> {
    let ticket = TICKETS.lock().last().copied()?;
    // Validate the live run only when opening, not on every motion sample.
    (identity(ticket.key.owner, ticket.key.window) == Some(ticket.identity)
        && window_broker::collapse_state(ticket.key.owner, ticket.key.window) == Some(true))
    .then_some((ticket.key.owner, ticket.key.window))
}

/// Driven by the software cursor service, including when no mouse is moving.
pub(super) fn advance() {
    let ticket = TICKETS
        .lock()
        .iter()
        .find(|t| t.phase != Phase::Sprite)
        .copied();
    if let Some(ticket) = ticket {
        if ticket.phase == Phase::Shrinking
            && (identity(ticket.key.owner, ticket.key.window) != Some(ticket.identity)
                || window_broker::collapse_state(ticket.key.owner, ticket.key.window) != Some(true))
        {
            cancel(ticket.key.owner, ticket.key.window);
            return;
        }
        if ticket.phase == Phase::Shrinking
            && super::collapse_animation::active_for(ticket.key.owner, ticket.key.window)
        {
            return;
        }
        let detached = {
            let mut tickets = TICKETS.lock();
            let Some(t) = tickets.iter_mut().find(|t| t.key == ticket.key) else {
                return;
            };
            let detached = t.phase == Phase::Shrinking;
            if detached {
                t.phase = Phase::Queued;
            }
            if t.phase == Phase::Queued && arrival::begin(t.key, t.start, t.opacity) {
                t.phase = Phase::Sprite;
            }
            detached
        };
        if detached {
            // One application composition removes the parked tile. Every
            // following slide update belongs exclusively to slot4 / Spirit.
            window_broker::collapse_animation_changed(ticket.key.owner, ticket.key.window);
        }
    }
}

pub(super) fn project(mut window: WindowSnapshot) -> WindowSnapshot {
    let ticket = TICKETS
        .lock()
        .iter()
        .find(|t| t.key.owner == window.owner && t.key.window == window.id)
        .copied();
    if let Some(ticket) = ticket {
        window.interaction = WindowInteraction {
            movable: false,
            maximizable: false,
            receives_input: false,
            primary_activation: false,
            hit_testable: false,
            resize_on_maximize: false,
        };
        if ticket.phase != Phase::Shrinking {
            window.placement.visible = false;
            window.presentation_placement.visible = false;
        }
    }
    window
}
