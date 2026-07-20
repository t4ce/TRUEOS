//! Independent UI4 interaction-plane service.
//!
//! Slot 4 contains only software cursors, selection/maximize outlines, and the
//! tiny context menu. It deliberately does not participate in application-
//! plane composition or its atomic SURF batch. Cursor input is coalesced to
//! the display cadence and only the old/new visual rectangles are rewritten.

use embassy_time::{Duration, Instant, with_timeout};
use spin::Mutex;

const SLOT4_PRESENT_PERIOD_MS: u64 = 16;
const SLOT4_RECT_CAPACITY: usize = 512;

type Slot4Rects = heapless::Vec<crate::intel::LiveOverlayRect, SLOT4_RECT_CAPACITY>;

static PRESENTED_RECTS: Mutex<Slot4Rects> = Mutex::new(Slot4Rects::new());

struct Slot4State {
    previous_rects: Slot4Rects,
    signature: u64,
    initialized: bool,
    consecutive_present_failures: u64,
    pending: Option<PendingSlot4Present>,
}

struct PendingSlot4Present {
    flip: crate::intel::Ui4LiveOverlayFlip,
    rects: Slot4Rects,
    signature: u64,
}

impl Slot4State {
    const fn new() -> Self {
        Self {
            previous_rects: Slot4Rects::new(),
            signature: 0,
            initialized: false,
            consecutive_present_failures: 0,
            pending: None,
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_slot4_service_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    crate::log_info!(target: "ui4/slot4";
        "ui4/slot4: service online carrier=ap1-ui-core plane=slot4 content=software-cursors/all-active-sources+selection-outline+maximize-preview+context-menu hardware-cursor=preferred-physical-source/concurrent present_ms={} wake=input-change coalesce=display-cadence damage=disjoint-old+new gpu_submits=0 synthetic-motion=off\n",
        SLOT4_PRESENT_PERIOD_MS,
    );

    let mut state = Slot4State::new();
    let mut visual_dirty = true;
    let mut next_present_ms = 0u64;

    loop {
        let now_ms = Instant::now().as_millis();
        if let Some(pending) = state.pending.take() {
            match crate::intel::poll_ui4_live_overlay_flip(pending.flip) {
                crate::intel::Ui4LiveOverlayFlipPoll::Pending => {
                    state.pending = Some(pending);
                }
                crate::intel::Ui4LiveOverlayFlipPoll::Complete => {
                    commit_presented_slot4(&mut state, pending.rects, pending.signature);
                    state.consecutive_present_failures = 0;
                }
                crate::intel::Ui4LiveOverlayFlipPoll::Failed => {
                    note_present_failure(&mut state, pending.rects.len());
                    visual_dirty = true;
                    next_present_ms = now_ms.saturating_add(SLOT4_PRESENT_PERIOD_MS);
                }
            }
        }

        if state.pending.is_none() && visual_dirty && now_ms >= next_present_ms {
            let rects = software_cursor_rects();
            match queue_slot4(&mut state, &rects) {
                Ok(Some(pending)) => {
                    state.pending = Some(pending);
                    visual_dirty = false;
                }
                Ok(None) => {
                    state.consecutive_present_failures = 0;
                    visual_dirty = false;
                }
                Err(()) => {
                    note_present_failure(&mut state, rects.len());
                    visual_dirty = true;
                }
            }
            next_present_ms = now_ms.saturating_add(SLOT4_PRESENT_PERIOD_MS);
        }

        let now_ms = Instant::now().as_millis();
        if state.pending.is_none() && !visual_dirty {
            super::input_broker::wait_slot4_visual_change().await;
            visual_dirty = true;
            continue;
        }
        let present_wait = if visual_dirty {
            next_present_ms.saturating_sub(now_ms)
        } else {
            u64::MAX
        };
        let flip_wait = if state.pending.is_some() { 1 } else { u64::MAX };
        let wait_ms = present_wait.min(flip_wait).max(1);
        if with_timeout(
            Duration::from_millis(wait_ms),
            super::input_broker::wait_slot4_visual_change(),
        )
        .await
        .is_ok()
        {
            visual_dirty = true;
        }
    }
}

fn queue_slot4(
    state: &mut Slot4State,
    rects: &Slot4Rects,
) -> Result<Option<PendingSlot4Present>, ()> {
    let signature = overlay_rect_signature(rects);
    if state.initialized && signature == state.signature {
        return Ok(None);
    }
    let damage = changed_rect_damage(&state.previous_rects, rects);
    if damage.is_empty() {
        commit_presented_slot4(state, rects.clone(), signature);
        return Ok(None);
    }
    let flip = crate::intel::queue_ui4_live_overlay_rects_on_slot_damage_region(
        super::INTERACTION_OVERLAY_PLANE_SLOT,
        rects,
        damage,
        "ui4-slot4-interaction",
    )
    .ok_or(())?;
    Ok(Some(PendingSlot4Present {
        flip,
        rects: rects.clone(),
        signature,
    }))
}

fn commit_presented_slot4(state: &mut Slot4State, rects: Slot4Rects, signature: u64) {
    state.previous_rects = rects.clone();
    state.signature = signature;
    state.initialized = true;
    *PRESENTED_RECTS.lock() = rects;
}

fn note_present_failure(state: &mut Slot4State, rect_count: usize) {
    state.consecutive_present_failures = state.consecutive_present_failures.saturating_add(1);
    if state.consecutive_present_failures <= 4
        || state.consecutive_present_failures.is_power_of_two()
    {
        crate::log_warn!(target: "ui4/slot4";
            "ui4/slot4: present deferred reason=display-transaction-busy rects={} consecutive={} retry_ms={}\n",
            rect_count,
            state.consecutive_present_failures,
            SLOT4_PRESENT_PERIOD_MS,
        );
    }
}

fn changed_rect_damage(
    previous: &[crate::intel::LiveOverlayRect],
    current: &[crate::intel::LiveOverlayRect],
) -> crate::intel::CompositionDamageRegion {
    let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
    for rect in previous.iter().chain(current) {
        damage.add(crate::intel::CompositionDamageRect::new(
            rect.x,
            rect.y,
            rect.width,
            rect.height,
        ));
    }
    damage
}

pub(super) fn presented_rects() -> Slot4Rects {
    PRESENTED_RECTS.lock().clone()
}

fn software_cursor_rects() -> Slot4Rects {
    use crate::graphics::primitives::Rgba8;

    let visuals = super::software_cursor_visuals();
    let mut rects = Slot4Rects::new();

    for visual in &visuals {
        let Some(preview) = visual.maximize_preview else {
            continue;
        };
        let hint = Rgba8::new(visual.color.r, visual.color.g, visual.color.b, 210);
        push_rect_border(&mut rects, preview, 3, hint);
        let marker_width = preview.width.min(192);
        push_overlay_rect(
            &mut rects,
            preview
                .x
                .saturating_add(preview.width.saturating_sub(marker_width) / 2),
            preview.y,
            marker_width,
            7.min(preview.height),
            Rgba8::new(visual.color.r, visual.color.g, visual.color.b, 235),
        );
    }

    for visual in &visuals {
        if let Some(selection) = visual.selection {
            push_rect_border(&mut rects, selection, 1, visual.color);
        }
    }

    for visual in &visuals {
        let Some((x, y)) = visual.context_menu else {
            continue;
        };
        let (screen_w, screen_h) =
            crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
        let menu_w = 196u32;
        let menu_h = 116u32;
        let menu_x = x.saturating_add(14).min(screen_w.saturating_sub(menu_w));
        let menu_y = y.saturating_add(14).min(screen_h.saturating_sub(menu_h));
        let menu_rect = super::Ui4VisualRect {
            x: menu_x,
            y: menu_y,
            width: menu_w,
            height: menu_h,
        };
        push_overlay_rect(&mut rects, menu_x, menu_y, menu_w, menu_h, Rgba8::new(22, 25, 33, 235));
        push_rect_border(&mut rects, menu_rect, 2, visual.color);
        for row in 1..4u32 {
            push_overlay_rect(
                &mut rects,
                menu_x.saturating_add(12),
                menu_y.saturating_add(row * 27),
                menu_w.saturating_sub(24),
                1,
                Rgba8::new(180, 188, 204, 150),
            );
        }
    }

    for visual in &visuals {
        if !visual.draw_cursor {
            continue;
        }
        let x = visual.x;
        let y = visual.y;
        let color = visual.color;
        push_overlay_rect(&mut rects, x.saturating_sub(2), y.saturating_sub(13), 5, 27, color);
        push_overlay_rect(&mut rects, x.saturating_sub(13), y.saturating_sub(2), 27, 5, color);
        push_overlay_rect(
            &mut rects,
            x.saturating_sub(4),
            y.saturating_sub(4),
            9,
            9,
            Rgba8::new(255, 255, 255, 240),
        );
        push_overlay_rect(&mut rects, x.saturating_sub(2), y.saturating_sub(2), 5, 5, color);
    }

    rects
}

fn overlay_rect_signature(rects: &[crate::intel::LiveOverlayRect]) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for rect in rects {
        for value in [
            rect.x,
            rect.y,
            rect.width,
            rect.height,
            u32::from_le_bytes([rect.color.r, rect.color.g, rect.color.b, rect.color.a]),
        ] {
            hash ^= u64::from(value);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    hash ^ rects.len() as u64
}

fn push_rect_border(
    rects: &mut Slot4Rects,
    rect: super::Ui4VisualRect,
    thickness: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    let thickness = thickness.min(rect.width).min(rect.height);
    push_overlay_rect(rects, rect.x, rect.y, rect.width, thickness, color);
    push_overlay_rect(
        rects,
        rect.x,
        rect.y.saturating_add(rect.height.saturating_sub(thickness)),
        rect.width,
        thickness,
        color,
    );
    push_overlay_rect(rects, rect.x, rect.y, thickness, rect.height, color);
    push_overlay_rect(
        rects,
        rect.x.saturating_add(rect.width.saturating_sub(thickness)),
        rect.y,
        thickness,
        rect.height,
        color,
    );
}

fn push_overlay_rect(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    if width == 0 || height == 0 {
        return;
    }
    let _ = rects.push(crate::intel::LiveOverlayRect::new(x, y, width, height, color));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::primitives::Rgba8;

    #[test]
    fn distant_cursor_damage_stays_disjoint() {
        let white = Rgba8::new(255, 255, 255, 255);
        let previous = [crate::intel::LiveOverlayRect::new(10, 20, 5, 27, white)];
        let current = [crate::intel::LiveOverlayRect::new(2_000, 900, 5, 27, white)];
        let damage = changed_rect_damage(&previous, &current);
        assert_eq!(damage.len(), 2);
        assert!(
            damage
                .rects()
                .contains(&crate::intel::CompositionDamageRect::new(10, 20, 5, 27))
        );
        assert!(
            damage
                .rects()
                .contains(&crate::intel::CompositionDamageRect::new(2_000, 900, 5, 27))
        );
    }
}
