use embassy_time::{Duration as EmbassyDuration, Timer};
use serde_json::Value;

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;
const UI3_WHEEL_EVENT_BATCH_CAP: usize = 64;
const UI3_WHEEL_SCROLL_PX_PER_NOTCH: f32 = 72.0;

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[derive(Clone, Debug, Default)]
struct Ui3Scene {
    frame: crate::surfer::Ui3RenderTreeFrame,
    scroll_y: f32,
    content_height: u32,
    viewport_width: u32,
    viewport_height: u32,
}

#[derive(Copy, Clone, Debug, Default)]
struct Ui3WheelDrain {
    read_seq: u64,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!("ui3-service: starting sink=render-tree-text-primary scroll=redraw\n");

    let mut stats = Ui3ServiceStats::default();
    let mut scene = Ui3Scene::default();
    let mut wheel = Ui3WheelDrain::default();
    let mut font = crate::ui3::font::Ui3FontScratch::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
        }

        let wheel_delta = drain_ui3_wheel_delta(&mut wheel);
        if wheel_delta != 0 && !scene.frame.layout_trace_json.is_empty() {
            scene.scroll_y =
                (scene.scroll_y - (wheel_delta as f32 * UI3_WHEEL_SCROLL_PX_PER_NOTCH)).max(0.0);
            scene.scroll_y = clamp_scroll_y_for_scene(
                scene.scroll_y,
                scene.content_height,
                scene.viewport_height,
            );
            redraw_scene_text(&mut scene, &mut font, 0, true);
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
            consume_render_tree_frame(&mut scene, stats.frames_taken, &mut font);
        }

        if !took_any {
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(
    scene: &mut Ui3Scene,
    taken_seq: u32,
    font: &mut crate::ui3::font::Ui3FontScratch,
) {
    let present = redraw_scene_text(scene, font, taken_seq, false);
    let frame = &scene.frame;
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} scroll_y={} scroll_redraw=0 content_height={} viewport={}x{} text_nodes={} placements={} clipped={} batches=1 clear_ok=0 clear_ms=0 presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
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
        present.clipped,
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
    clipped: usize,
    submit_ok: bool,
    presented: bool,
    submit_ms: u64,
    present_ms: u64,
    total_ms: u64,
}

fn redraw_scene_text(
    scene: &mut Ui3Scene,
    font: &mut crate::ui3::font::Ui3FontScratch,
    taken_seq: u32,
    is_scroll: bool,
) -> Ui3LayoutInspectResult {
    let total_start = embassy_time_driver::now();
    let frame = &scene.frame;
    let Ok(value) = serde_json::from_str::<Value>(frame.layout_trace_json.as_str()) else {
        crate::log!(
            "ui3-service: layout json parse failed browser={} seq={} bytes={}\n",
            frame.browser_instance_id,
            frame.seq,
            frame.layout_trace_json.len()
        );
        return Ui3LayoutInspectResult::default();
    };

    let Some(layout) = value
        .get("trace")
        .and_then(|trace| trace.get("layout"))
        .or_else(|| value.get("layout"))
    else {
        crate::log!(
            "ui3-service: layout json missing layout browser={} seq={}\n",
            frame.browser_instance_id,
            frame.seq
        );
        return Ui3LayoutInspectResult::default();
    };

    if let Some((viewport_width, viewport_height)) = crate::intel::active_scanout_dimensions() {
        scene.viewport_width = viewport_width;
        scene.viewport_height = viewport_height;
    }
    scene.content_height = json_f32_field(layout, "height")
        .map(|height| ceil_u32(height).max(scene.viewport_height))
        .unwrap_or(scene.viewport_height);
    scene.scroll_y =
        clamp_scroll_y_for_scene(scene.scroll_y, scene.content_height, scene.viewport_height);

    let font_scene = crate::ui3::font::Ui3FontScene {
        scroll_y: scene.scroll_y,
        viewport_width: scene.viewport_width,
        viewport_height: scene.viewport_height,
    };
    let draw = crate::ui3::font::draw_layout_primary(
        layout,
        font_scene,
        font,
        if is_scroll {
            "ui3-text-scroll-primary"
        } else {
            "ui3-text-frame-primary"
        },
    );
    let total_ms = elapsed_ms_since(total_start);

    if is_scroll {
        crate::log!(
            "ui3-service: scroll taken={} browser={} seq={} scroll_y={} content_height={} viewport={}x{} text_nodes={} placements={} clipped={} presented={} submit_ok={} submit_ms={} present_ms={} total_ms={} url={}\n",
            taken_seq,
            frame.browser_instance_id,
            frame.seq,
            scene.scroll_y as u32,
            scene.content_height,
            scene.viewport_width,
            scene.viewport_height,
            draw.text_nodes,
            draw.placements,
            draw.clipped,
            draw.presented as u8,
            draw.submit_ok as u8,
            draw.submit_ms,
            draw.present_ms,
            total_ms,
            frame.url
        );
    }

    Ui3LayoutInspectResult {
        text_nodes: draw.text_nodes,
        placements: draw.placements,
        clipped: draw.clipped,
        submit_ok: draw.submit_ok,
        presented: draw.presented,
        submit_ms: draw.submit_ms,
        present_ms: draw.present_ms,
        total_ms: elapsed_ms_since(total_start),
    }
}

fn json_f32_field(node: &Value, key: &str) -> Option<f32> {
    let number = node.get(key)?.as_f64()?;
    if number.is_finite() {
        Some(number as f32)
    } else {
        None
    }
}

fn clamp_scroll_y_for_scene(scroll_y: f32, content_height: u32, viewport_height: u32) -> f32 {
    if scroll_y <= 0.0 || content_height <= viewport_height {
        return 0.0;
    }
    scroll_y.min(content_height.saturating_sub(viewport_height) as f32)
}

fn ceil_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    libm::ceilf(value).min(u32::MAX as f32) as u32
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

fn drain_ui3_wheel_delta(state: &mut Ui3WheelDrain) -> i32 {
    let mut out = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI3_WHEEL_EVENT_BATCH_CAP];
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(state.read_seq, out.as_mut_slice());
    state.read_seq = next_seq;
    let wheel_delta = out
        .iter()
        .take(wrote)
        .fold(0i32, |sum, event| sum.saturating_add(event.wheel as i32));
    if wheel_delta != 0 {
        crate::log!(
            "ui3-service: wheel events={} dropped={} delta={} read_seq={}\n",
            wrote,
            dropped,
            wheel_delta,
            state.read_seq
        );
    }
    wheel_delta
}
