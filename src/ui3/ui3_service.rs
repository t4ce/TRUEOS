use alloc::{string::String, vec::Vec};

use embassy_time::{Duration as EmbassyDuration, Timer};
use serde_json::Value;

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;
const UI3_WHEEL_EVENT_BATCH_CAP: usize = 64;
const UI3_WHEEL_SCROLL_PX_PER_NOTCH: f32 = 72.0;
const UI3_CLICK_MAX_MOVE_PX: u32 = 6;
const UI3_BACKEND_PREFETCH_BANDS_AFTER_PRESENT: usize = 1;
const UI3_CHOICE_OVERRIDE_CAP: usize = 64;
const UI3_SUMMARY_OVERRIDE_CAP: usize = 64;

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[derive(Debug, Default)]
struct Ui3Scene {
    frame: crate::surfer::Ui3RenderTreeFrame,
    scroll_y: f32,
    content_height: u32,
    viewport_width: u32,
    viewport_height: u32,
    surface: Option<crate::ui3::ui3_surface::Ui3RgbaSurface>,
    painted_bands: Vec<Ui3PaintedBand>,
    choice_overrides: Vec<Ui3ChoiceControlOverride>,
    summary_overrides: Vec<Ui3SummaryOverride>,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3PaintedBand {
    y0: u32,
    y1: u32,
}

#[derive(Clone, Debug)]
struct Ui3ChoiceControlOverride {
    key: String,
    checked: bool,
}

#[derive(Clone, Debug)]
struct Ui3SummaryOverride {
    key: String,
    open: bool,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LiveOverlayState {
    context_menu_open: bool,
    context_menu_x: u32,
    context_menu_y: u32,
    selection_probe_active: bool,
    selection_probe_start_x: u32,
    selection_probe_start_y: u32,
    selection_probe_current_x: u32,
    selection_probe_current_y: u32,
    last_buttons_down: u32,
}

#[derive(Clone, Debug, Default)]
struct Ui3CursorInput {
    wheel_delta: i32,
    overlay_dirty: bool,
    choice_dirty: bool,
    summary_dirty: bool,
    toggle_stamp: Option<Ui3ToggleStamp>,
}

#[derive(Clone, Debug)]
struct Ui3ActivationHit {
    key: String,
    kind: String,
    url: String,
}

#[derive(Clone, Debug)]
struct Ui3ChoiceHit {
    key: String,
    kind: String,
    checked: bool,
    stamp_x: i32,
    stamp_y: i32,
}

#[derive(Clone, Debug)]
struct Ui3SummaryHit {
    key: String,
    open: bool,
    stamp_x: i32,
    stamp_y: i32,
}

#[derive(Clone, Debug)]
struct Ui3ToggleStamp {
    key: String,
    kind: String,
    x: i32,
    y: i32,
    slot: u16,
}

#[derive(Clone, Debug, Default)]
struct Ui3ClickResult {
    activated: bool,
    choice_toggled: bool,
    summary_toggled: bool,
    toggle_stamp: Option<Ui3ToggleStamp>,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!("ui3-service: starting sink=render-tree-backend scroll=retained\n");

    let mut stats = Ui3ServiceStats::default();
    let mut scene = Ui3Scene::default();
    let mut cursor_events = crate::ui3::ui3_hid::Ui3CursorEventDrain::default();
    let mut live_overlay = Ui3LiveOverlayState::default();
    let mut shell_overlay = crate::ui3::ui3_shell_overlay::Ui3ShellOverlayState::default();
    let mut font = crate::ui3::ui3_font::Ui3FontScratch::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
        }

        let (overlay_width, overlay_height) = ui3_overlay_viewport(&scene);
        let shell_input = crate::ui3::ui3_shell_overlay::handle_keyboard(
            &mut shell_overlay,
            overlay_width,
            overlay_height,
        );
        let cursor_input =
            drain_ui3_cursor_input(&mut cursor_events, &mut live_overlay, &mut scene);
        if shell_input.toggled_on || shell_input.toggled_off {
            let invalidated = invalidate_visible_scene_bands(&mut scene);
            crate::log!(
                "ui3-service: shell-toggle active={} invalidated_visible={}\n",
                shell_overlay.active as u8,
                invalidated as u8
            );
        }
        if shell_overlay.active {
            let _ = draw_shell_on_scene(&mut scene, &mut shell_overlay, false, "ui3-shell-scene");
        } else if shell_input.toggled_off {
            let present = redraw_scene_text(&mut scene, &mut font, 0, false);
            crate::log!(
                "ui3-service: shell-hide repaint presented={} submit_ok={} present_ms={} total_ms={}\n",
                present.presented as u8,
                present.submit_ok as u8,
                present.present_ms,
                present.total_ms
            );
        } else if cursor_input.choice_dirty || cursor_input.summary_dirty {
            let stamped = cursor_input
                .toggle_stamp
                .as_ref()
                .is_some_and(|stamp| stamp_ui3_toggle(&scene, stamp));
            if !stamped {
                let invalidated = invalidate_visible_scene_bands(&mut scene);
                let present = redraw_scene_text(&mut scene, &mut font, 0, false);
                crate::log!(
                    "ui3-service: ui-toggle repaint choice={} summary={} invalidated_visible={} presented={} submit_ok={} present_ms={} total_ms={}\n",
                    cursor_input.choice_dirty as u8,
                    cursor_input.summary_dirty as u8,
                    invalidated as u8,
                    present.presented as u8,
                    present.submit_ok as u8,
                    present.present_ms,
                    present.total_ms
                );
            }
            if cursor_input.overlay_dirty {
                let _ = redraw_live_overlay(&scene, &live_overlay, "ui3-live-overlay-toggle");
            }
        } else if cursor_input.overlay_dirty {
            let _ = redraw_live_overlay(&scene, &live_overlay, "ui3-live-overlay-cursor");
        }
        let wheel_delta = cursor_input.wheel_delta;
        if wheel_delta != 0 && !shell_overlay.active && !scene.frame.layout_trace_json.is_empty() {
            let old_scroll_y = scene.scroll_y;
            let new_scroll_y = clamp_scroll_y_for_scene(
                (old_scroll_y - (wheel_delta as f32 * UI3_WHEEL_SCROLL_PX_PER_NOTCH)).max(0.0),
                scene.content_height,
                scene.viewport_height,
            );
            if new_scroll_y == old_scroll_y {
                crate::log!(
                    "ui3-service: scroll noop reason=bounds delta={} scroll_y={} content_height={} viewport={}x{}\n",
                    wheel_delta,
                    scene.scroll_y as u32,
                    scene.content_height,
                    scene.viewport_width,
                    scene.viewport_height
                );
            } else {
                scene.scroll_y = new_scroll_y;
                let present = redraw_scene_text(&mut scene, &mut font, 0, true);
                crate::log!(
                    "ui3-service: scroll retained delta={} scroll_y={} content_height={} viewport={}x{} painted_bands={} presented={} submit_ok={} present_ms={} total_ms={}\n",
                    wheel_delta,
                    scene.scroll_y as u32,
                    scene.content_height,
                    scene.viewport_width,
                    scene.viewport_height,
                    scene.painted_bands.len(),
                    present.presented as u8,
                    present.submit_ok as u8,
                    present.present_ms,
                    present.total_ms
                );
            }
        }

        let mut took_any = false;
        for browser_instance_id in 1..=crate::surfer::MAX_BROWSER_INSTANCE_ID {
            let Some(frame) =
                crate::surfer::take_ui3_render_tree_frame_for_browser(browser_instance_id)
            else {
                continue;
            };
            took_any = true;
            stats.frames_taken = stats.frames_taken.saturating_add(1);
            scene.frame = frame;
            scene.painted_bands.clear();
            consume_render_tree_frame(&mut scene, stats.frames_taken, &mut font);
            if shell_overlay.active {
                let _ = draw_shell_on_scene(
                    &mut scene,
                    &mut shell_overlay,
                    true,
                    "ui3-shell-scene-frame",
                );
            }
        }

        if !took_any {
            let asset_ready_mask = crate::surfer::take_ui3_asset_batch_ready_mask();
            if !scene.frame.layout_trace_json.is_empty()
                && browser_mask_has(asset_ready_mask, scene.frame.browser_instance_id)
            {
                let invalidated = invalidate_ready_asset_bands(&mut scene);
                if invalidated != 0 {
                    let present = redraw_scene_text(&mut scene, &mut font, 0, false);
                    if shell_overlay.active {
                        let _ = draw_shell_on_scene(
                            &mut scene,
                            &mut shell_overlay,
                            true,
                            "ui3-shell-scene-asset",
                        );
                    }
                    crate::log!(
                        "ui3-service: asset batch redraw browser={} seq={} invalidated={} scroll_y={} content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
                        scene.frame.browser_instance_id,
                        scene.frame.seq,
                        invalidated,
                        scene.scroll_y as u32,
                        scene.content_height,
                        scene.viewport_width,
                        scene.viewport_height,
                        present.text_nodes,
                        present.placements,
                        present.gradients,
                        present.assets,
                        present.layout_shift_px,
                        present.embedded_scenes,
                        present.clipped,
                        present.batches,
                        present.clear_ok as u8,
                        present.clear_ms,
                        present.rect_ms,
                        present.asset_ms,
                        present.text_ms,
                        present.show_ms,
                        present.presented as u8,
                        present.submit_ok as u8,
                        present.submit_ms,
                        present.present_ms,
                        present.total_ms,
                        scene.frame.url
                    );
                } else {
                    crate::log!(
                        "ui3-service: asset batch defer browser={} seq={} reason=no-painted-band-intersection scroll_y={} content_height={} viewport={}x{} painted_bands={} url={}\n",
                        scene.frame.browser_instance_id,
                        scene.frame.seq,
                        scene.scroll_y as u32,
                        scene.content_height,
                        scene.viewport_width,
                        scene.viewport_height,
                        scene.painted_bands.len(),
                        scene.frame.url
                    );
                }
            }
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(
    scene: &mut Ui3Scene,
    taken_seq: u32,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
) {
    let present = redraw_scene_text(scene, font, taken_seq, false);
    let frame = &scene.frame;
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} scroll_y={} scroll_redraw=0 content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        frame.render_tree_json.len(),
        frame.layout_trace_json.len(),
        scene.scroll_y as u32,
        scene.content_height,
        scene.viewport_width,
        scene.viewport_height,
        present.text_nodes,
        present.placements,
        present.gradients,
        present.assets,
        present.layout_shift_px,
        present.embedded_scenes,
        present.clipped,
        present.batches,
        present.clear_ok as u8,
        present.clear_ms,
        present.rect_ms,
        present.asset_ms,
        present.text_ms,
        present.show_ms,
        present.presented as u8,
        present.submit_ok as u8,
        present.submit_ms,
        present.present_ms,
        present.total_ms,
        frame.url
    );
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3LayoutInspectResult {
    text_nodes: usize,
    placements: usize,
    assets: usize,
    layout_shift_px: u32,
    batches: usize,
    gradients: usize,
    embedded_scenes: usize,
    clipped: usize,
    submit_ok: bool,
    presented: bool,
    clear_ok: bool,
    clear_ms: u64,
    rect_ms: u64,
    asset_ms: u64,
    text_ms: u64,
    show_ms: u64,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
}

fn redraw_scene_text(
    scene: &mut Ui3Scene,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
    taken_seq: u32,
    is_scroll: bool,
) -> Ui3LayoutInspectResult {
    let total_start = embassy_time_driver::now();
    let browser_instance_id = scene.frame.browser_instance_id;
    let frame_seq = scene.frame.seq;
    let layout_trace_len = scene.frame.layout_trace_json.len();
    let Ok(mut value) = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str())
    else {
        crate::log!(
            "ui3-service: layout json parse failed browser={} seq={} bytes={}\n",
            browser_instance_id,
            frame_seq,
            layout_trace_len
        );
        return Ui3LayoutInspectResult::default();
    };
    apply_choice_overrides_to_layout_value(&mut value, scene.choice_overrides.as_slice());
    apply_summary_overrides_to_layout_value(&mut value, scene.summary_overrides.as_slice());

    let embedded_scenes = value
        .get("trace")
        .and_then(|trace| trace.get("embeddedScenes"))
        .or_else(|| value.get("embeddedScenes"))
        .and_then(Value::as_array)
        .map(|scenes| scenes.len())
        .unwrap_or(0);
    let Some(paint_plan) = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))
    else {
        crate::log!(
            "ui3-service: layout json missing ui3PaintPlan browser={} seq={}\n",
            browser_instance_id,
            frame_seq
        );
        return Ui3LayoutInspectResult::default();
    };

    if let Some((viewport_width, viewport_height)) = crate::intel::active_scanout_dimensions() {
        scene.viewport_width = viewport_width;
        scene.viewport_height = viewport_height;
    }
    scene.content_height = json_f32_field(paint_plan, "contentHeight")
        .map(|height| ceil_u32(height).max(scene.viewport_height))
        .unwrap_or(scene.viewport_height);
    scene.scroll_y =
        clamp_scroll_y_for_scene(scene.scroll_y, scene.content_height, scene.viewport_height);
    let visible_y0 = scene.scroll_y as u32;
    let visible_y1 = visible_y0
        .saturating_add(scene.viewport_height)
        .min(scene.content_height.max(scene.viewport_height));
    let surface_ok =
        ensure_scene_surface(scene, scene.viewport_width, visible_y1.max(scene.viewport_height));
    if !surface_ok {
        crate::log!(
            "ui3-service: scene surface unavailable browser={} seq={} viewport={}x{} content_height={}\n",
            browser_instance_id,
            frame_seq,
            scene.viewport_width,
            scene.viewport_height,
            scene.content_height
        );
    }

    let present_reason = if is_scroll {
        "ui3-text-scroll-backend"
    } else {
        "ui3-text-frame-backend"
    };
    let mut draw = paint_missing_scene_bands(
        scene,
        font,
        paint_plan,
        browser_instance_id,
        visible_y0,
        visible_y1,
        usize::MAX,
        present_reason,
    );
    if draw.layout_shift_px != 0 {
        scene.content_height = scene.content_height.saturating_add(draw.layout_shift_px);
    }
    let scanout_start = embassy_time_driver::now();
    let scanout = bind_scene_surface_scanout(scene, present_reason);
    let scanout_ms = if scanout {
        elapsed_ms_since(scanout_start)
    } else {
        0
    };
    let prefetch_start = embassy_time_driver::now();
    let prefetch = prefetch_scene_bands_after_present(
        scene,
        font,
        paint_plan,
        browser_instance_id,
        visible_y1,
        UI3_BACKEND_PREFETCH_BANDS_AFTER_PRESENT,
        "ui3-text-prefetch-backend",
    );
    if prefetch.submit_ok || prefetch.clear_ok {
        crate::log!(
            "ui3-service: band-prefetch browser={} seq={} from_y={} bands={} text_nodes={} placements={} gradients={} assets={} submit_ok={} ms={}\n",
            browser_instance_id,
            frame_seq,
            visible_y1,
            scene.painted_bands.len(),
            prefetch.text_nodes,
            prefetch.placements,
            prefetch.gradients,
            prefetch.assets,
            prefetch.submit_ok as u8,
            elapsed_ms_since(prefetch_start)
        );
    }
    merge_layout_result(&mut draw, prefetch);
    let total_ms = elapsed_ms_since(total_start);

    if is_scroll {
        crate::log!(
            "ui3-service: scroll taken={} browser={} seq={} scroll_y={} content_height={} viewport={}x{} text_nodes={} placements={} gradients={} assets={} layout_shift={} embedded_scenes={} clipped={} batches={} clear_ok={} clear_ms={} rect_ms={} asset_ms={} text_ms={} show_ms={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
            taken_seq,
            browser_instance_id,
            frame_seq,
            scene.scroll_y as u32,
            scene.content_height,
            scene.viewport_width,
            scene.viewport_height,
            draw.text_nodes,
            draw.placements,
            draw.gradients,
            draw.assets,
            draw.layout_shift_px,
            embedded_scenes,
            draw.clipped,
            draw.batches,
            draw.clear_ok as u8,
            draw.clear_ms,
            draw.rect_ms,
            draw.asset_ms,
            draw.text_ms,
            draw.show_ms,
            draw.presented as u8,
            draw.submit_ok as u8,
            draw.submit_ms,
            draw.present_ms,
            total_ms,
            scene.frame.url
        );
    }

    Ui3LayoutInspectResult {
        text_nodes: draw.text_nodes,
        placements: draw.placements,
        assets: draw.assets,
        layout_shift_px: draw.layout_shift_px,
        batches: draw.batches,
        gradients: draw.gradients,
        embedded_scenes,
        clipped: draw.clipped,
        submit_ok: draw.submit_ok,
        presented: draw.presented || scanout,
        clear_ok: draw.clear_ok,
        clear_ms: draw.clear_ms,
        rect_ms: draw.rect_ms,
        asset_ms: draw.asset_ms,
        text_ms: draw.text_ms,
        show_ms: draw.show_ms,
        submit_ms: draw.submit_ms,
        present_ms: draw.present_ms.saturating_add(scanout_ms),
        total_ms: elapsed_ms_since(total_start),
    }
}

fn paint_missing_scene_bands(
    scene: &mut Ui3Scene,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
    paint_plan: &Value,
    browser_instance_id: u32,
    y0: u32,
    y1: u32,
    max_bands: usize,
    reason: &str,
) -> crate::ui3::ui3_font::Ui3FontDrawResult {
    let mut out = crate::ui3::ui3_font::Ui3FontDrawResult::default();
    if y0 >= y1 || max_bands == 0 || scene.viewport_width == 0 || scene.viewport_height == 0 {
        return out;
    }
    let band_height = scene_band_height(scene);
    let mut band_y0 = align_down_u32(y0, band_height);
    let mut painted = 0usize;
    while band_y0 < y1 && painted < max_bands {
        let band_y1 = band_y0
            .saturating_add(band_height)
            .min(scene.content_height.max(scene.viewport_height));
        if band_y0 >= band_y1 {
            break;
        }
        if painted_range_covers(scene, band_y0, band_y1) {
            band_y0 = band_y1;
            continue;
        }
        if !ensure_scene_surface(scene, scene.viewport_width, band_y1.max(scene.viewport_height)) {
            break;
        }
        let Some(surface) = scene.surface.as_ref() else {
            break;
        };
        let font_scene = crate::ui3::ui3_font::Ui3FontScene {
            browser_instance_id,
            scroll_y: band_y0 as f32,
            viewport_width: scene.viewport_width,
            viewport_height: band_y1.saturating_sub(band_y0),
        };
        let draw = crate::ui3::ui3_font::draw_paint_plan_backend_band(
            paint_plan,
            font_scene,
            font,
            surface,
            band_y0,
            band_y1.saturating_sub(band_y0),
            reason,
        );
        mark_painted_range(scene, band_y0, band_y1);
        merge_font_draw_result(&mut out, draw);
        painted = painted.saturating_add(1);
        crate::log!(
            "ui3-service: band-paint reason={} y={}..{} band_h={} painted_bands={} text_nodes={} placements={} gradients={} assets={} submit_ok={}\n",
            reason,
            band_y0,
            band_y1,
            band_height,
            scene.painted_bands.len(),
            draw.text_nodes,
            draw.placements,
            draw.gradients,
            draw.assets,
            draw.submit_ok as u8
        );
        band_y0 = band_y1;
    }
    out
}

fn prefetch_scene_bands_after_present(
    scene: &mut Ui3Scene,
    font: &mut crate::ui3::ui3_font::Ui3FontScratch,
    paint_plan: &Value,
    browser_instance_id: u32,
    visible_y1: u32,
    max_bands: usize,
    reason: &str,
) -> crate::ui3::ui3_font::Ui3FontDrawResult {
    if max_bands == 0 {
        return crate::ui3::ui3_font::Ui3FontDrawResult::default();
    }
    let band_height = scene_band_height(scene);
    let prefetch_y0 = align_up_u32(visible_y1, band_height);
    if prefetch_y0 >= scene.content_height {
        return crate::ui3::ui3_font::Ui3FontDrawResult::default();
    }
    let prefetch_y1 = prefetch_y0
        .saturating_add(band_height.saturating_mul(max_bands as u32))
        .min(scene.content_height);
    paint_missing_scene_bands(
        scene,
        font,
        paint_plan,
        browser_instance_id,
        prefetch_y0,
        prefetch_y1,
        max_bands,
        reason,
    )
}

fn merge_font_draw_result(
    out: &mut crate::ui3::ui3_font::Ui3FontDrawResult,
    draw: crate::ui3::ui3_font::Ui3FontDrawResult,
) {
    out.text_nodes = out.text_nodes.saturating_add(draw.text_nodes);
    out.placements = out.placements.saturating_add(draw.placements);
    out.assets = out.assets.saturating_add(draw.assets);
    out.layout_shift_px = out.layout_shift_px.max(draw.layout_shift_px);
    out.batches = out.batches.saturating_add(draw.batches);
    out.gradients = out.gradients.saturating_add(draw.gradients);
    out.clipped = out.clipped.saturating_add(draw.clipped);
    out.clear_ok |= draw.clear_ok;
    out.clear_ms = out.clear_ms.saturating_add(draw.clear_ms);
    out.rect_ms = out.rect_ms.saturating_add(draw.rect_ms);
    out.asset_ms = out.asset_ms.saturating_add(draw.asset_ms);
    out.text_ms = out.text_ms.saturating_add(draw.text_ms);
    out.show_ms = out.show_ms.saturating_add(draw.show_ms);
    out.submit_ok |= draw.submit_ok;
    out.presented |= draw.presented;
    out.submit_ms = out.submit_ms.saturating_add(draw.submit_ms);
    out.present_ms = out.present_ms.saturating_add(draw.present_ms);
}

fn scene_band_height(scene: &Ui3Scene) -> u32 {
    (scene.viewport_height / 2).max(1)
}

fn align_down_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        value
    } else {
        value / align * align
    }
}

fn align_up_u32(value: u32, align: u32) -> u32 {
    if align == 0 {
        return value;
    }
    value
        .saturating_add(align.saturating_sub(1))
        .checked_div(align)
        .unwrap_or(0)
        .saturating_mul(align)
}

fn painted_range_covers(scene: &Ui3Scene, y0: u32, y1: u32) -> bool {
    if y0 >= y1 {
        return true;
    }
    let mut cursor = y0;
    for band in &scene.painted_bands {
        if band.y1 <= cursor {
            continue;
        }
        if band.y0 > cursor {
            return false;
        }
        cursor = cursor.max(band.y1);
        if cursor >= y1 {
            return true;
        }
    }
    false
}

fn mark_painted_range(scene: &mut Ui3Scene, y0: u32, y1: u32) {
    if y0 >= y1 {
        return;
    }
    scene.painted_bands.push(Ui3PaintedBand { y0, y1 });
    scene.painted_bands.sort_by_key(|band| band.y0);
    let mut merged: Vec<Ui3PaintedBand> = Vec::new();
    for band in scene.painted_bands.iter().copied() {
        if let Some(last) = merged.last_mut() {
            if band.y0 <= last.y1 {
                last.y1 = last.y1.max(band.y1);
                continue;
            }
        }
        merged.push(band);
    }
    scene.painted_bands = merged;
}

fn invalidate_ready_asset_bands(scene: &mut Ui3Scene) -> usize {
    if scene.painted_bands.is_empty() || scene.frame.layout_trace_json.is_empty() {
        return 0;
    }
    let Ok(value) = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str()) else {
        return 0;
    };
    let Some(paint_plan) = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))
    else {
        return 0;
    };
    let Some(boxes) = paint_plan.get("paintedBoxes").and_then(Value::as_array) else {
        return 0;
    };
    let band_height = scene_band_height(scene);
    let mut invalidated = 0usize;
    for item in boxes {
        if item.get("role").and_then(Value::as_str) != Some("image") {
            continue;
        }
        let Some(key) = item.get("key").and_then(Value::as_str) else {
            continue;
        };
        if crate::surfer::asset_shack::ready_asset_for_tag(scene.frame.browser_instance_id, key)
            .is_none()
        {
            continue;
        }
        let y = json_f32_field(item, "y").unwrap_or(0.0);
        let height = json_f32_field(item, "height")
            .map(ceil_u32)
            .unwrap_or(0)
            .max(1);
        let y0 = floor_i32(y).max(0) as u32;
        let y1 = y0
            .saturating_add(height)
            .min(scene.content_height.max(scene.viewport_height));
        if y0 >= y1 || !painted_range_intersects(scene, y0, y1) {
            continue;
        }
        let band_y0 = align_down_u32(y0, band_height);
        let band_y1 =
            align_up_u32(y1, band_height).min(scene.content_height.max(scene.viewport_height));
        if remove_painted_range(scene, band_y0, band_y1) {
            invalidated = invalidated.saturating_add(1);
            crate::log!(
                "ui3-service: asset-band-invalidate key={} y={}..{} band={}..{} painted_bands={}\n",
                key,
                y0,
                y1,
                band_y0,
                band_y1,
                scene.painted_bands.len()
            );
        }
    }
    invalidated
}

fn painted_range_intersects(scene: &Ui3Scene, y0: u32, y1: u32) -> bool {
    y0 < y1
        && scene
            .painted_bands
            .iter()
            .any(|band| band.y0 < y1 && y0 < band.y1)
}

fn remove_painted_range(scene: &mut Ui3Scene, y0: u32, y1: u32) -> bool {
    if y0 >= y1 || scene.painted_bands.is_empty() {
        return false;
    }
    let mut changed = false;
    let mut kept: Vec<Ui3PaintedBand> = Vec::new();
    for band in scene.painted_bands.iter().copied() {
        if band.y1 <= y0 || band.y0 >= y1 {
            kept.push(band);
            continue;
        }
        changed = true;
        if band.y0 < y0 {
            kept.push(Ui3PaintedBand {
                y0: band.y0,
                y1: y0,
            });
        }
        if band.y1 > y1 {
            kept.push(Ui3PaintedBand {
                y0: y1,
                y1: band.y1,
            });
        }
    }
    if changed {
        scene.painted_bands = kept;
    }
    changed
}

fn merge_layout_result(
    draw: &mut crate::ui3::ui3_font::Ui3FontDrawResult,
    other: crate::ui3::ui3_font::Ui3FontDrawResult,
) {
    merge_font_draw_result(draw, other);
}

fn invalidate_visible_scene_bands(scene: &mut Ui3Scene) -> bool {
    if scene.viewport_width == 0 || scene.viewport_height == 0 {
        return false;
    }
    let y0 = scene.scroll_y as u32;
    let y1 = y0
        .saturating_add(scene.viewport_height)
        .min(scene.content_height.max(scene.viewport_height));
    remove_painted_range(scene, y0, y1)
}

fn draw_shell_on_scene(
    scene: &mut Ui3Scene,
    shell: &mut crate::ui3::ui3_shell_overlay::Ui3ShellOverlayState,
    force: bool,
    reason: &str,
) -> bool {
    if !ensure_shell_scene_surface(scene, reason) {
        return false;
    }
    let Some(surface) = scene.surface.as_ref() else {
        return false;
    };
    let drew = crate::ui3::ui3_shell_overlay::draw_scene_if_dirty(
        shell,
        surface,
        scene.viewport_width,
        scene.viewport_height,
        scene.scroll_y as u32,
        force,
        reason,
    );
    if !drew {
        return false;
    }
    bind_scene_surface_scanout(scene, reason)
}

fn ensure_shell_scene_surface(scene: &mut Ui3Scene, reason: &str) -> bool {
    if (scene.viewport_width == 0 || scene.viewport_height == 0)
        && let Some((viewport_width, viewport_height)) = crate::intel::active_scanout_dimensions()
    {
        scene.viewport_width = viewport_width;
        scene.viewport_height = viewport_height;
    }
    if scene.viewport_width == 0 || scene.viewport_height == 0 {
        crate::log!(
            "ui3-service: shell scene unavailable reason={} cause=no-viewport\n",
            reason
        );
        return false;
    }
    scene.content_height = scene.content_height.max(scene.viewport_height);
    let visible_y1 = (scene.scroll_y as u32)
        .saturating_add(scene.viewport_height)
        .min(scene.content_height.max(scene.viewport_height));
    ensure_scene_surface(scene, scene.viewport_width, visible_y1.max(scene.viewport_height))
}

fn bind_scene_surface_scanout(scene: &Ui3Scene, reason: &str) -> bool {
    let Some(surface) = scene.surface.as_ref() else {
        crate::log!(
            "ui3-service: scanout-bind reason={} ok=0 cause=no-surface scroll_y={} viewport={}x{} content_height={}\n",
            reason,
            scene.scroll_y as u32,
            scene.viewport_width,
            scene.viewport_height,
            scene.content_height
        );
        return false;
    };
    let ok = surface.bind_primary_scanout(
        scene.scroll_y as u32,
        scene.viewport_width,
        scene.viewport_height,
        reason,
    );
    crate::log!(
        "ui3-service: scanout-bind reason={} ok={} scroll_y={} viewport={}x{} content_height={} surface={}x{} pitch={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X}\n",
        reason,
        ok as u8,
        scene.scroll_y as u32,
        scene.viewport_width,
        scene.viewport_height,
        scene.content_height,
        surface.width,
        surface.height,
        surface.pitch_bytes,
        surface.gpu,
        surface.phys,
        surface.bytes
    );
    ok
}

fn stamp_ui3_toggle(scene: &Ui3Scene, stamp: &Ui3ToggleStamp) -> bool {
    let total_start = embassy_time_driver::now();
    let Some(surface) = scene.surface.as_ref() else {
        return false;
    };
    let draw = crate::ui3::ui3_font::stamp_sprite64_backend(
        surface,
        stamp.x,
        stamp.y,
        stamp.slot,
        "ui3-toggle-stamp-backend",
    );
    if !draw.submit_ok {
        crate::log!(
            "ui3-service: toggle-stamp failed browser={} key={} kind={} x={} y={} slot={} clipped={} clear_ms={} text_ms={}\n",
            scene.frame.browser_instance_id,
            stamp.key,
            stamp.kind,
            stamp.x,
            stamp.y,
            stamp.slot,
            draw.clipped,
            draw.clear_ms,
            draw.text_ms
        );
        return false;
    }
    let scanout_start = embassy_time_driver::now();
    let scanout = bind_scene_surface_scanout(scene, "ui3-toggle-stamp-scanout");
    crate::log!(
        "ui3-service: toggle-stamp browser={} key={} kind={} x={} y={} slot={} submitted={} batches={} scanout={} clear_ms={} text_ms={} present_ms={} total_ms={}\n",
        scene.frame.browser_instance_id,
        stamp.key,
        stamp.kind,
        stamp.x,
        stamp.y,
        stamp.slot,
        draw.submit_ok as u8,
        draw.batches,
        scanout as u8,
        draw.clear_ms,
        draw.text_ms,
        elapsed_ms_since(scanout_start),
        elapsed_ms_since(total_start)
    );
    scanout
}

fn ensure_scene_surface(scene: &mut Ui3Scene, width: u32, min_height: u32) -> bool {
    if width == 0 || min_height == 0 {
        return false;
    }

    if let Some(surface) = scene.surface.as_mut() {
        if surface.width == width {
            let old_height = surface.height;
            let ok = surface.ensure_height(min_height);
            if ok && surface.height != old_height {
                crate::log!(
                    "ui3-service: scene-surface grow width={} old_height={} new_height={} pitch={} gpu=0x{:X} bytes=0x{:X}\n",
                    width,
                    old_height,
                    surface.height,
                    surface.pitch_bytes,
                    surface.gpu,
                    surface.bytes
                );
            } else if !ok {
                crate::log!(
                    "ui3-service: scene-surface grow failed width={} old_height={} requested_height={} gpu=0x{:X}\n",
                    width,
                    old_height,
                    min_height,
                    surface.gpu
                );
            }
            return ok;
        }
    }

    scene.surface = crate::ui3::ui3_surface::Ui3RgbaSurface::alloc(
        width,
        min_height,
        crate::intel::GPU_VA_DISPLAY_UI3_SCENE_BASE,
    );
    if let Some(surface) = scene.surface.as_ref() {
        crate::log!(
            "ui3-service: scene-surface alloc width={} height={} pitch={} gpu=0x{:X} phys=0x{:X} bytes=0x{:X}\n",
            surface.width,
            surface.height,
            surface.pitch_bytes,
            surface.gpu,
            surface.phys,
            surface.bytes
        );
    } else {
        crate::log!(
            "ui3-service: scene-surface alloc failed width={} height={} gpu=0x{:X}\n",
            width,
            min_height,
            crate::intel::GPU_VA_DISPLAY_UI3_SCENE_BASE
        );
    }
    scene.surface.is_some()
}

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}

fn json_string_field(node: &Value, key: &str) -> Option<String> {
    node.get(key).and_then(Value::as_str).map(String::from)
}

fn json_bool_field(node: &Value, key: &str) -> Option<bool> {
    node.get(key).and_then(Value::as_bool)
}

fn paint_plan_mut(value: &mut Value) -> Option<&mut Value> {
    if value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .is_some()
    {
        return value
            .get_mut("trace")
            .and_then(|trace| trace.get_mut("ui3PaintPlan"));
    }
    value.get_mut("ui3PaintPlan")
}

fn apply_choice_overrides_to_layout_value(
    value: &mut Value,
    overrides: &[Ui3ChoiceControlOverride],
) {
    if overrides.is_empty() {
        return;
    }
    let Some(paint_plan) = paint_plan_mut(value) else {
        return;
    };
    let Some(controls) = paint_plan
        .get_mut("choiceControls")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for control in controls {
        let Some(key) = control.get("key").and_then(Value::as_str) else {
            continue;
        };
        let Some(override_state) = overrides.iter().find(|state| state.key == key) else {
            continue;
        };
        control["checked"] = Value::Bool(override_state.checked);
        control["indeterminate"] = Value::Bool(false);
    }
}

fn apply_summary_overrides_to_layout_value(value: &mut Value, overrides: &[Ui3SummaryOverride]) {
    if overrides.is_empty() {
        return;
    }
    let Some(paint_plan) = paint_plan_mut(value) else {
        return;
    };
    let Some(icons) = paint_plan
        .get_mut("summaryIcons")
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for icon in icons {
        let Some(key) = icon.get("key").and_then(Value::as_str) else {
            continue;
        };
        let Some(override_state) = overrides.iter().find(|state| state.key == key) else {
            continue;
        };
        icon["open"] = Value::Bool(override_state.open);
    }
}

fn clamp_scroll_y_for_scene(scroll_y: f32, content_height: u32, viewport_height: u32) -> f32 {
    if scroll_y <= 0.0 || content_height <= viewport_height {
        return 0.0;
    }
    scroll_y.min(content_height.saturating_sub(viewport_height) as f32)
}

fn browser_mask_has(mask: u64, browser_instance_id: u32) -> bool {
    if browser_instance_id == 0 || browser_instance_id > 64 {
        return false;
    }
    (mask & (1u64 << browser_instance_id.saturating_sub(1))) != 0
}

fn ceil_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    libm::ceilf(value).min(u32::MAX as f32) as u32
}

fn floor_i32(value: f32) -> i32 {
    if !value.is_finite() {
        return 0;
    }
    libm::floorf(value)
        .max(i32::MIN as f32)
        .min(i32::MAX as f32) as i32
}

fn elapsed_ms_since(start: u64) -> u64 {
    let now = embassy_time_driver::now();
    let ticks = now.saturating_sub(start);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        ticks.saturating_mul(1000) / hz
    }
}

fn drain_ui3_cursor_input(
    state: &mut crate::ui3::ui3_hid::Ui3CursorEventDrain,
    live_overlay: &mut Ui3LiveOverlayState,
    scene: &mut Ui3Scene,
) -> Ui3CursorInput {
    let mut out = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI3_WHEEL_EVENT_BATCH_CAP];
    let read = crate::ui3::ui3_hid::drain_cursor_events(state, out.as_mut_slice());
    let mut input = Ui3CursorInput::default();
    let (viewport_width, viewport_height) = ui3_overlay_viewport(scene);
    for event in out.iter().take(read.wrote) {
        input.wheel_delta = input
            .wheel_delta
            .saturating_add(crate::ui3::ui3_hid::event_wheel_delta(*event));
        if (event.flags & crate::ui3::ui3_hid::UI3_CURSOR_EVENT_FLAG_MOTION) != 0 {
            let (x, y) =
                crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
            if live_overlay.selection_probe_active
                && (event.buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0
            {
                live_overlay.selection_probe_current_x = x;
                live_overlay.selection_probe_current_y = y;
            }
            input.overlay_dirty = true;
        }
        if crate::ui3::ui3_hid::event_has_button_change(*event) {
            let was_right = (live_overlay.last_buttons_down
                & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_RIGHT)
                != 0;
            let was_left =
                (live_overlay.last_buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0;
            let is_right = crate::ui3::ui3_hid::event_has_right_button(*event);
            let is_left = (event.buttons_down & crate::ui3::ui3_hid::UI3_CURSOR_BUTTON_LEFT) != 0;
            if is_right && !was_right {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                live_overlay.context_menu_open = true;
                live_overlay.selection_probe_active = false;
                live_overlay.context_menu_x = x;
                live_overlay.context_menu_y = y;
                input.overlay_dirty = true;
            } else if live_overlay.context_menu_open && is_left {
                live_overlay.context_menu_open = false;
                input.overlay_dirty = true;
            } else if is_left && !was_left {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                live_overlay.selection_probe_active = true;
                live_overlay.selection_probe_start_x = x;
                live_overlay.selection_probe_start_y = y;
                live_overlay.selection_probe_current_x = x;
                live_overlay.selection_probe_current_y = y;
                input.overlay_dirty = true;
            } else if !is_left && was_left && live_overlay.selection_probe_active {
                let (x, y) =
                    crate::ui3::ui3_hid::event_position_px(*event, viewport_width, viewport_height);
                crate::ui3::ui3_orbits::maybe_proc_weather_from_drag(
                    live_overlay.selection_probe_start_x,
                    live_overlay.selection_probe_start_y,
                    x,
                    y,
                );
                let click = activate_ui3_click_if_any(scene, live_overlay, x, y);
                input.choice_dirty |= click.choice_toggled;
                input.summary_dirty |= click.summary_toggled;
                input.overlay_dirty |= click.activated;
                if click.toggle_stamp.is_some() {
                    input.toggle_stamp = click.toggle_stamp;
                }
                live_overlay.selection_probe_active = false;
                input.overlay_dirty = true;
            }
            live_overlay.last_buttons_down = event.buttons_down;
        }
    }
    if input.wheel_delta != 0 {
        crate::log!(
            "ui3-service: wheel events={} dropped={} delta={} read_seq={}\n",
            read.wrote,
            read.dropped,
            input.wheel_delta,
            read.next_seq
        );
    }
    input
}

fn activate_ui3_click_if_any(
    scene: &mut Ui3Scene,
    live_overlay: &Ui3LiveOverlayState,
    release_x: u32,
    release_y: u32,
) -> Ui3ClickResult {
    if !ui3_click_within_threshold(
        live_overlay.selection_probe_start_x,
        live_overlay.selection_probe_start_y,
        release_x,
        release_y,
    ) {
        return Ui3ClickResult::default();
    }

    if let (Some(press_choice), Some(release_choice)) = (
        ui3_choice_hit_at(
            scene,
            live_overlay.selection_probe_start_x,
            live_overlay.selection_probe_start_y,
        ),
        ui3_choice_hit_at(scene, release_x, release_y),
    ) {
        if press_choice.key == release_choice.key && press_choice.kind == release_choice.kind {
            let checked = toggle_ui3_choice_override(scene, &release_choice);
            let stamp = ui3_choice_toggle_stamp(&release_choice, checked, false);
            crate::log!(
                "ui3-service: activate kind=choice-toggle browser={} key={} type={} checked={}\n",
                scene.frame.browser_instance_id,
                release_choice.key,
                release_choice.kind,
                checked as u8
            );
            return Ui3ClickResult {
                activated: true,
                choice_toggled: true,
                summary_toggled: false,
                toggle_stamp: stamp,
            };
        }
    }

    if let (Some(press_summary), Some(release_summary)) = (
        ui3_summary_hit_at(
            scene,
            live_overlay.selection_probe_start_x,
            live_overlay.selection_probe_start_y,
        ),
        ui3_summary_hit_at(scene, release_x, release_y),
    ) {
        if press_summary.key == release_summary.key {
            let open = toggle_ui3_summary_override(scene, &release_summary);
            let stamp = ui3_summary_toggle_stamp(&release_summary, open);
            crate::log!(
                "ui3-service: activate kind=summary-toggle browser={} key={} open={}\n",
                scene.frame.browser_instance_id,
                release_summary.key,
                open as u8
            );
            return Ui3ClickResult {
                activated: true,
                choice_toggled: false,
                summary_toggled: true,
                toggle_stamp: stamp,
            };
        }
    }

    let Some(press_hit) = ui3_activation_hit_at(
        scene,
        live_overlay.selection_probe_start_x,
        live_overlay.selection_probe_start_y,
    ) else {
        return Ui3ClickResult::default();
    };
    let Some(release_hit) = ui3_activation_hit_at(scene, release_x, release_y) else {
        return Ui3ClickResult::default();
    };
    if press_hit.key != release_hit.key || press_hit.kind != release_hit.kind {
        return Ui3ClickResult::default();
    }

    if release_hit.kind == "navigate" && !release_hit.url.is_empty() {
        let queued = crate::surfer::queue_browser_navigation(
            scene.frame.browser_instance_id,
            release_hit.url.as_str(),
        );
        crate::log!(
            "ui3-service: activate kind=navigate queued={} browser={} key={} url={}\n",
            if queued { 1 } else { 0 },
            scene.frame.browser_instance_id,
            release_hit.key,
            release_hit.url
        );
        return Ui3ClickResult {
            activated: queued,
            choice_toggled: false,
            summary_toggled: false,
            toggle_stamp: None,
        };
    }

    Ui3ClickResult::default()
}

fn ui3_click_within_threshold(start_x: u32, start_y: u32, end_x: u32, end_y: u32) -> bool {
    start_x.abs_diff(end_x) <= UI3_CLICK_MAX_MOVE_PX
        && start_y.abs_diff(end_y) <= UI3_CLICK_MAX_MOVE_PX
}

fn ui3_activation_hit_at(
    scene: &Ui3Scene,
    screen_x: u32,
    screen_y: u32,
) -> Option<Ui3ActivationHit> {
    if scene.frame.layout_trace_json.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str()).ok()?;
    let paint_plan = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))?;
    let hit_boxes = paint_plan.get("hitBoxes").and_then(Value::as_array)?;
    let content_x = screen_x as f32;
    let content_y = screen_y as f32 + scene.scroll_y;
    let mut best: Option<Ui3ActivationHit> = None;
    for hit in hit_boxes {
        let x = json_f32_field(hit, "x").unwrap_or(0.0);
        let y = json_f32_field(hit, "y").unwrap_or(0.0);
        let width = json_f32_field(hit, "width").unwrap_or(0.0);
        let height = json_f32_field(hit, "height").unwrap_or(0.0);
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        if content_x < x || content_y < y || content_x >= x + width || content_y >= y + height {
            continue;
        }
        let Some(activation) = hit.get("activation").and_then(Value::as_object) else {
            continue;
        };
        let kind = activation
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if kind.is_empty() {
            continue;
        }
        let url = activation
            .get("resolvedHref")
            .or_else(|| activation.get("href"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        best = Some(Ui3ActivationHit {
            key: json_string_field(hit, "key").unwrap_or_default(),
            kind: String::from(kind),
            url: String::from(url),
        });
    }
    best
}

fn ui3_choice_hit_at(scene: &Ui3Scene, screen_x: u32, screen_y: u32) -> Option<Ui3ChoiceHit> {
    if scene.frame.layout_trace_json.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str()).ok()?;
    let paint_plan = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))?;
    let controls = paint_plan.get("choiceControls").and_then(Value::as_array)?;
    let content_x = screen_x as f32;
    let content_y = screen_y as f32 + scene.scroll_y;
    let mut best: Option<Ui3ChoiceHit> = None;
    for control in controls {
        let kind = control
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if kind != "checkbox" && kind != "radio" {
            continue;
        }
        let x = json_f32_field(control, "x").unwrap_or(0.0);
        let y = json_f32_field(control, "y").unwrap_or(0.0);
        let width = json_f32_field(control, "width").unwrap_or(0.0);
        let height = json_f32_field(control, "height").unwrap_or(0.0);
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        if content_x < x || content_y < y || content_x >= x + width || content_y >= y + height {
            continue;
        }
        let key = json_string_field(control, "key").unwrap_or_default();
        let checked = ui3_choice_override_checked(scene, key.as_str())
            .unwrap_or_else(|| json_bool_field(control, "checked").unwrap_or(false));
        let stamp_x = floor_i32(
            x + (width - crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as f32) * 0.5,
        );
        let stamp_y = floor_i32(
            y + (height - crate::intel::gpgpu::SPRITE64_WORKLIST_CELL_PIXELS as f32) * 0.5,
        );
        best = Some(Ui3ChoiceHit {
            key,
            kind: String::from(kind),
            checked,
            stamp_x,
            stamp_y,
        });
    }
    best
}

fn ui3_summary_hit_at(scene: &Ui3Scene, screen_x: u32, screen_y: u32) -> Option<Ui3SummaryHit> {
    if scene.frame.layout_trace_json.is_empty() {
        return None;
    }
    let value = serde_json::from_str::<Value>(scene.frame.layout_trace_json.as_str()).ok()?;
    let paint_plan = value
        .get("trace")
        .and_then(|trace| trace.get("ui3PaintPlan"))
        .or_else(|| value.get("ui3PaintPlan"))?;
    let hit_boxes = paint_plan.get("hitBoxes").and_then(Value::as_array)?;
    let content_x = screen_x as f32;
    let content_y = screen_y as f32 + scene.scroll_y;
    let mut best: Option<Ui3SummaryHit> = None;
    for hit in hit_boxes {
        if hit.get("tagName").and_then(Value::as_str) != Some("summary") {
            continue;
        }
        let x = json_f32_field(hit, "x").unwrap_or(0.0);
        let y = json_f32_field(hit, "y").unwrap_or(0.0);
        let width = json_f32_field(hit, "width").unwrap_or(0.0);
        let height = json_f32_field(hit, "height").unwrap_or(0.0);
        if width <= 0.0 || height <= 0.0 {
            continue;
        }
        if content_x < x || content_y < y || content_x >= x + width || content_y >= y + height {
            continue;
        }
        let key = json_string_field(hit, "key").unwrap_or_default();
        let open = ui3_summary_override_open(scene, key.as_str())
            .unwrap_or_else(|| ui3_summary_icon_open(paint_plan, key.as_str()).unwrap_or(false));
        let Some((stamp_x, stamp_y, _slot)) =
            crate::ui3::ui3_font::summary_icon_stamp_for_ui3(x, y, height, open)
        else {
            continue;
        };
        best = Some(Ui3SummaryHit {
            key,
            open,
            stamp_x,
            stamp_y,
        });
    }
    best
}

fn ui3_summary_icon_open(paint_plan: &Value, key: &str) -> Option<bool> {
    let icons = paint_plan.get("summaryIcons").and_then(Value::as_array)?;
    icons
        .iter()
        .find(|icon| icon.get("key").and_then(Value::as_str) == Some(key))
        .and_then(|icon| json_bool_field(icon, "open"))
}

fn ui3_choice_override_checked(scene: &Ui3Scene, key: &str) -> Option<bool> {
    scene
        .choice_overrides
        .iter()
        .find(|state| state.key == key)
        .map(|state| state.checked)
}

fn ui3_summary_override_open(scene: &Ui3Scene, key: &str) -> Option<bool> {
    scene
        .summary_overrides
        .iter()
        .find(|state| state.key == key)
        .map(|state| state.open)
}

fn toggle_ui3_choice_override(scene: &mut Ui3Scene, hit: &Ui3ChoiceHit) -> bool {
    let next_checked = !ui3_choice_override_checked(scene, hit.key.as_str()).unwrap_or(hit.checked);
    if let Some(state) = scene
        .choice_overrides
        .iter_mut()
        .find(|state| state.key == hit.key)
    {
        state.checked = next_checked;
        return next_checked;
    }
    if scene.choice_overrides.len() >= UI3_CHOICE_OVERRIDE_CAP {
        scene.choice_overrides.remove(0);
    }
    scene.choice_overrides.push(Ui3ChoiceControlOverride {
        key: hit.key.clone(),
        checked: next_checked,
    });
    next_checked
}

fn ui3_choice_toggle_stamp(
    hit: &Ui3ChoiceHit,
    checked: bool,
    indeterminate: bool,
) -> Option<Ui3ToggleStamp> {
    let slot = crate::ui3::ui3_font::choice_control_slot_for_ui3(
        hit.kind.as_str(),
        checked,
        indeterminate,
    )?;
    Some(Ui3ToggleStamp {
        key: hit.key.clone(),
        kind: hit.kind.clone(),
        x: hit.stamp_x,
        y: hit.stamp_y,
        slot,
    })
}

fn ui3_summary_toggle_stamp(hit: &Ui3SummaryHit, open: bool) -> Option<Ui3ToggleStamp> {
    let slot = crate::ui3::ui3_font::summary_icon_slot_for_ui3(open)?;
    Some(Ui3ToggleStamp {
        key: hit.key.clone(),
        kind: String::from("summary"),
        x: hit.stamp_x,
        y: hit.stamp_y,
        slot,
    })
}

fn toggle_ui3_summary_override(scene: &mut Ui3Scene, hit: &Ui3SummaryHit) -> bool {
    let next_open = !ui3_summary_override_open(scene, hit.key.as_str()).unwrap_or(hit.open);
    if let Some(state) = scene
        .summary_overrides
        .iter_mut()
        .find(|state| state.key == hit.key)
    {
        state.open = next_open;
        return next_open;
    }
    if scene.summary_overrides.len() >= UI3_SUMMARY_OVERRIDE_CAP {
        scene.summary_overrides.remove(0);
    }
    scene.summary_overrides.push(Ui3SummaryOverride {
        key: hit.key.clone(),
        open: next_open,
    });
    next_open
}

fn redraw_live_overlay(scene: &Ui3Scene, state: &Ui3LiveOverlayState, reason: &str) -> bool {
    let (viewport_width, viewport_height) = ui3_overlay_viewport(scene);
    if viewport_width == 0 || viewport_height == 0 {
        return false;
    }
    let mut rects: Vec<crate::intel::LiveOverlayRect> = Vec::new();
    if state.context_menu_open {
        crate::ui3::ui3_hid::push_context_menu_rects(
            &mut rects,
            state.context_menu_x,
            state.context_menu_y,
            viewport_width,
            viewport_height,
        );
    }
    if state.selection_probe_active {
        crate::ui3::ui3_hid::push_drag_selection_probe_rects(
            &mut rects,
            state.selection_probe_start_x,
            state.selection_probe_start_y,
            state.selection_probe_current_x,
            state.selection_probe_current_y,
        );
    }
    if !crate::ui3::ui3_orbits::orbit_visuals_active() {
        crate::ui3::ui3_hid::push_software_cursor_rects(
            &mut rects,
            viewport_width,
            viewport_height,
        );
    }
    let preserve = crate::ui3::ui3_canvas::live_overlay_preserve_rect(rects.as_slice());
    crate::ui3::ui3_orbits::submit_live_overlay_rects(rects.as_slice(), preserve, reason)
}

fn ui3_overlay_viewport(scene: &Ui3Scene) -> (u32, u32) {
    if scene.viewport_width != 0 && scene.viewport_height != 0 {
        return (scene.viewport_width, scene.viewport_height);
    }
    crate::intel::active_scanout_dimensions().unwrap_or((0, 0))
}
