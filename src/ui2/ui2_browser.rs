use super::*;

pub(super) fn log_browser_surface_updates(state: &mut Ui2State) {
    let mut mismatches = Vec::new();
    for window in &state.windows {
        if window.kind != Ui2WindowKind::HostedBrowser
            || !window_content_participates_in_composition(window)
        {
            continue;
        }
        let snapshot = browser_surface_state_for_window(window);
        if let Some(content) = window_content_rect(state, window) {
            let (_, _, want_w, want_h) = snap_browser_content_rect(content);
            if snapshot.viewport_width != want_w || snapshot.viewport_height != want_h {
                mismatches.push((
                    window.id,
                    snapshot.viewport_width,
                    snapshot.viewport_height,
                    want_w,
                    want_h,
                ));
            }
        }
    }

    for (window_id, have_w, have_h, want_w, want_h) in mismatches {
        let _ = note_window_viewport_sync_needed(state, window_id);
        crate::log!(
            "ui2: browser-viewport-mismatch window={} have={}x{} want={}x{}\n",
            window_id,
            have_w,
            have_h,
            want_w,
            want_h
        );
    }
}

pub(super) fn draw_hosted_browser_window_content(
    state: &Ui2State,
    window: &Ui2Window,
    content: Ui2Rect,
    chrome_ms: u64,
) -> Ui2WindowDrawTiming {
    let scene_started_at = Instant::now();
    if !window
        .hosted_browser_snapshot
        .gadget_snapshot
        .gadgets
        .is_empty()
        && browser_text::draw_hosted_browser_gadget_scene(state, window, content)
    {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: 0,
            placeholder_ms: elapsed_ms_since(scene_started_at),
            content_path: "browser-gadget-scene",
        };
    }

    let content_started_at = Instant::now();
    if texture_is_drawable(window.content_tex_id)
        && draw_texture_rect_no_present(
            window.content_tex_id,
            content.x,
            content.y,
            content.w,
            content.h,
            state.view_w,
            state.view_h,
            true,
            window.alpha,
        )
    {
        return Ui2WindowDrawTiming {
            chrome_ms,
            texture_ms: elapsed_ms_since(content_started_at),
            placeholder_ms: 0,
            content_path: "browser-texture",
        };
    }

    Ui2WindowDrawTiming {
        chrome_ms,
        texture_ms: 0,
        placeholder_ms: elapsed_ms_since(scene_started_at),
        content_path: "browser-empty",
    }
}
