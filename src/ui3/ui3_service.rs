use embassy_time::{Duration as EmbassyDuration, Timer};

const TASK_NAME: &str = "ui3-service";
const UI3_SERVICE_IDLE_MS: u64 = 16;

#[derive(Copy, Clone, Debug, Default)]
struct Ui3ServiceStats {
    frames_taken: u32,
    empty_polls: u32,
}

#[embassy_executor::task]
pub async fn ui3_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    crate::log!(
        "ui3-service: starting sink=render-tree-retained-image font=lucida-half mode=single-slot\n"
    );

    let mut stats = Ui3ServiceStats::default();
    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!(
                "ui3-service: stop requested frames={} empty_polls={}; exit\n",
                stats.frames_taken,
                stats.empty_polls
            );
            return;
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
            consume_render_tree_frame(frame, stats.frames_taken);
        }

        if !took_any {
            stats.empty_polls = stats.empty_polls.saturating_add(1);
            Timer::after(EmbassyDuration::from_millis(UI3_SERVICE_IDLE_MS)).await;
        }
    }
}

fn consume_render_tree_frame(frame: crate::surfer::Ui3RenderTreeFrame, taken_seq: u32) {
    let render_bytes = frame.render_tree_json.len();
    let layout_bytes = frame.layout_trace_json.len();
    crate::log!(
        "ui3-service: frame taken={} browser={} seq={} render_hash={} layout_hash={} render_bytes={} layout_bytes={} url={}\n",
        taken_seq,
        frame.browser_instance_id,
        frame.seq,
        frame.render_hash,
        frame.layout_hash,
        render_bytes,
        layout_bytes,
        frame.url
    );

    // Next step: parse `layout_trace_json` into retained text draw commands and submit those
    // commands through the sprite64 Lucida-half GPGPU font path.
}
