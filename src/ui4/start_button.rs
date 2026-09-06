//! Always-resident TRUEOS menu button on UI4's interaction plane.
//!
//! The frame is kernel-owned rather than a Blueprint. Its physical extent is
//! derived from the active mode and EDID exactly like Gridpaper. FontKernel
//! produces the centered `§` coverage once; the AP1 UI service thereafter
//! changes only a small static sprite or the broker's whole-window opacity.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, Ui4InputEvent, WindowCreate,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionId,
    acquire_frame_buffer, begin_window_session, create_frame, create_window, destroy_frame,
    finish_window_session, publish_frame_buffer, publish_window_frame, set_window_placement,
    take_owner_input_events, window_placement, writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::START_BUTTON_SERVICE;
const BUTTON_WIDTH_MM: u32 = 20;
const BUTTON_HEIGHT_MM: u32 = 40;
const BUTTON_CORNER_RADIUS_MM: u32 = 4;
const BUTTON_GLYPH_PADDING_MM: u32 = 2;
/// The screen-edge reveal cap surrounding the attached button.
const REVEAL_CAP_MM: u32 = 5;
const FADE_DURATION_MS: u64 = 3_000;
const HOME_REVEAL_HOLD_MS: u64 = 5_000;
const RELEASE_STATE_MS: u64 = 180;
const SERVICE_PERIOD_MS: u64 = 16;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const SECONDARY_BUTTON_MASK: u32 = 1 << 1;

static REVEAL_SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum ButtonVisualState {
    Normal,
    Hover,
    Pressed,
    Released,
}

impl ButtonVisualState {
    const fn colors(self) -> ([u8; 3], [u8; 3]) {
        match self {
            Self::Normal => ([82, 87, 96], [248, 248, 250]),
            Self::Hover => ([232, 234, 239], [30, 34, 42]),
            Self::Pressed => ([38, 174, 92], [255, 255, 255]),
            Self::Released => ([54, 116, 242], [255, 255, 255]),
        }
    }
}

struct ActiveStartButton {
    _session: WindowSessionId,
    frame: FrameHandle,
    window: super::WindowId,
    screen_width: u32,
    screen_height: u32,
    width: u32,
    height: u32,
    corner_radius: u32,
    reveal_cap_x: u32,
    reveal_cap_y: u32,
    glyph_alpha: Vec<u8>,
    visual_state: ButtonVisualState,
    pressed_by: Option<Ui4CursorSource>,
    released_until_ms: u64,
    fade_started_ms: Option<u64>,
    home_visible_until_ms: u64,
    reveal_sequence: u32,
    opacity: u8,
    attached_right: bool,
}

/// Replace the former Win/Start-to-Shell shortcut with a reveal request.
/// Repeated presses are deliberately coalesced by the button service.
pub(crate) fn request_start_button_reveal() {
    REVEAL_SEQUENCE.fetch_add(1, Ordering::Release);
}

/// Keep this one special frame inside its left/right attachment bands while
/// retaining UI4's stock secondary-button drag and selection machinery.
pub(super) fn constrain_drag_placement(
    owner: WindowOwner,
    mut placement: WindowPlacement,
    cursor_x: u32,
    screen_width: u32,
) -> WindowPlacement {
    if owner == OWNER {
        placement.x = if cursor_x >= screen_width / 2 {
            screen_width.saturating_sub(placement.width) as i32
        } else {
            0
        };
    }
    placement
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_start_button_service_task() {
    crate::intel::gpu_font::wait_for_font_face_available(
        crate::intel::gpu_font::GpuFontFace::Default,
    )
    .await;

    let mut active = loop {
        match initialize_start_button() {
            Ok(active) => break active,
            Err(reason) => crate::log_warn!(target: "ui4/start-button";
                "ui4/start-button: initialization deferred reason={} retry_ms=1000\n",
                reason,
            ),
        }
        Timer::after(Duration::from_millis(1_000)).await;
    };

    crate::log_info!(target: "ui4/start-button";
        "ui4/start-button: service online carrier=ap1-ui-core owner=kernel-ui4 plane=slot4-interaction glyph=section-sign font=font extent={}x{} physical_mm={}x{} edge_cap_mm={} placement=left-or-right-attached drag=secondary/vertical-band select=stock click=primary-release action=shell.bp alpha=half-sine-fast-decay fade_ms={} home_reveal_hold_ms={} states=normal-gray/hover-white/press-green/release-blue\n",
        active.width,
        active.height,
        BUTTON_WIDTH_MM,
        BUTTON_HEIGHT_MM,
        REVEAL_CAP_MM,
        FADE_DURATION_MS,
        HOME_REVEAL_HOLD_MS,
    );

    loop {
        service_start_button(&mut active);
        Timer::after(Duration::from_millis(SERVICE_PERIOD_MS)).await;
    }
}

fn initialize_start_button() -> Result<ActiveStartButton, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) =
        crate::intel::active_scanout_dimensions().ok_or("scanout-unavailable")?;
    let (width, height, extent_source) =
        crate::intel::physical_extent_pixels(BUTTON_WIDTH_MM, BUTTON_HEIGHT_MM)
            .map(|extent| (extent.0, extent.1, "edid-physical-mm"))
            .unwrap_or((BUTTON_WIDTH_MM, BUTTON_HEIGHT_MM, "logical-fallback"));
    if width == 0 || height == 0 || width > screen_width || height > screen_height {
        return Err("button-extent-invalid");
    }
    let (corner_radius, _) =
        crate::intel::physical_extent_pixels(BUTTON_CORNER_RADIUS_MM, BUTTON_CORNER_RADIUS_MM)
            .unwrap_or((BUTTON_CORNER_RADIUS_MM, BUTTON_CORNER_RADIUS_MM));
    let (glyph_padding, _) =
        crate::intel::physical_extent_pixels(BUTTON_GLYPH_PADDING_MM, BUTTON_GLYPH_PADDING_MM)
            .unwrap_or((BUTTON_GLYPH_PADDING_MM, BUTTON_GLYPH_PADDING_MM));
    let (reveal_cap_x, reveal_cap_y) =
        crate::intel::physical_extent_pixels(REVEAL_CAP_MM, REVEAL_CAP_MM)
            .unwrap_or((REVEAL_CAP_MM, REVEAL_CAP_MM));

    let glyph = crate::intel::gpu_font::render_centered_text_sprite_readback(
        "§",
        crate::intel::gpu_font::GpuFontFace::Default,
        width,
        height,
        glyph_padding,
    )?;
    if glyph.width != width || glyph.height != height {
        return Err("font-sprite-extent-mismatch");
    }
    let glyph_alpha = glyph
        .pixels
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    if glyph_alpha.len() != width as usize * height as usize
        || !glyph_alpha.iter().any(|alpha| *alpha != 0)
    {
        return Err("font-sprite-empty");
    }

    let session = begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = finish_window_session(OWNER, session);
            return Err("frame-create");
        }
    };
    let y = screen_height.saturating_sub(height) / 2;
    let window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Interaction,
        placement: WindowPlacement {
            x: 0,
            y: y as i32,
            width,
            height,
            z: i32::MAX,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction {
            movable: true,
            maximizable: false,
            receives_input: true,
            primary_activation: false,
            hit_testable: true,
            resize_on_maximize: false,
        },
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(OWNER, session);
            let _ = destroy_frame(frame);
            return Err("window-create");
        }
    };

    let now_ms = Instant::now().as_millis();
    let active = ActiveStartButton {
        _session: session,
        frame,
        window,
        screen_width,
        screen_height,
        width,
        height,
        corner_radius: corner_radius.min(width / 2).min(height / 2).max(1),
        reveal_cap_x,
        reveal_cap_y,
        glyph_alpha,
        visual_state: ButtonVisualState::Normal,
        pressed_by: None,
        released_until_ms: 0,
        fade_started_ms: Some(now_ms),
        home_visible_until_ms: 0,
        reveal_sequence: 0,
        opacity: u8::MAX,
        attached_right: false,
    };
    if render_button_sprite(&active, ButtonVisualState::Normal).is_err() {
        let _ = finish_window_session(OWNER, session);
        let _ = destroy_frame(frame);
        return Err("initial-sprite-publish");
    }
    crate::log_info!(target: "ui4/start-button";
        "ui4/start-button: initialized frame={} window={} extent={}x{} extent_source={} font_sprite=fontkernel-readback-once transparent_background=1 rounded_radius_px={} edge_cap_px={}x{} initial_alpha=255\n",
        frame.raw(),
        window.raw(),
        width,
        height,
        extent_source,
        active.corner_radius,
        reveal_cap_x,
        reveal_cap_y,
    );
    Ok(active)
}

fn service_start_button(button: &mut ActiveStartButton) {
    let now_ms = Instant::now().as_millis();
    for event in take_owner_input_events(OWNER) {
        match event {
            Ui4InputEvent::Button(event)
                if event.window == button.window
                    && event.changed_buttons & PRIMARY_BUTTON_MASK != 0 =>
            {
                match event.phase {
                    super::Ui4ButtonPhase::Down => button.pressed_by = Some(event.source),
                    super::Ui4ButtonPhase::Up => {
                        let activated = button.pressed_by.take() == Some(event.source)
                            && local_point_inside(
                                event.local_x,
                                event.local_y,
                                button.width,
                                button.height,
                            );
                        if activated {
                            button.released_until_ms = now_ms.saturating_add(RELEASE_STATE_MS);
                            super::input_broker::request_desktop_shell_launch(
                                event.source,
                                event.x,
                                event.y,
                            );
                        }
                    }
                }
            }
            Ui4InputEvent::Pointer(event)
                if event.window == button.window
                    && event.buttons_down & SECONDARY_BUTTON_MASK != 0 =>
            {
                button.attached_right = event.x >= button.screen_width / 2;
            }
            _ => {}
        }
    }

    let mut placement = match window_placement(OWNER, button.window) {
        Ok(placement) => placement,
        Err(_) => return,
    };
    let visuals = super::software_cursor_visuals();
    let cursor_inside = visuals
        .iter()
        .any(|cursor| point_near_placement(placement, cursor.x, cursor.y, 0, 0));
    let cursor_near = visuals.iter().any(|cursor| {
        point_near_placement(
            placement,
            cursor.x,
            cursor.y,
            button.reveal_cap_x,
            button.reveal_cap_y,
        )
    });

    let reveal_sequence = REVEAL_SEQUENCE.load(Ordering::Acquire);
    if reveal_sequence != button.reveal_sequence {
        button.reveal_sequence = reveal_sequence;
        if button.opacity == 0 {
            button.home_visible_until_ms = now_ms.saturating_add(HOME_REVEAL_HOLD_MS);
            button.fade_started_ms = None;
            crate::log_info!(target: "ui4/start-button";
                "ui4/start-button: home reveal accepted hold_ms={} cursor_present={} action=show-only shell_launch=0\n",
                HOME_REVEAL_HOLD_MS,
                (!visuals.is_empty()) as u8,
            );
        }
    }

    let held_visible = now_ms < button.home_visible_until_ms;
    let opacity = if cursor_near || held_visible {
        button.fade_started_ms = None;
        u8::MAX
    } else {
        let fade_started = *button.fade_started_ms.get_or_insert(now_ms);
        fast_wave_fade_alpha(now_ms.saturating_sub(fade_started))
    };
    let visual_state = if button.pressed_by.is_some() {
        ButtonVisualState::Pressed
    } else if now_ms < button.released_until_ms {
        ButtonVisualState::Released
    } else if cursor_inside {
        ButtonVisualState::Hover
    } else {
        ButtonVisualState::Normal
    };
    if visual_state != button.visual_state && render_button_sprite(button, visual_state).is_ok() {
        button.visual_state = visual_state;
    }

    placement.x = if button.attached_right {
        button.screen_width.saturating_sub(button.width) as i32
    } else {
        0
    };
    placement.y = placement
        .y
        .clamp(0, button.screen_height.saturating_sub(button.height) as i32);
    placement.z = i32::MAX;
    placement.opacity = opacity;
    placement.visible = true;
    if opacity != button.opacity || window_placement(OWNER, button.window).ok() != Some(placement) {
        if set_window_placement(OWNER, button.window, placement).is_ok() {
            button.opacity = opacity;
            super::input_broker::notify_slot4_visual_change();
        }
    }
}

fn render_button_sprite(button: &ActiveStartButton, state: ButtonVisualState) -> Result<(), ()> {
    let lease = acquire_frame_buffer(button.frame).map_err(|_| ())?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = super::cancel_frame_buffer(lease);
            return Err(());
        }
    };
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    pixels.fill(0);
    let pitch = view.pitch as usize;
    let (background, foreground) = state.colors();
    for y in 0..button.height {
        for x in 0..button.width {
            if !rounded_rect_contains(x, y, button.width, button.height, button.corner_radius) {
                continue;
            }
            let glyph = button.glyph_alpha[(y * button.width + x) as usize];
            let inverse = u16::from(u8::MAX - glyph);
            let blend = |foreground: u8, background: u8| -> u8 {
                ((u16::from(foreground) * u16::from(glyph) + u16::from(background) * inverse + 127)
                    / 255) as u8
            };
            let offset = y as usize * pitch + x as usize * 4;
            pixels[offset..offset + 4].copy_from_slice(&[
                blend(foreground[0], background[0]),
                blend(foreground[1], background[1]),
                blend(foreground[2], background[2]),
                u8::MAX,
            ]);
        }
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    publish_frame_buffer(lease).map_err(|_| ())?;
    publish_window_frame(OWNER, button.window, DamageRect::FULL).map_err(|_| ())?;
    super::input_broker::notify_slot4_visual_change();
    Ok(())
}

fn fast_wave_fade_alpha(elapsed_ms: u64) -> u8 {
    if elapsed_ms >= FADE_DURATION_MS {
        return 0;
    }
    let phase = elapsed_ms as f32 / FADE_DURATION_MS as f32;
    let wave = 1.0 - libm::sinf(core::f32::consts::FRAC_PI_2 * phase);
    libm::roundf(wave * 255.0).clamp(0.0, 255.0) as u8
}

fn local_point_inside(x: i32, y: i32, width: u32, height: u32) -> bool {
    x >= 0 && y >= 0 && x < width as i32 && y < height as i32
}

fn point_near_placement(
    placement: WindowPlacement,
    x: u32,
    y: u32,
    cap_x: u32,
    cap_y: u32,
) -> bool {
    let x = i64::from(x);
    let y = i64::from(y);
    let left = i64::from(placement.x).saturating_sub(i64::from(cap_x));
    let top = i64::from(placement.y).saturating_sub(i64::from(cap_y));
    let right = i64::from(placement.x)
        .saturating_add(i64::from(placement.width))
        .saturating_add(i64::from(cap_x));
    let bottom = i64::from(placement.y)
        .saturating_add(i64::from(placement.height))
        .saturating_add(i64::from(cap_y));
    x >= left && x < right && y >= top && y < bottom
}

fn rounded_rect_contains(x: u32, y: u32, width: u32, height: u32, radius: u32) -> bool {
    let radius = radius.min(width / 2).min(height / 2);
    if radius == 0 || x >= radius && x < width - radius || y >= radius && y < height - radius {
        return true;
    }
    let center_x = if x < radius {
        radius
    } else {
        width - radius - 1
    };
    let center_y = if y < radius {
        radius
    } else {
        height - radius - 1
    };
    let dx = i64::from(x) - i64::from(center_x);
    let dy = i64::from(y) - i64::from(center_y);
    dx * dx + dy * dy <= i64::from(radius) * i64::from(radius)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_reaches_full_transparency_at_three_seconds() {
        assert_eq!(fast_wave_fade_alpha(0), 255);
        assert!(fast_wave_fade_alpha(1_000) < 160);
        assert!(fast_wave_fade_alpha(2_000) < 50);
        assert_eq!(fast_wave_fade_alpha(3_000), 0);
        assert_eq!(fast_wave_fade_alpha(30_000), 0);
    }

    #[test]
    fn five_mm_cap_expands_the_reveal_target() {
        let placement = WindowPlacement {
            x: 0,
            y: 100,
            width: 20,
            height: 40,
            z: i32::MAX,
            opacity: 0,
            visible: true,
        };
        assert!(point_near_placement(placement, 24, 120, 5, 5));
        assert!(!point_near_placement(placement, 25, 120, 5, 5));
    }

    #[test]
    fn rounded_tile_keeps_center_and_cuts_outer_corner() {
        assert!(!rounded_rect_contains(0, 0, 20, 40, 4));
        assert!(rounded_rect_contains(10, 20, 20, 40, 4));
        assert!(rounded_rect_contains(4, 0, 20, 40, 4));
    }

    #[test]
    fn drag_stays_in_left_or_right_attachment_band() {
        let placement = WindowPlacement {
            x: 200,
            y: 100,
            width: 20,
            height: 40,
            z: i32::MAX,
            opacity: 255,
            visible: true,
        };
        assert_eq!(constrain_drag_placement(OWNER, placement, 49, 100).x, 0);
        assert_eq!(constrain_drag_placement(OWNER, placement, 50, 100).x, 80);
    }
}
