//! Permanent kernel UI4 compositor service.
//!
//! Producers own frames and windows. This service owns the broker snapshot,
//! per-plane damage history, software-cursor composition, and the atomic plane
//! surface-flip batch. It intentionally creates no application windows.

use alloc::vec::Vec;

use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::r::mouse_motion_service::{
    MOUSE_CONTROL_EASING_FAST_LINEAR, MOUSE_CONTROL_EASING_NATURAL, MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
    MOUSE_CONTROL_OPCODE_STROKE, MOUSE_CONTROL_OPCODE_TELEPORT, MOUSE_CONTROL_PATH_CUBIC,
    MOUSE_CONTROL_PATH_LINE, MouseControlCommand, MouseControlCursor, MouseControlError,
    MouseControlPrincipal, cursor_is_idle, release_cursor, request_cursor, submit_program,
};

use super::{
    DamageRect, FrameHandle, FramePoolError, FrameReadLease, FrameRgbaView, OutputId, WindowId,
    WindowPlacement, WindowSnapshot, acknowledge_window_frame, acquire_published_frame,
    published_rgba_view, release_published_frame, software_cursor_visuals,
    visible_windows_for_output,
};

const COMPOSITION_PERIOD_MS: u64 = 16;
const HEARTBEAT_REST_FRAMES: u32 = 50;
const MAX_COMPOSITION_WINDOWS: usize = super::window_broker::MAX_ACTIVE_WINDOWS;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum Ui4CompositorError {
    Frame(FramePoolError),
    PresentFailed,
    MouseMotion(MouseControlError),
}

impl From<FramePoolError> for Ui4CompositorError {
    fn from(error: FramePoolError) -> Self {
        Self::Frame(error)
    }
}

impl From<MouseControlError> for Ui4CompositorError {
    fn from(error: MouseControlError) -> Self {
        Self::MouseMotion(error)
    }
}

struct Runtime {
    heartbeat_cursor: MouseControlCursor,
    heartbeat_rest_frames: u32,
    composition: CompositionState,
    cursor_plane: CursorPlaneState,
}

#[derive(Copy, Clone)]
struct CompositionState {
    primary: PlaneCompositionState,
    solara: PlaneCompositionState,
    draw3d: PlaneCompositionState,
}

#[derive(Copy, Clone)]
struct PlaneCompositionState {
    initialized: bool,
    windows: [Option<CompositionWindowStamp>; MAX_COMPOSITION_WINDOWS],
}

#[derive(Copy, Clone)]
enum CompositionTarget {
    Primary,
    Overlay(usize),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CompositionWindowStamp {
    id: WindowId,
    frame: FrameHandle,
    publish_serial: u64,
    placement: WindowPlacement,
}

#[derive(Copy, Clone)]
struct CursorPlaneState {
    previous_bounds: Option<crate::intel::CompositionDamageRect>,
    signature: u64,
    initialized: bool,
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_compositor_service_task() {
    crate::log_info!(
        target: "ui4";
        "ui4 compositor carrier online placement=ap1-ui-core expected_slot={} current_slot={}\n",
        crate::workers::AP1_UI_SERVICE_SLOT,
        crate::percpu::current_slot()
    );
    crate::intel::wait_hw_logo_sequence_done().await;
    let mut runtime = match initialize() {
        Ok(runtime) => runtime,
        Err(error) => {
            crate::log_error!(
                target: "ui4";
                "ui4 compositor init failed error={:?}\n",
                error
            );
            return;
        }
    };

    crate::log_info!(
        target: "ui4";
        "ui4 compositor live application_windows=broker-owned composition_ms={} planes=primary+slot2+slot3 interaction=slot4 input=ui4-owner-queues heartbeat_vcursor_slot={}\n",
        COMPOSITION_PERIOD_MS,
        runtime.heartbeat_cursor.slot_id
    );

    loop {
        if runtime.heartbeat_rest_frames != 0 {
            runtime.heartbeat_rest_frames -= 1;
        } else {
            let idle = match cursor_is_idle(
                MouseControlPrincipal::KernelApp(1),
                runtime.heartbeat_cursor.handle,
            ) {
                Ok(idle) => idle,
                Err(error) => {
                    fail_and_cleanup(runtime, error.into());
                    return;
                }
            };
            if idle {
                if let Err(error) = submit_heartbeat_program(runtime.heartbeat_cursor) {
                    fail_and_cleanup(runtime, error);
                    return;
                }
                runtime.heartbeat_rest_frames = HEARTBEAT_REST_FRAMES;
            }
        }

        if let Err(error) = present_composition(&mut runtime) {
            fail_and_cleanup(runtime, error);
            return;
        }

        Timer::after(EmbassyDuration::from_millis(COMPOSITION_PERIOD_MS)).await;
    }
}

fn initialize() -> Result<Runtime, Ui4CompositorError> {
    let heartbeat_cursor = request_cursor(MouseControlPrincipal::KernelApp(1), "ui4-heartbeat")?;
    let mut runtime = Runtime {
        heartbeat_cursor,
        heartbeat_rest_frames: 0,
        composition: CompositionState {
            primary: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            solara: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
            draw3d: PlaneCompositionState {
                initialized: false,
                windows: [None; MAX_COMPOSITION_WINDOWS],
            },
        },
        cursor_plane: CursorPlaneState {
            previous_bounds: None,
            signature: 0,
            initialized: false,
        },
    };
    if let Err(error) = present_composition(&mut runtime) {
        release_runtime_resources(&runtime);
        return Err(error);
    }

    Ok(runtime)
}

fn submit_heartbeat_program(cursor: MouseControlCursor) -> Result<(), Ui4CompositorError> {
    let teleport = MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_TELEPORT,
        flags: MOUSE_CONTROL_FLAG_CLEAR_QUEUE,
        x: 1380,
        y: 1220,
        ..MouseControlCommand::default()
    };
    let line = |x, y, duration_ms| MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_LINE,
        easing: MOUSE_CONTROL_EASING_FAST_LINEAR,
        duration_ms,
        x,
        y,
        ..MouseControlCommand::default()
    };
    let accent = |x, y, c1x, c1y, c2x, c2y, duration_ms| MouseControlCommand {
        opcode: MOUSE_CONTROL_OPCODE_STROKE,
        path: MOUSE_CONTROL_PATH_CUBIC,
        easing: MOUSE_CONTROL_EASING_NATURAL,
        duration_ms,
        x,
        y,
        control1_x: c1x,
        control1_y: c1y,
        control2_x: c2x,
        control2_y: c2y,
        ..MouseControlCommand::default()
    };
    let program = [
        teleport,
        line(1430, 1220, 120),
        accent(1470, 1192, 1440, 1220, 1456, 1192, 100),
        accent(1500, 1268, 1480, 1192, 1490, 1268, 110),
        accent(1535, 1130, 1510, 1268, 1520, 1130, 125),
        accent(1570, 1220, 1545, 1130, 1555, 1220, 120),
        line(1640, 1220, 150),
    ];
    submit_program(MouseControlPrincipal::KernelApp(1), cursor.handle, &program)?;
    Ok(())
}

fn present_composition(runtime: &mut Runtime) -> Result<(), Ui4CompositorError> {
    let output = OutputId::from_slot(0).expect("UI4 D01 must exist");
    super::advance_window_close_transitions();
    let windows = visible_windows_for_output(output);
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        // `ui4` deliberately has no narrower LogArea, so log-os routes this
        // admission failure to [global] [warn].
        crate::log_warn!(
            target: "ui4";
            "ui4 compositor visible-window soft-cap exceeded output={} requested={} cap={} action=reject-composition\n",
            output.name(),
            windows.len(),
            MAX_COMPOSITION_WINDOWS,
        );
        return Err(Ui4CompositorError::PresentFailed);
    }
    if windows.iter().any(|window| {
        let slot = window.plane.slot();
        slot != super::PRIMARY_PLANE_SLOT
            && slot != super::RGB_OVERLAY_PLANE_SLOT_2
            && slot != super::RGB_OVERLAY_PLANE_SLOT_3
    }) {
        return Err(Ui4CompositorError::PresentFailed);
    }

    if !crate::intel::begin_ui4_plane_surface_flip_batch() {
        return Err(Ui4CompositorError::PresentFailed);
    }
    let cursor_rects = software_cursor_rects();
    let present_result = (|| {
        present_plane_composition(
            &mut runtime.composition.primary,
            &windows,
            CompositionTarget::Primary,
        )?;
        present_plane_composition(
            &mut runtime.composition.solara,
            &windows,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_2),
        )?;
        present_plane_composition(
            &mut runtime.composition.draw3d,
            &windows,
            CompositionTarget::Overlay(super::RGB_OVERLAY_PLANE_SLOT_3),
        )?;
        present_software_cursor_plane(&mut runtime.cursor_plane, &cursor_rects)
    })();
    let flips_committed = crate::intel::finish_ui4_plane_surface_flip_batch();
    present_result?;
    if !flips_committed {
        return Err(Ui4CompositorError::PresentFailed);
    }
    super::screenshot::capture_compositor_frame(&windows, &cursor_rects);
    Ok(())
}

fn present_plane_composition(
    state: &mut PlaneCompositionState,
    all_windows: &[WindowSnapshot],
    target: CompositionTarget,
) -> Result<(), Ui4CompositorError> {
    let plane_slot = match target {
        CompositionTarget::Primary => super::PRIMARY_PLANE_SLOT,
        CompositionTarget::Overlay(slot) => slot,
    };
    let windows: Vec<WindowSnapshot> = all_windows
        .iter()
        .copied()
        .filter(|window| window.plane.slot() == plane_slot)
        .collect();
    if windows.len() > MAX_COMPOSITION_WINDOWS {
        return Err(Ui4CompositorError::PresentFailed);
    }
    if windows.is_empty() && !state.initialized {
        return Ok(());
    }

    let mut next_windows = [None; MAX_COMPOSITION_WINDOWS];
    for (slot, window) in windows.iter().enumerate() {
        next_windows[slot] = Some(CompositionWindowStamp {
            id: window.id,
            frame: window.frame,
            publish_serial: window.publish_serial,
            placement: window.placement,
        });
    }

    let mut changed = [false; MAX_COMPOSITION_WINDOWS];
    let mut content_damage = [false; MAX_COMPOSITION_WINDOWS];
    let mut composition_changed = !state.initialized;
    let (output_width, output_height) = crate::intel::active_scanout_dimensions().unwrap_or((0, 0));
    let mut damage = crate::intel::CompositionDamageRegion::EMPTY;
    for (slot, current) in next_windows.iter().flatten().enumerate() {
        let previous = state
            .windows
            .iter()
            .flatten()
            .find(|previous| previous.id == current.id);
        changed[slot] = !state.initialized || previous != Some(current);
        composition_changed |= changed[slot];
        match previous {
            None => {
                add_placement_damage(&mut damage, current.placement, output_width, output_height)
            }
            Some(previous) if previous.placement != current.placement => {
                add_placement_damage(&mut damage, previous.placement, output_width, output_height);
                add_placement_damage(&mut damage, current.placement, output_width, output_height);
            }
            Some(previous) if previous.frame != current.frame => {
                add_placement_damage(&mut damage, current.placement, output_width, output_height)
            }
            Some(previous) if previous.publish_serial != current.publish_serial => {
                content_damage[slot] = true;
            }
            Some(_) => {}
        }
    }
    for previous in state.windows.iter().flatten() {
        if !next_windows
            .iter()
            .flatten()
            .any(|current| current.id == previous.id)
        {
            composition_changed = true;
            add_placement_damage(&mut damage, previous.placement, output_width, output_height);
        }
    }
    if !state.initialized {
        damage = crate::intel::CompositionDamageRegion::from_rect(
            crate::intel::CompositionDamageRect::new(0, 0, output_width, output_height),
        );
    }

    if composition_changed {
        let mut leases = Vec::with_capacity(windows.len());
        for window in &windows {
            match acquire_published_frame(window.frame) {
                Ok(lease) => leases.push(lease),
                Err(error) => {
                    release_leases(&leases);
                    return Err(error.into());
                }
            }
        }

        let result = (|| {
            let views: Vec<FrameRgbaView> = leases
                .iter()
                .copied()
                .map(published_rgba_view)
                .collect::<Result<_, _>>()?;
            for (slot, (window, view)) in windows.iter().zip(views.iter()).enumerate() {
                if !content_damage[slot] {
                    continue;
                }
                let local = window
                    .damage
                    .unwrap_or_else(|| super::DamageRegion::from_rect(DamageRect::FULL));
                add_mapped_window_damage(
                    &mut damage,
                    local,
                    *view,
                    window.placement,
                    output_width,
                    output_height,
                );
            }
            for (slot, view) in views.iter().enumerate() {
                if changed[slot] {
                    crate::intel::dma_flush(view.virt, view.byte_len);
                }
            }
            let pixels: Vec<&[u8]> = views
                .iter()
                .map(|view| unsafe {
                    core::slice::from_raw_parts(
                        view.virt.cast_const(),
                        (view.pitch as usize).saturating_mul(view.height as usize),
                    )
                })
                .collect();
            let tiles: Vec<_> = windows
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
                    opacity: window.placement.opacity,
                    expected_rgba: None,
                })
                .collect();
            if !damage.is_empty() {
                let presented = match target {
                    CompositionTarget::Primary => {
                        crate::intel::present_premultiplied_rgba_primary_tiles_damage(
                            &tiles,
                            damage,
                            "ui4-compositor-primary",
                        )
                    }
                    CompositionTarget::Overlay(slot) => {
                        let reason = match slot {
                            super::RGB_OVERLAY_PLANE_SLOT_2 => "ui4-solara-slot2",
                            super::RGB_OVERLAY_PLANE_SLOT_3 => "ui4-draw3d-slot3",
                            _ => "ui4-overlay",
                        };
                        crate::intel::present_premultiplied_rgba_overlay_tiles_on_slot_damage(
                            slot, &tiles, damage, reason,
                        )
                    }
                };
                if !presented {
                    return Err(Ui4CompositorError::PresentFailed);
                }
            }
            Ok(())
        })();
        release_leases(&leases);
        result?;
        for window in &windows {
            let _ = acknowledge_window_frame(window.id, window.publish_serial);
        }
        state.initialized = true;
        state.windows = next_windows;
    }
    Ok(())
}

fn software_cursor_rects() -> heapless::Vec<crate::intel::LiveOverlayRect, 512> {
    use crate::graphics::primitives::Rgba8;

    let visuals = software_cursor_visuals();
    let mut rects: heapless::Vec<crate::intel::LiveOverlayRect, 512> = heapless::Vec::new();

    // Keep selection deliberately trim: four one-pixel edges on the same
    // topmost interaction plane as its owning cursor, with no filled area to
    // obscure application content.
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

    // Every source becomes visible only after its first real movement. The
    // color remains bound to the full HID source identity, not to focus.
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

fn present_software_cursor_plane(
    state: &mut CursorPlaneState,
    rects: &[crate::intel::LiveOverlayRect],
) -> Result<(), Ui4CompositorError> {
    let current_bounds = overlay_rect_bounds(rects);
    let signature = overlay_rect_signature(rects);
    if state.initialized && signature == state.signature && current_bounds == state.previous_bounds
    {
        return Ok(());
    }
    let damage = match (state.previous_bounds, current_bounds) {
        (Some(previous), Some(current)) => damage_union(previous, current),
        (Some(previous), None) => previous,
        (None, Some(current)) => current,
        (None, None) => return Ok(()),
    };
    if crate::intel::present_live_overlay_rects_on_slot_damage(
        super::INTERACTION_OVERLAY_PLANE_SLOT,
        rects,
        damage,
        "ui4-interaction-slot4",
    ) {
        state.previous_bounds = current_bounds;
        state.signature = signature;
        state.initialized = true;
        Ok(())
    } else {
        Err(Ui4CompositorError::PresentFailed)
    }
}

fn damage_union(
    a: crate::intel::CompositionDamageRect,
    b: crate::intel::CompositionDamageRect,
) -> crate::intel::CompositionDamageRect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let right = a.x.saturating_add(a.width).max(b.x.saturating_add(b.width));
    let bottom =
        a.y.saturating_add(a.height)
            .max(b.y.saturating_add(b.height));
    crate::intel::CompositionDamageRect::new(
        x,
        y,
        right.saturating_sub(x),
        bottom.saturating_sub(y),
    )
}

fn add_placement_damage(
    region: &mut crate::intel::CompositionDamageRegion,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) {
    if let Some(rect) = clipped_output_rect(
        i64::from(placement.x),
        i64::from(placement.y),
        i64::from(placement.x).saturating_add(i64::from(placement.width)),
        i64::from(placement.y).saturating_add(i64::from(placement.height)),
        output_width,
        output_height,
    ) {
        region.add(rect);
    }
}

fn add_mapped_window_damage(
    output: &mut crate::intel::CompositionDamageRegion,
    local: super::DamageRegion,
    view: FrameRgbaView,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) {
    for rect in local.rects() {
        if let Some(rect) = map_window_damage_rect(
            *rect,
            view.width,
            view.height,
            placement,
            output_width,
            output_height,
        ) {
            output.add(rect);
        }
    }
}

fn map_window_damage_rect(
    local: DamageRect,
    source_width: u32,
    source_height: u32,
    placement: WindowPlacement,
    output_width: u32,
    output_height: u32,
) -> Option<crate::intel::CompositionDamageRect> {
    if source_width == 0 || source_height == 0 || placement.width == 0 || placement.height == 0 {
        return None;
    }
    let local = local.intersection(DamageRect::new(0, 0, source_width, source_height))?;
    let source_right = local.x.saturating_add(local.width);
    let source_bottom = local.y.saturating_add(local.height);
    let destination_left = scale_floor(local.x, placement.width, source_width);
    let destination_top = scale_floor(local.y, placement.height, source_height);
    let destination_right = scale_ceil(source_right, placement.width, source_width);
    let destination_bottom = scale_ceil(source_bottom, placement.height, source_height);
    clipped_output_rect(
        i64::from(placement.x).saturating_add(i64::from(destination_left)),
        i64::from(placement.y).saturating_add(i64::from(destination_top)),
        i64::from(placement.x).saturating_add(i64::from(destination_right)),
        i64::from(placement.y).saturating_add(i64::from(destination_bottom)),
        output_width,
        output_height,
    )
}

fn scale_floor(coordinate: u32, destination_extent: u32, source_extent: u32) -> u32 {
    (u64::from(coordinate).saturating_mul(u64::from(destination_extent)) / u64::from(source_extent))
        .min(u64::from(u32::MAX)) as u32
}

fn scale_ceil(coordinate: u32, destination_extent: u32, source_extent: u32) -> u32 {
    let numerator = u64::from(coordinate).saturating_mul(u64::from(destination_extent));
    numerator
        .saturating_add(u64::from(source_extent).saturating_sub(1))
        .checked_div(u64::from(source_extent))
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32
}

fn clipped_output_rect(
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
    output_width: u32,
    output_height: u32,
) -> Option<crate::intel::CompositionDamageRect> {
    let left = left.clamp(0, i64::from(output_width));
    let top = top.clamp(0, i64::from(output_height));
    let right = right.clamp(0, i64::from(output_width));
    let bottom = bottom.clamp(0, i64::from(output_height));
    (right > left && bottom > top).then(|| {
        crate::intel::CompositionDamageRect::new(
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        )
    })
}

fn union_damage(
    current: Option<crate::intel::CompositionDamageRect>,
    next: crate::intel::CompositionDamageRect,
) -> Option<crate::intel::CompositionDamageRect> {
    Some(
        current
            .map(|current| damage_union(current, next))
            .unwrap_or(next),
    )
}

fn overlay_rect_bounds(
    rects: &[crate::intel::LiveOverlayRect],
) -> Option<crate::intel::CompositionDamageRect> {
    rects.iter().fold(None, |bounds, rect| {
        union_damage(
            bounds,
            crate::intel::CompositionDamageRect::new(rect.x, rect.y, rect.width, rect.height),
        )
    })
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

fn push_rect_border<const N: usize>(
    rects: &mut heapless::Vec<crate::intel::LiveOverlayRect, N>,
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

fn push_overlay_rect<const N: usize>(
    rects: &mut heapless::Vec<crate::intel::LiveOverlayRect, N>,
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

fn release_leases(leases: &[FrameReadLease]) {
    for lease in leases {
        let _ = release_published_frame(*lease);
    }
}

fn fail_and_cleanup(runtime: Runtime, error: Ui4CompositorError) {
    crate::log_error!(
        target: "ui4";
        "ui4 compositor stopped error={:?}\n",
        error
    );
    release_runtime_resources(&runtime);
}

fn release_runtime_resources(runtime: &Runtime) {
    let _ = release_cursor(MouseControlPrincipal::KernelApp(1), runtime.heartbeat_cursor.handle);
}

#[cfg(test)]
mod damage_tests {
    use super::*;

    #[test]
    fn producer_damage_scales_outward() {
        let placement = WindowPlacement {
            x: 10,
            y: 20,
            width: 33,
            height: 33,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        assert_eq!(
            map_window_damage_rect(DamageRect::new(1, 1, 1, 1), 100, 100, placement, 200, 200),
            Some(crate::intel::CompositionDamageRect::new(10, 20, 1, 1))
        );
    }

    #[test]
    fn producer_damage_clips_negative_placement() {
        let placement = WindowPlacement {
            x: -25,
            y: -10,
            width: 100,
            height: 80,
            z: 0,
            opacity: u8::MAX,
            visible: true,
        };
        assert_eq!(
            map_window_damage_rect(DamageRect::FULL, 100, 80, placement, 100, 100),
            Some(crate::intel::CompositionDamageRect::new(0, 0, 75, 70))
        );
    }
}
