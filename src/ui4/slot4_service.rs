//! Independent UI4 interaction-plane service.
//!
//! Slot 4 contains fixed-shape colored crosshairs, one-pixel selection and
//! maximize outlines, selected-frame strips, and context menus. It deliberately
//! does not participate in application-plane composition or its atomic SURF
//! batch. Cursor input is coalesced to the display cadence.

use embassy_time::{Duration, Instant, with_timeout};
use spin::Mutex;

const SLOT4_RECT_CAPACITY: usize = 3_072;
const SOFTWARE_CURSOR_STROKE_PX: u32 = 5;
const SOFTWARE_CURSOR_LONG_PX: u32 = 27;

type Slot4Rects = heapless::Vec<crate::intel::LiveOverlayRect, SLOT4_RECT_CAPACITY>;

static PRESENTED_RECTS: Mutex<Slot4Rects> = Mutex::new(Slot4Rects::new());

struct Slot4State {
    previous_rects: Slot4Rects,
    consecutive_present_failures: u64,
    pending: Option<PendingSlot4Present>,
}

struct PendingSlot4Present {
    flip: crate::intel::Ui4LiveOverlayFlip,
    rects: Slot4Rects,
}

impl Slot4State {
    const fn new() -> Self {
        Self {
            previous_rects: Slot4Rects::new(),
            consecutive_present_failures: 0,
            pending: None,
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_slot4_service_task() {
    crate::intel::wait_hw_logo_sequence_done().await;
    crate::log_info!(target: "ui4/slot4";
        "ui4/slot4: service online carrier=ap1-ui-core plane=slot4 content=static-color-crosshairs+selected-frame-strips+selection-outline-1px+maximize-outline-1px+context-menu hardware-cursor=preferred-physical-source/concurrent cadence_hz={} cadence_clock=absolute-fractional wake=input-or-frame-state-change coalesce=display-cadence damage=ordered-linear-diff gpu_submits=0 synthetic-motion=off\n",
        super::INTERACTION_CADENCE_HZ,
    );

    let mut state = Slot4State::new();
    let mut visual_dirty = true;
    let mut cadence = super::InteractionCadence::new();
    let mut next_present = Instant::MIN;

    loop {
        let now = Instant::now();
        if let Some(pending) = state.pending.take() {
            match crate::intel::poll_ui4_live_overlay_flip(pending.flip) {
                crate::intel::Ui4LiveOverlayFlipPoll::Pending => {
                    state.pending = Some(pending);
                }
                crate::intel::Ui4LiveOverlayFlipPoll::Complete => {
                    commit_presented_slot4(&mut state, pending.rects);
                    state.consecutive_present_failures = 0;
                }
                crate::intel::Ui4LiveOverlayFlipPoll::Failed => {
                    note_present_failure(&mut state, pending.rects.len());
                    visual_dirty = true;
                    next_present = cadence.next_deadline();
                }
            }
        }

        if state.pending.is_none() && visual_dirty && now >= next_present {
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
            next_present = cadence.next_deadline();
        }

        let now = Instant::now();
        if state.pending.is_none() && !visual_dirty {
            super::input_broker::wait_slot4_visual_change().await;
            visual_dirty = true;
            continue;
        }
        let present_wait = if visual_dirty {
            next_present.saturating_duration_since(now)
        } else {
            Duration::MAX
        };
        let flip_wait = if state.pending.is_some() {
            Duration::from_hz(super::INTERACTION_CADENCE_HZ)
        } else {
            Duration::MAX
        };
        let wait = present_wait.min(flip_wait).max(Duration::from_ticks(1));
        if with_timeout(wait, super::input_broker::wait_slot4_visual_change())
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
    let damage = changed_rect_damage(&state.previous_rects, rects);
    if damage.is_empty() {
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
    }))
}

fn commit_presented_slot4(state: &mut Slot4State, rects: Slot4Rects) {
    state.previous_rects = rects.clone();
    *PRESENTED_RECTS.lock() = rects;
}

fn note_present_failure(state: &mut Slot4State, rect_count: usize) {
    state.consecutive_present_failures = state.consecutive_present_failures.saturating_add(1);
    if state.consecutive_present_failures <= 4
        || state.consecutive_present_failures.is_power_of_two()
    {
        crate::log_warn!(target: "ui4/slot4";
            "ui4/slot4: present deferred reason=display-transaction-busy rects={} consecutive={} retry_hz={}\n",
            rect_count,
            state.consecutive_present_failures,
            super::INTERACTION_CADENCE_HZ,
        );
    }
}

fn changed_rect_damage(
    previous: &[crate::intel::LiveOverlayRect],
    current: &[crate::intel::LiveOverlayRect],
) -> crate::intel::CompositionDamageRegion {
    let mut damage = crate::intel::CompositionDamageRegion::EMPTY;

    // The slot builder has stable ordering. Compare matching positions in one
    // linear pass; a changed entry contributes its old and new bounds.
    let common = previous.len().min(current.len());
    for index in 0..common {
        if !overlay_rect_eq(&previous[index], &current[index]) {
            add_overlay_rect_damage(&mut damage, &previous[index]);
            add_overlay_rect_damage(&mut damage, &current[index]);
        }
    }
    for rect in &previous[common..] {
        add_overlay_rect_damage(&mut damage, rect);
    }
    for rect in &current[common..] {
        add_overlay_rect_damage(&mut damage, rect);
    }
    damage
}

fn overlay_rect_eq(
    left: &crate::intel::LiveOverlayRect,
    right: &crate::intel::LiveOverlayRect,
) -> bool {
    left.x == right.x
        && left.y == right.y
        && left.width == right.width
        && left.height == right.height
        && left.color == right.color
}

fn add_overlay_rect_damage(
    damage: &mut crate::intel::CompositionDamageRegion,
    rect: &crate::intel::LiveOverlayRect,
) {
    damage.add(crate::intel::CompositionDamageRect::new(rect.x, rect.y, rect.width, rect.height));
}

pub(super) fn presented_rects() -> Slot4Rects {
    PRESENTED_RECTS.lock().clone()
}

fn software_cursor_rects() -> Slot4Rects {
    use crate::graphics::primitives::Rgba8;

    let visuals = super::software_cursor_visuals();
    let mut rects = Slot4Rects::new();
    let (screen_w, screen_h) = crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));

    if let Some(output) = super::OutputId::from_slot(0) {
        for strip in super::selection_strips(output, screen_w, screen_h) {
            let count = strip.colors.len() as u64;
            for (index, color) in strip.colors.iter().copied().enumerate() {
                let start = u64::from(strip.width).saturating_mul(index as u64) / count;
                let end = u64::from(strip.width).saturating_mul(index as u64 + 1) / count;
                push_overlay_rect(
                    &mut rects,
                    strip.x.saturating_add(start as u32),
                    strip.y,
                    end.saturating_sub(start) as u32,
                    1,
                    color,
                );
            }
        }
    }

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
        let menu_rect = super::input_broker::context_menu_rect((x, y), screen_w, screen_h);
        push_overlay_rect(
            &mut rects,
            menu_rect.x,
            menu_rect.y,
            menu_rect.width,
            menu_rect.height,
            Rgba8::new(22, 25, 33, 235),
        );
        push_rect_border(&mut rects, menu_rect, 2, visual.color);
        for row in 1..4u32 {
            push_overlay_rect(
                &mut rects,
                menu_rect.x.saturating_add(12),
                menu_rect.y.saturating_add(row * 27),
                menu_rect.width.saturating_sub(24),
                1,
                Rgba8::new(180, 188, 204, 150),
            );
        }
    }

    if let Some(menu) = super::context_menu::visual() {
        push_requested_context_menu(&mut rects, &menu, screen_w, screen_h);
    }

    for visual in &visuals {
        let x = visual.x;
        let y = visual.y;
        let color = visual.color;
        match visual.icon {
            super::Ui4CursorIcon::Default => push_software_cursor(
                &mut rects,
                x,
                y,
                screen_w,
                screen_h,
                color,
                visual.buttons_down != 0,
            ),
            super::Ui4CursorIcon::Loading => {
                push_loading_cursor(&mut rects, x, y, screen_w, screen_h, color)
            }
            super::Ui4CursorIcon::ResizeHorizontal => {
                push_resize_horizontal_cursor(&mut rects, x, y, screen_w, screen_h, color)
            }
            super::Ui4CursorIcon::ResizeVertical => {
                push_resize_vertical_cursor(&mut rects, x, y, screen_w, screen_h, color)
            }
            super::Ui4CursorIcon::ResizeDiagonal => {
                push_resize_diagonal_cursor(&mut rects, x, y, screen_w, screen_h, color)
            }
            super::Ui4CursorIcon::AppOwned => {}
        }
    }

    rects
}

fn push_requested_context_menu(
    rects: &mut Slot4Rects,
    menu: &super::context_menu::ContextMenuVisual,
    screen_w: u32,
    screen_h: u32,
) {
    use crate::graphics::primitives::Rgba8;

    let menu_rect =
        super::context_menu::menu_rect(menu.anchor, menu.entries.len(), screen_w, screen_h);
    push_overlay_rect(
        rects,
        menu_rect.x,
        menu_rect.y,
        menu_rect.width,
        menu_rect.height,
        Rgba8::new(22, 25, 33, 245),
    );
    push_rect_border(rects, menu_rect, 2, menu.color);

    for (index, entry) in menu.entries.iter().enumerate() {
        let Some(row) = super::context_menu::entry_rect(menu_rect, index) else {
            break;
        };
        if menu.hovered == Some(index) && entry.enabled {
            push_overlay_rect(
                rects,
                row.x,
                row.y,
                row.width,
                row.height,
                Rgba8::new(menu.color.r, menu.color.g, menu.color.b, 72),
            );
        }
        if index != 0 {
            push_overlay_rect(
                rects,
                row.x.saturating_add(6),
                row.y,
                row.width.saturating_sub(12),
                1,
                Rgba8::new(180, 188, 204, 105),
            );
        }
        let text_color = if entry.enabled {
            Rgba8::new(236, 240, 248, 245)
        } else {
            Rgba8::new(125, 132, 146, 210)
        };
        push_tiny_menu_text(
            rects,
            row.x
                .saturating_add(super::context_menu::MENU_TEXT_INSET_PX),
            row.y.saturating_add(7),
            entry.label.as_str(),
            text_color,
        );
    }
}

fn push_tiny_menu_text(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    text: &str,
    color: crate::graphics::primitives::Rgba8,
) {
    const SCALE: u32 = 2;
    const ADVANCE: u32 = 8;

    for (character_index, ch) in text
        .chars()
        .take(super::context_menu::MENU_RENDER_LABEL_CHARS)
        .enumerate()
    {
        let glyph = tiny_menu_glyph(ch);
        let glyph_x = x.saturating_add((character_index as u32).saturating_mul(ADVANCE));
        for (row, bits) in glyph.into_iter().enumerate() {
            let mut column = 0u32;
            while column < 3 {
                if bits & (1 << (2 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 3 && bits & (1 << (2 - column)) != 0 {
                    column += 1;
                }
                push_overlay_rect(
                    rects,
                    glyph_x.saturating_add(start.saturating_mul(SCALE)),
                    y.saturating_add((row as u32).saturating_mul(SCALE)),
                    column.saturating_sub(start).saturating_mul(SCALE),
                    SCALE,
                    color,
                );
            }
        }
    }
}

fn tiny_menu_glyph(ch: char) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        'A' => [0b111, 0b101, 0b111, 0b101, 0b101],
        'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        'Q' => [0b111, 0b101, 0b101, 0b111, 0b001],
        'R' => [0b111, 0b101, 0b111, 0b110, 0b101],
        'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        '_' => [0b000, 0b000, 0b000, 0b000, 0b111],
        '.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        ':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        '/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        '(' => [0b010, 0b100, 0b100, 0b100, 0b010],
        ')' => [0b010, 0b001, 0b001, 0b001, 0b010],
        ' ' => [0; 5],
        _ => [0b111, 0b001, 0b010, 0b000, 0b010],
    }
}

/// Draw a centered cross in the interior and deliberately morph it into an
/// inward-facing T at every display edge. Each component is clamped separately
/// so the top/left saturation effect is mirrored at the bottom/right, while no
/// cursor rectangle or its damage can extend beyond the scanout.
fn push_software_cursor(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
    pressed: bool,
) {
    use crate::graphics::primitives::Rgba8;

    let long = if pressed {
        SOFTWARE_CURSOR_PRESSED_LONG_PX
    } else {
        SOFTWARE_CURSOR_LONG_PX
    };
    let long_before = long / 2;

    push_cursor_rect(
        rects,
        x,
        y,
        2,
        long_before,
        SOFTWARE_CURSOR_STROKE_PX,
        long,
        screen_w,
        screen_h,
        color,
    );
    push_cursor_rect(
        rects,
        x,
        y,
        long_before,
        2,
        long,
        SOFTWARE_CURSOR_STROKE_PX,
        screen_w,
        screen_h,
        color,
    );
    push_cursor_rect(
        rects,
        x,
        y,
        4,
        4,
        SOFTWARE_CURSOR_HALO_PX,
        SOFTWARE_CURSOR_HALO_PX,
        screen_w,
        screen_h,
        Rgba8::new(255, 255, 255, 240),
    );
    push_cursor_rect(
        rects,
        x,
        y,
        2,
        2,
        SOFTWARE_CURSOR_STROKE_PX,
        SOFTWARE_CURSOR_STROKE_PX,
        screen_w,
        screen_h,
        color,
    );
}

fn push_loading_cursor(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    use crate::graphics::primitives::Rgba8;

    push_cursor_offset_rect(rects, x, y, -8, -10, 17, 3, screen_w, screen_h, color);
    push_cursor_offset_rect(rects, x, y, -8, 8, 17, 3, screen_w, screen_h, color);
    for (offset_x, offset_y) in [
        (-6, -7),
        (4, -7),
        (-4, -5),
        (2, -5),
        (-2, -3),
        (0, -3),
        (-2, 1),
        (0, 1),
        (-4, 3),
        (2, 3),
        (-6, 5),
        (4, 5),
    ] {
        push_cursor_offset_rect(rects, x, y, offset_x, offset_y, 3, 3, screen_w, screen_h, color);
    }
    push_cursor_offset_rect(
        rects,
        x,
        y,
        -1,
        -1,
        3,
        3,
        screen_w,
        screen_h,
        Rgba8::new(255, 255, 255, 240),
    );
}

fn push_resize_horizontal_cursor(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    use crate::graphics::primitives::Rgba8;

    push_cursor_offset_rect(rects, x, y, -13, -2, 27, 5, screen_w, screen_h, color);
    push_cursor_offset_rect(rects, x, y, -13, -6, 5, 13, screen_w, screen_h, color);
    push_cursor_offset_rect(rects, x, y, 9, -6, 5, 13, screen_w, screen_h, color);
    push_cursor_offset_rect(
        rects,
        x,
        y,
        -1,
        -1,
        3,
        3,
        screen_w,
        screen_h,
        Rgba8::new(255, 255, 255, 240),
    );
}

fn push_resize_vertical_cursor(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    use crate::graphics::primitives::Rgba8;

    push_cursor_offset_rect(rects, x, y, -2, -13, 5, 27, screen_w, screen_h, color);
    push_cursor_offset_rect(rects, x, y, -6, -13, 13, 5, screen_w, screen_h, color);
    push_cursor_offset_rect(rects, x, y, -6, 9, 13, 5, screen_w, screen_h, color);
    push_cursor_offset_rect(
        rects,
        x,
        y,
        -1,
        -1,
        3,
        3,
        screen_w,
        screen_h,
        Rgba8::new(255, 255, 255, 240),
    );
}

fn push_resize_diagonal_cursor(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    use crate::graphics::primitives::Rgba8;

    for offset in [-10, -8, -6, -4, -2, 0, 2, 4, 6, 8, 10] {
        push_cursor_offset_rect(rects, x, y, offset, offset, 3, 3, screen_w, screen_h, color);
    }
    for (offset_x, offset_y) in [(-10, -6), (-6, -10), (10, 6), (6, 10)] {
        push_cursor_offset_rect(rects, x, y, offset_x, offset_y, 3, 3, screen_w, screen_h, color);
    }
    push_cursor_offset_rect(
        rects,
        x,
        y,
        -1,
        -1,
        3,
        3,
        screen_w,
        screen_h,
        Rgba8::new(255, 255, 255, 240),
    );
}

#[allow(clippy::too_many_arguments)]
fn push_cursor_offset_rect(
    rects: &mut Slot4Rects,
    center_x: u32,
    center_y: u32,
    offset_x: i32,
    offset_y: i32,
    width: u32,
    height: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    let width = width.min(screen_w);
    let height = height.min(screen_h);
    if width == 0 || height == 0 {
        return;
    }
    let x = i64::from(center_x)
        .saturating_add(i64::from(offset_x))
        .clamp(0, i64::from(screen_w - width)) as u32;
    let y = i64::from(center_y)
        .saturating_add(i64::from(offset_y))
        .clamp(0, i64::from(screen_h - height)) as u32;
    push_overlay_rect(rects, x, y, width, height, color);
}

#[allow(clippy::too_many_arguments)]
fn push_cursor_rect(
    rects: &mut Slot4Rects,
    center_x: u32,
    center_y: u32,
    pixels_before_x: u32,
    pixels_before_y: u32,
    width: u32,
    height: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    let width = width.min(screen_w);
    let height = height.min(screen_h);
    if width == 0 || height == 0 {
        return;
    }
    let x = center_x
        .saturating_sub(pixels_before_x)
        .min(screen_w - width);
    let y = center_y
        .saturating_sub(pixels_before_y)
        .min(screen_h - height);
    push_overlay_rect(rects, x, y, width, height, color);
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
    if thickness == 0 {
        return;
    }

    // Partition the border instead of overlapping its four corners. Damage
    // regions conservatively union overlapping rectangles into their bounding
    // box; overlapping border pieces would therefore turn a thin outline into
    // full-rectangle damage on every resize or selection movement.
    let top_height = thickness;
    let bottom_height = thickness.min(rect.height.saturating_sub(top_height));
    let middle_y = rect.y.saturating_add(top_height);
    let middle_height = rect
        .height
        .saturating_sub(top_height)
        .saturating_sub(bottom_height);
    let left_width = thickness;
    let right_width = thickness.min(rect.width.saturating_sub(left_width));

    push_overlay_rect(rects, rect.x, rect.y, rect.width, top_height, color);
    push_overlay_rect(
        rects,
        rect.x,
        rect.y
            .saturating_add(rect.height.saturating_sub(bottom_height)),
        rect.width,
        bottom_height,
        color,
    );
    push_overlay_rect(rects, rect.x, middle_y, left_width, middle_height, color);
    push_overlay_rect(
        rects,
        rect.x
            .saturating_add(rect.width.saturating_sub(right_width)),
        middle_y,
        right_width,
        middle_height,
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

    const TEST_SCREEN_W: u32 = 100;
    const TEST_SCREEN_H: u32 = 80;

    fn test_cursor_rects(x: u32, y: u32, pressed: bool) -> Slot4Rects {
        let mut rects = Slot4Rects::new();
        push_software_cursor(
            &mut rects,
            x,
            y,
            TEST_SCREEN_W,
            TEST_SCREEN_H,
            Rgba8::new(255, 0, 0, 255),
            pressed,
        );
        rects
    }

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

    #[test]
    fn software_cursor_shape_is_mirrored_at_opposite_edges() {
        let left = test_cursor_rects(0, TEST_SCREEN_H / 2, false);
        let right = test_cursor_rects(TEST_SCREEN_W - 1, TEST_SCREEN_H / 2, false);
        let top = test_cursor_rects(TEST_SCREEN_W / 2, 0, false);
        let bottom = test_cursor_rects(TEST_SCREEN_W / 2, TEST_SCREEN_H - 1, false);

        assert_eq!(left.len(), 4);
        assert_eq!(right.len(), left.len());
        assert_eq!(top.len(), 4);
        assert_eq!(bottom.len(), top.len());
        for (left, right) in left.iter().zip(&right) {
            assert_eq!(right.x, TEST_SCREEN_W - left.x - left.width);
            assert_eq!(right.y, left.y);
            assert_eq!(right.width, left.width);
            assert_eq!(right.height, left.height);
        }
        for (top, bottom) in top.iter().zip(&bottom) {
            assert_eq!(bottom.x, top.x);
            assert_eq!(bottom.y, TEST_SCREEN_H - top.y - top.height);
            assert_eq!(bottom.width, top.width);
            assert_eq!(bottom.height, top.height);
        }
    }

    #[test]
    fn software_cursor_remains_centered_away_from_edges() {
        let rects = test_cursor_rects(50, 40, false);
        let expected = [
            (48, 27, SOFTWARE_CURSOR_STROKE_PX, SOFTWARE_CURSOR_LONG_PX),
            (37, 38, SOFTWARE_CURSOR_LONG_PX, SOFTWARE_CURSOR_STROKE_PX),
            (46, 36, SOFTWARE_CURSOR_HALO_PX, SOFTWARE_CURSOR_HALO_PX),
            (48, 38, SOFTWARE_CURSOR_STROKE_PX, SOFTWARE_CURSOR_STROKE_PX),
        ];

        assert_eq!(rects.len(), expected.len());
        for (rect, (x, y, width, height)) in rects.iter().zip(expected) {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (x, y, width, height));
        }
    }

    #[test]
    fn software_cursor_rects_stay_inside_every_corner() {
        for (x, y) in [
            (0, 0),
            (TEST_SCREEN_W - 1, 0),
            (0, TEST_SCREEN_H - 1),
            (TEST_SCREEN_W - 1, TEST_SCREEN_H - 1),
        ] {
            let rects = test_cursor_rects(x, y, false);
            assert_eq!(rects.len(), 4);
            for rect in rects {
                assert!(rect.x.saturating_add(rect.width) <= TEST_SCREEN_W);
                assert!(rect.y.saturating_add(rect.height) <= TEST_SCREEN_H);
            }
        }
    }

    #[test]
    fn pressed_cursor_shortens_only_the_long_axes() {
        let released = test_cursor_rects(50, 40, false);
        let pressed = test_cursor_rects(50, 40, true);
        assert_eq!(pressed.len(), released.len());
        assert_eq!(
            (pressed[0].x, pressed[0].y, pressed[0].width, pressed[0].height),
            (48, 30, SOFTWARE_CURSOR_STROKE_PX, SOFTWARE_CURSOR_PRESSED_LONG_PX)
        );
        assert_eq!(
            (pressed[1].x, pressed[1].y, pressed[1].width, pressed[1].height),
            (40, 38, SOFTWARE_CURSOR_PRESSED_LONG_PX, SOFTWARE_CURSOR_STROKE_PX)
        );
        for index in 2..released.len() {
            assert!(overlay_rect_eq(&released[index], &pressed[index]));
        }
    }

    #[test]
    fn press_damage_is_only_the_four_removed_caps() {
        let released = test_cursor_rects(50, 40, false);
        let pressed = test_cursor_rects(50, 40, true);
        let damage = changed_rect_damage(&released, &pressed);
        let expected = [
            crate::intel::CompositionDamageRect::new(48, 27, 5, 3),
            crate::intel::CompositionDamageRect::new(48, 51, 5, 3),
            crate::intel::CompositionDamageRect::new(37, 38, 3, 5),
            crate::intel::CompositionDamageRect::new(61, 38, 3, 5),
        ];
        assert_eq!(damage.len(), expected.len());
        for rect in expected {
            assert!(damage.rects().contains(&rect));
        }
        assert_eq!(changed_rect_damage(&pressed, &released), damage);
    }

    #[test]
    fn border_rectangles_partition_the_outline_without_overlap() {
        let mut rects = Slot4Rects::new();
        push_rect_border(
            &mut rects,
            super::super::Ui4VisualRect {
                x: 10,
                y: 20,
                width: 30,
                height: 20,
            },
            3,
            Rgba8::new(255, 0, 0, 255),
        );

        let expected = [
            (10, 20, 30, 3),
            (10, 37, 30, 3),
            (10, 23, 3, 14),
            (37, 23, 3, 14),
        ];
        assert_eq!(rects.len(), expected.len());
        for (rect, expected) in rects.iter().zip(expected) {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), expected);
        }
        for left in 0..rects.len() {
            for right in left + 1..rects.len() {
                let left = &rects[left];
                let right = &rects[right];
                let left_right = left.x.saturating_add(left.width);
                let left_bottom = left.y.saturating_add(left.height);
                let right_right = right.x.saturating_add(right.width);
                let right_bottom = right.y.saturating_add(right.height);
                assert!(
                    left_right <= right.x
                        || right_right <= left.x
                        || left_bottom <= right.y
                        || right_bottom <= left.y
                );
            }
        }
    }

    #[test]
    fn one_pixel_high_border_is_not_emitted_four_times() {
        let mut rects = Slot4Rects::new();
        push_rect_border(
            &mut rects,
            super::super::Ui4VisualRect {
                x: 4,
                y: 7,
                width: 23,
                height: 1,
            },
            1,
            Rgba8::new(255, 0, 0, 255),
        );

        assert_eq!(rects.len(), 1);
        assert_eq!((rects[0].x, rects[0].y, rects[0].width, rects[0].height), (4, 7, 23, 1));
    }
}
