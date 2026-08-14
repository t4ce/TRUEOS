//! Independent UI4 interaction-plane service.
//!
//! Slot 4 contains the kernel color picker, fixed-shape colored crosshairs,
//! one-pixel selection and maximize outlines, selected-frame strips, and
//! context menus. It deliberately does not participate in application-plane
//! composition or its atomic SURF batch. Cursor input is coalesced to the
//! display cadence.

use alloc::vec::Vec;

use embassy_time::{Duration, Instant, with_timeout};
use spin::Mutex;

const SLOT4_RECT_CAPACITY: usize = 3_072;
const SOFTWARE_CURSOR_STROKE_PX: u32 = 5;
const SOFTWARE_CURSOR_LONG_PX: u32 = 27;

type Slot4Rects = heapless::Vec<crate::intel::LiveOverlayRect, SLOT4_RECT_CAPACITY>;

static PRESENTED_RECTS: Mutex<Slot4Rects> = Mutex::new(Slot4Rects::new());

struct Slot4State {
    previous_rects: Slot4Rects,
    previous_windows: Vec<Slot4WindowStamp>,
    consecutive_present_failures: u64,
    pending: Option<PendingSlot4Present>,
}

struct PendingSlot4Present {
    flip: crate::intel::Ui4LiveOverlayFlip,
    rects: Slot4Rects,
    windows: Vec<Slot4WindowStamp>,
    leases: Vec<super::FrameReadLease>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct Slot4WindowStamp {
    id: super::WindowId,
    frame: super::FrameHandle,
    publish_serial: u64,
    revision: u64,
    placement: super::WindowPlacement,
}

impl Slot4State {
    const fn new() -> Self {
        Self {
            previous_rects: Slot4Rects::new(),
            previous_windows: Vec::new(),
            consecutive_present_failures: 0,
            pending: None,
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_slot4_service_task() {
    crate::log_info!(target: "ui4/slot4";
        "ui4/slot4: service online carrier=ap1-ui-core plane=slot4 content=color-picker-rgba8+static-color-crosshairs+selected-frame-strips+selection-outline-1px+maximize-outline-1px+context-menu hardware-cursor=preferred-physical-source/concurrent cadence_hz={} cadence_clock=absolute-fractional wake=input-or-frame-state-change coalesce=display-cadence damage=ordered-linear-diff+window-old-new gpu_submits=0 synthetic-motion=off\n",
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
                    commit_presented_slot4(&mut state, pending.rects, pending.windows);
                    release_window_leases(&pending.leases);
                    state.consecutive_present_failures = 0;
                }
                crate::intel::Ui4LiveOverlayFlipPoll::Failed => {
                    release_window_leases(&pending.leases);
                    note_present_failure(&mut state, pending.rects.len());
                    visual_dirty = true;
                    next_present = cadence.next_deadline();
                }
            }
        }

        if state.pending.is_none() && visual_dirty && now >= next_present {
            let rects = software_cursor_rects();
            let windows = slot4_windows();
            match queue_slot4(&mut state, &rects, &windows) {
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
    windows: &[super::WindowSnapshot],
) -> Result<Option<PendingSlot4Present>, ()> {
    let window_stamps = windows
        .iter()
        .copied()
        .map(slot4_window_stamp)
        .collect::<Vec<_>>();
    let mut damage = changed_rect_damage(&state.previous_rects, rects);
    add_changed_window_damage(&mut damage, &state.previous_windows, &window_stamps);
    if damage.is_empty() {
        return Ok(None);
    }

    let mut leases = Vec::with_capacity(windows.len());
    for window in windows {
        match super::acquire_published_frame(window.frame) {
            Ok(lease) => leases.push(lease),
            Err(_) => {
                release_window_leases(&leases);
                return Err(());
            }
        }
    }
    let result = (|| {
        let views = leases
            .iter()
            .copied()
            .map(super::published_rgba_view)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ())?;
        if views.iter().any(|view| view.gpu_authored) {
            crate::log_warn!(target: "ui4/slot4";
                "ui4/slot4: interaction-window rejected reason=gpu-authored-source windows={} policy=cpu-authored-slot4-only fallback_application_plane=0\n",
                windows.len(),
            );
            return Err(());
        }
        for view in &views {
            crate::intel::dma_flush(view.virt, view.byte_len);
        }
        let pixels = views
            .iter()
            .map(|view| unsafe {
                // SAFETY: the matching published-frame lease remains pinned
                // through the synchronous slot-4 raster copy below.
                core::slice::from_raw_parts(view.virt.cast_const(), view.byte_len)
            })
            .collect::<Vec<_>>();
        let tiles = windows
            .iter()
            .zip(views.iter())
            .zip(pixels.iter())
            .map(|((window, view), pixels)| crate::intel::RgbaOverlayTile {
                x: window.placement.x.max(0) as u32,
                y: window.placement.y.max(0) as u32,
                width: window.placement.width,
                height: window.placement.height,
                source_width: view.width,
                source_height: view.height,
                pitch_bytes: view.pitch as usize,
                pixels,
                gpgpu_surface: None,
                gpgpu_scanout_cache: false,
                opacity: window.placement.opacity,
                known_opaque: false,
                expected_rgba: None,
            })
            .collect::<Vec<_>>();
        crate::intel::queue_ui4_live_overlay_scene_on_slot_damage_region(
            super::INTERACTION_OVERLAY_PLANE_SLOT,
            &tiles,
            rects,
            damage,
            "ui4-slot4-interaction",
        )
        .ok_or(())
    })();
    let flip = match result {
        Ok(flip) => flip,
        Err(()) => {
            release_window_leases(&leases);
            return Err(());
        }
    };
    Ok(Some(PendingSlot4Present {
        flip,
        rects: rects.clone(),
        windows: window_stamps,
        leases,
    }))
}

fn commit_presented_slot4(
    state: &mut Slot4State,
    rects: Slot4Rects,
    windows: Vec<Slot4WindowStamp>,
) {
    for window in &windows {
        let _ = super::window_broker::acknowledge_window_frame_revision(
            window.id,
            window.publish_serial,
            window.revision,
        );
    }
    state.previous_rects = rects.clone();
    state.previous_windows = windows;
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

fn slot4_windows() -> Vec<super::WindowSnapshot> {
    let Some(output) = super::OutputId::from_slot(0) else {
        return Vec::new();
    };
    super::window_broker::interaction_windows_for_output(output)
}

fn slot4_window_stamp(window: super::WindowSnapshot) -> Slot4WindowStamp {
    Slot4WindowStamp {
        id: window.id,
        frame: window.frame,
        publish_serial: window.publish_serial,
        revision: window.revision,
        placement: window.placement,
    }
}

fn add_changed_window_damage(
    damage: &mut crate::intel::CompositionDamageRegion,
    previous: &[Slot4WindowStamp],
    current: &[Slot4WindowStamp],
) {
    for current_window in current {
        match previous
            .iter()
            .find(|previous_window| previous_window.id == current_window.id)
        {
            Some(previous_window) if previous_window == current_window => {}
            Some(previous_window) => {
                add_window_stamp_damage(damage, *previous_window);
                add_window_stamp_damage(damage, *current_window);
            }
            None => add_window_stamp_damage(damage, *current_window),
        }
    }
    for previous_window in previous {
        if !current
            .iter()
            .any(|current_window| current_window.id == previous_window.id)
        {
            add_window_stamp_damage(damage, *previous_window);
        }
    }
}

fn add_window_stamp_damage(
    damage: &mut crate::intel::CompositionDamageRegion,
    window: Slot4WindowStamp,
) {
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));
    let left = i64::from(window.placement.x).clamp(0, i64::from(screen_width));
    let top = i64::from(window.placement.y).clamp(0, i64::from(screen_height));
    let right = i64::from(window.placement.x)
        .saturating_add(i64::from(window.placement.width))
        .clamp(0, i64::from(screen_width));
    let bottom = i64::from(window.placement.y)
        .saturating_add(i64::from(window.placement.height))
        .clamp(0, i64::from(screen_height));
    if right <= left || bottom <= top {
        return;
    }
    damage.add(crate::intel::CompositionDamageRect::new(
        left as u32,
        top as u32,
        (right - left) as u32,
        (bottom - top) as u32,
    ));
}

fn release_window_leases(leases: &[super::FrameReadLease]) {
    for lease in leases {
        let _ = super::release_published_frame(*lease);
    }
}

fn software_cursor_rects() -> Slot4Rects {
    use crate::graphics::primitives::Rgba8;

    let visuals = super::software_cursor_visuals();
    let mut rects = Slot4Rects::new();
    let (screen_w, screen_h) = crate::intel::active_scanout_dimensions().unwrap_or((2560, 1440));

    if let Some(output) = super::OutputId::from_slot(0) {
        for strip in super::selection_strips(output, screen_w, screen_h) {
            push_overlay_rect(&mut rects, strip.x, strip.y, strip.width, 1, strip.color);
        }
    }

    for visual in &visuals {
        if let Some(selection) = visual.selection {
            push_selection_outline(&mut rects, selection, visual.color);
        }
        if let Some(maximize_preview) = visual.maximize_preview {
            push_selection_outline(&mut rects, maximize_preview, visual.color);
        }
    }

    for visual in &visuals {
        let Some((x, y)) = visual.context_menu else {
            continue;
        };
        let menu_rect = super::input_broker::context_menu_rect((x, y), screen_w, screen_h);
        let row_height = super::input_broker::CONTEXT_MENU_ROW_HEIGHT_PX;
        let horizontal_inset = super::input_broker::DESKTOP_CONTEXT_MENU_HORIZONTAL_INSET_PX;
        let vertical_inset = super::input_broker::DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX;
        let row_text_y = 0;
        let row_top = menu_rect.y.saturating_add(vertical_inset);
        push_overlay_rect(
            &mut rects,
            menu_rect.x,
            menu_rect.y,
            menu_rect.width,
            menu_rect.height,
            Rgba8::new(22, 25, 33, 235),
        );
        push_rect_border(&mut rects, menu_rect, 1, visual.color);
        for row in 1..super::input_broker::DESKTOP_CONTEXT_MENU_ENTRY_COUNT {
            push_overlay_rect(
                &mut rects,
                menu_rect.x.saturating_add(horizontal_inset),
                row_top.saturating_add(row.saturating_mul(row_height)),
                menu_rect
                    .width
                    .saturating_sub(horizontal_inset.saturating_mul(2)),
                1,
                Rgba8::new(180, 188, 204, 150),
            );
        }
        for (row, label) in [(0u32, "COLOR PICKER"), (1u32, "SHELL")] {
            let y = row_top
                .saturating_add(row.saturating_mul(row_height))
                .saturating_add(row_text_y);
            let width = menu_rect
                .width
                .saturating_sub(horizontal_inset.saturating_mul(2));
            let x = menu_rect.x.saturating_add(horizontal_inset);
            if row == 0 {
                push_microfont_rainbow_menu_text(&mut rects, x, y, width, label);
            } else {
                push_microfont_menu_text(
                    &mut rects,
                    x,
                    y,
                    width,
                    label,
                    Rgba8::new(255, 55, 255, 255),
                );
            }
        }
    }

    if let Some(menu) = super::context_menu::visual() {
        push_requested_context_menu(&mut rects, &menu, screen_w, screen_h);
    }

    for visual in &visuals {
        push_software_crosshair(&mut rects, visual.x, visual.y, screen_w, screen_h, visual.color);
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
    push_rect_border(rects, menu_rect, 1, menu.color);

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
        let scaled_font_height = (microfont::FHEIGHT as u32).saturating_mul(2);
        let row_text_y = row.height.saturating_sub(scaled_font_height) / 2;
        push_microfont_menu_text(
            rects,
            row.x
                .saturating_add(super::context_menu::MENU_TEXT_INSET_PX),
            row.y.saturating_add(row_text_y),
            row.width.saturating_sub(super::context_menu::MENU_TEXT_INSET_PX),
            entry.label.as_str(),
            text_color,
        );
    }
}

fn push_microfont_menu_text(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    width: u32,
    text: &str,
    color: crate::graphics::primitives::Rgba8,
) {
    let text = menu_label_prefix(text, super::context_menu::MENU_RENDER_LABEL_CHARS);
    if text.is_empty() || width == 0 {
        return;
    }

    let width = usize::try_from(width).ok().unwrap_or(0);
    let height = microfont::FHEIGHT;
    let Some(pixel_count) = width.checked_mul(height) else {
        return;
    };
    let mut pixels = vec![0u8; pixel_count];
    if microfont::stamp_text(
        &mut pixels,
        width,
        height,
        0,
        0,
        text,
        1u8,
    )
    .is_err()
    {
        return;
    }

    for (row, scanline) in pixels.chunks_exact(width).take(height).enumerate() {
        let mut col = 0usize;
        let y = y.saturating_add(
            u32::try_from(row)
                .unwrap_or(0)
                .saturating_mul(2),
        );
        while col < width {
            if scanline[col] == 0 {
                col = col.saturating_add(1);
                continue;
            }
            let run_start = col;
            while col < width && scanline[col] != 0 {
                col = col.saturating_add(1);
            }
            push_overlay_rect(
                rects,
                x.saturating_add(
                    u32::try_from(run_start)
                        .unwrap_or(0)
                        .saturating_mul(2),
                ),
                y,
                u32::try_from(col.saturating_sub(run_start)).unwrap_or(0).saturating_mul(2),
                2,
                color,
            );
        }
    }
}

fn push_microfont_rainbow_menu_text(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    width: u32,
    text: &str,
) {
    let text = menu_label_prefix(text, super::context_menu::MENU_RENDER_LABEL_CHARS);
    if text.is_empty() || width == 0 {
        return;
    }

    let width = usize::try_from(width).ok().unwrap_or(0);
    let height = microfont::FHEIGHT;
    let Some(pixel_count) = width.checked_mul(height) else {
        return;
    };
    let mut pixels = vec![0u8; pixel_count];
    if microfont::stamp_text(
        &mut pixels,
        width,
        height,
        0,
        0,
        text,
        1u8,
    )
    .is_err()
    {
        return;
    }

    let width_u32 = u32::try_from(width).ok().unwrap_or(0);
    for (row, scanline) in pixels.chunks_exact(width).take(height).enumerate() {
        let mut col = 0usize;
        let y = y.saturating_add(
            u32::try_from(row)
                .unwrap_or(0)
                .saturating_mul(2),
        );
        while col < width {
            if scanline[col] == 0 {
                col = col.saturating_add(1);
                continue;
            }
            let run_start = col;
            while col < width && scanline[col] != 0 {
                col = col.saturating_add(1);
            }
            let run_width = col.saturating_sub(run_start);
            let run_midpoint = run_start.saturating_add(run_width / 2);
            let hue = if width_u32 == 0 {
                0
            } else {
                (u32::try_from(run_midpoint)
                    .unwrap_or(0)
                    .saturating_mul(360))
                    .saturating_div(width_u32)
            };
            let color = rainbow_color(hue);
            push_overlay_rect(
                rects,
                x.saturating_add(
                    u32::try_from(run_start)
                        .unwrap_or(0)
                        .saturating_mul(2),
                ),
                y,
                u32::try_from(run_width).unwrap_or(0).saturating_mul(2),
                2,
                color,
            );
        }
    }
}

fn rainbow_color(hue: u32) -> crate::graphics::primitives::Rgba8 {
    let hue = hue % 360;
    let c = 255u32;
    let sector = hue / 60;
    let within_sector = hue % 60;
    let two_sector = within_sector.saturating_mul(2);
    let x = {
        let diff = if two_sector >= 60 {
            two_sector.saturating_sub(60)
        } else {
            60u32.saturating_sub(two_sector)
        };
        (c.saturating_mul(60u32.saturating_sub(diff))).saturating_div(60)
    };
    let (r, g, b) = match sector {
        0 => (c, x, 0),
        1 => (x, c, 0),
        2 => (0, c, x),
        3 => (0, x, c),
        4 => (x, 0, c),
        _ => (c, 0, x),
    };
    crate::graphics::primitives::Rgba8::new(r as u8, g as u8, b as u8, 255)
}

fn menu_label_prefix<'a>(text: &'a str, char_limit: usize) -> &'a str {
    if char_limit == 0 {
        return "";
    }
    let mut chars = text.char_indices();
    match chars.nth(char_limit) {
        Some((end, _)) => &text[..end],
        None => text,
    }
}

/// Draw a centered cross in the interior and deliberately morph it into an
/// inward-facing T at every display edge. Each component is clamped separately
/// so the top/left saturation effect is mirrored at the bottom/right, while no
/// cursor rectangle or its damage can extend beyond the scanout.
fn push_software_crosshair(
    rects: &mut Slot4Rects,
    x: u32,
    y: u32,
    screen_w: u32,
    screen_h: u32,
    color: crate::graphics::primitives::Rgba8,
) {
    let long_before = SOFTWARE_CURSOR_LONG_PX / 2;

    push_cursor_rect(
        rects,
        x,
        y,
        2,
        long_before,
        SOFTWARE_CURSOR_STROKE_PX,
        SOFTWARE_CURSOR_LONG_PX,
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
        SOFTWARE_CURSOR_LONG_PX,
        SOFTWARE_CURSOR_STROKE_PX,
        screen_w,
        screen_h,
        color,
    );
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

fn push_selection_outline(
    rects: &mut Slot4Rects,
    rect: super::Ui4VisualRect,
    color: crate::graphics::primitives::Rgba8,
) {
    push_rect_border(rects, rect, 1, color);
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

    fn test_cursor_rects(x: u32, y: u32) -> Slot4Rects {
        let mut rects = Slot4Rects::new();
        push_software_crosshair(
            &mut rects,
            x,
            y,
            TEST_SCREEN_W,
            TEST_SCREEN_H,
            Rgba8::new(255, 0, 0, 255),
        );
        rects
    }

    fn test_window_stamp(
        id: u32,
        publish_serial: u64,
        revision: u64,
        x: i32,
        opacity: u8,
    ) -> Slot4WindowStamp {
        Slot4WindowStamp {
            id: super::super::WindowId::from_raw(id).unwrap(),
            frame: super::super::FrameHandle::from_raw(id as u64).unwrap(),
            publish_serial,
            revision,
            placement: super::super::WindowPlacement {
                x,
                y: 20,
                width: 40,
                height: 30,
                z: 100,
                opacity,
                visible: true,
            },
        }
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
    fn interaction_window_damage_tracks_publish_move_and_removal() {
        let first = test_window_stamp(1, 1, 1, 10, u8::MAX);
        let published = test_window_stamp(1, 2, 2, 10, u8::MAX);
        let moved = test_window_stamp(1, 2, 3, 100, 128);

        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_changed_window_damage(&mut damage, &[], &[first]);
        assert_eq!(
            damage.bounding_rect(),
            Some(crate::intel::CompositionDamageRect::new(10, 20, 40, 30))
        );

        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_changed_window_damage(&mut damage, &[first], &[published]);
        assert_eq!(
            damage.bounding_rect(),
            Some(crate::intel::CompositionDamageRect::new(10, 20, 40, 30))
        );

        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_changed_window_damage(&mut damage, &[published], &[moved]);
        assert_eq!(damage.len(), 2);
        assert_eq!(
            damage.bounding_rect(),
            Some(crate::intel::CompositionDamageRect::new(10, 20, 130, 30))
        );

        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_changed_window_damage(&mut damage, &[moved], &[]);
        assert_eq!(
            damage.bounding_rect(),
            Some(crate::intel::CompositionDamageRect::new(100, 20, 40, 30))
        );
    }

    #[test]
    fn unchanged_interaction_window_adds_no_damage() {
        let window = test_window_stamp(1, 7, 11, 10, 200);
        let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
        add_changed_window_damage(&mut damage, &[window], &[window]);
        assert!(damage.is_empty());
    }

    #[test]
    fn software_cursor_shape_is_mirrored_at_opposite_edges() {
        let left = test_cursor_rects(0, TEST_SCREEN_H / 2);
        let right = test_cursor_rects(TEST_SCREEN_W - 1, TEST_SCREEN_H / 2);
        let top = test_cursor_rects(TEST_SCREEN_W / 2, 0);
        let bottom = test_cursor_rects(TEST_SCREEN_W / 2, TEST_SCREEN_H - 1);

        assert_eq!(left.len(), 2);
        assert_eq!(right.len(), left.len());
        assert_eq!(top.len(), 2);
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
        let rects = test_cursor_rects(50, 40);
        let expected = [
            (48, 27, SOFTWARE_CURSOR_STROKE_PX, SOFTWARE_CURSOR_LONG_PX),
            (37, 38, SOFTWARE_CURSOR_LONG_PX, SOFTWARE_CURSOR_STROKE_PX),
        ];

        assert_eq!(rects.len(), expected.len());
        for (rect, (x, y, width, height)) in rects.iter().zip(expected) {
            assert_eq!((rect.x, rect.y, rect.width, rect.height), (x, y, width, height));
            assert_eq!(rect.color, Rgba8::new(255, 0, 0, 255));
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
            let rects = test_cursor_rects(x, y);
            assert_eq!(rects.len(), 2);
            for rect in rects {
                assert!(rect.x.saturating_add(rect.width) <= TEST_SCREEN_W);
                assert!(rect.y.saturating_add(rect.height) <= TEST_SCREEN_H);
            }
        }
    }

    #[test]
    fn selection_outline_is_one_pixel_and_non_overlapping() {
        let mut rects = Slot4Rects::new();
        push_selection_outline(
            &mut rects,
            super::super::Ui4VisualRect {
                x: 10,
                y: 20,
                width: 30,
                height: 20,
            },
            Rgba8::new(255, 0, 0, 255),
        );

        let expected = [
            (10, 20, 30, 1),
            (10, 39, 30, 1),
            (10, 21, 1, 18),
            (39, 21, 1, 18),
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
