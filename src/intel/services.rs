use embassy_time::{Duration as EmbassyDuration, Timer};
use heapless::Vec;

const MAX_CURSOR_SOURCES: usize = 8;
const CURSOR_SERVICE_TICK_MS: u64 = 33;
const CURSOR_OVERLAY_REFRESH_TICKS: u32 = 30;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CursorOverlayEntry {
    slot_id: u32,
    buttons_down: u32,
    x_px: u32,
    y_px: u32,
}

#[embassy_executor::task]
pub async fn intel_display_service_task() {
    crate::log!("intel/display-svc: task online\n");

    let mut last_entries: Vec<CursorOverlayEntry, MAX_CURSOR_SOURCES> = Vec::new();
    let mut stable_idle_ticks: u32 = 0;

    loop {
        let entries = collect_cursor_overlay_entries();
        if entries != last_entries {
            stable_idle_ticks = 0;
            apply_cursor_overlay(entries.as_slice());
            log_cursor_overlay(entries.as_slice());
            last_entries.clear();
            for entry in entries.iter().copied() {
                let _ = last_entries.push(entry);
            }
        } else if !entries.is_empty() {
            stable_idle_ticks = stable_idle_ticks.saturating_add(1);
            if stable_idle_ticks.is_multiple_of(CURSOR_OVERLAY_REFRESH_TICKS) {
                apply_cursor_overlay(entries.as_slice());
            }
            if stable_idle_ticks == 1 || stable_idle_ticks.is_multiple_of(300) {
                crate::log!(
                    "intel/display-svc: cursor overlay stable active={} ticks={}\n",
                    entries.len(),
                    stable_idle_ticks
                );
            }
        } else {
            stable_idle_ticks = 0;
        }

        Timer::after(EmbassyDuration::from_millis(CURSOR_SERVICE_TICK_MS)).await;
    }
}

fn collect_cursor_overlay_entries() -> Vec<CursorOverlayEntry, MAX_CURSOR_SOURCES> {
    let mut out = Vec::new();
    let Some((vp_w, vp_h)) = crate::intel::display::active_scanout_dimensions() else {
        return out;
    };
    if vp_w == 0 || vp_h == 0 {
        return out;
    }

    let snapshots = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    for (idx, (slot_id, x, y, buttons_down)) in snapshots.into_iter().enumerate() {
        if idx >= MAX_CURSOR_SOURCES {
            break;
        }
        let x_px = norm_to_px(x, vp_w);
        let y_px = norm_to_px(y, vp_h);
        let _ = out.push(CursorOverlayEntry {
            slot_id,
            buttons_down,
            x_px,
            y_px,
        });
    }
    out
}

fn norm_to_px(value: f64, extent: u32) -> u32 {
    if extent <= 1 {
        return 0;
    }
    let clamped = if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let max = (extent - 1) as f64;
    (clamped * max) as u32
}

fn log_cursor_overlay(entries: &[CursorOverlayEntry]) {
    if entries.is_empty() {
        crate::log!("intel/display-svc: cursor overlay active=0\n");
        return;
    }

    for entry in entries {
        let color = crate::r::ui2::cursor_color(entry.slot_id);
        crate::log!(
            "intel/display-svc: cursor-overlay slot={} pos={}x{} buttons=0x{:X} rgba=#{:02X}{:02X}{:02X}{:02X}\n",
            entry.slot_id,
            entry.x_px,
            entry.y_px,
            entry.buttons_down,
            color.0,
            color.1,
            color.2,
            color.3
        );
    }
}

fn apply_cursor_overlay(entries: &[CursorOverlayEntry]) {
    if entries.is_empty() {
        let _ = crate::intel::display::disable_cursor_overlay();
        return;
    }

    let mut overlay_entries: Vec<(u32, u32, u32, u32), MAX_CURSOR_SOURCES> = Vec::new();
    for entry in entries {
        let _ = overlay_entries.push((entry.slot_id, entry.x_px, entry.y_px, entry.buttons_down));
    }
    let _ = crate::intel::display::update_cursor_overlay(overlay_entries.as_slice());
}
