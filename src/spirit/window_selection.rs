//! Spirit-owned UI4 window selection choreography.
//!
//! UI4 publishes an ordered event only after a new window's first composed
//! frame crosses plane SURFLIVE. This one task maps that trusted window name
//! to a Spirit background, flies Spirit's tagged *software* vCursor to the
//! frame, and invokes UI4 focus directly without synthesizing a mouse click.

use embassy_time::{Duration, Instant, Timer, with_timeout};

use super::spirit_vfx::SpiritVfxBackgroundEffect;

const SELECTION_MONITOR_MS: u64 = 16;
const CURSOR_SETTLE_POLL_MS: u64 = 8;
const INPUT_BROKER_SETTLE_MS: u64 = 24;
const CURSOR_SETTLE_GRACE_MS: u64 = 250;
const CURSOR_REGISTRATION_RETRY_MS: u64 = 50;

#[derive(Copy, Clone)]
struct WindowVfxProfile {
    app: &'static str,
    effect: SpiritVfxBackgroundEffect,
}

#[derive(Copy, Clone)]
struct SpiritWindowSelection {
    key: crate::ui4::CursorFrameKey,
    profile: WindowVfxProfile,
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn spirit_window_selection_task() {
    let source = loop {
        match super::lilly_cursor::selection_source() {
            Ok(source) => break source,
            Err(error) => {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: window selector waiting for software vcursor error={:?} retry_ms={}\n",
                    error,
                    CURSOR_REGISTRATION_RETRY_MS,
                );
                Timer::after(Duration::from_millis(CURSOR_REGISTRATION_RETRY_MS)).await;
            }
        }
    };
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: window selector online cursor={}:{}:{} kind={} cursor_plane=ui4-slot4 trigger=first-frame-post-surflive selection=direct-ui4-focus/no-click hardware_cur_pos=unchanged mappings=gridpaper:cyber-grid,weather:nebula-smoke,vid:bokeh-field\n",
        source.controller_id,
        source.slot_id,
        source.ep_target,
        source.hid_kind,
    );

    let mut owned_selection: Option<SpiritWindowSelection> = None;
    loop {
        if let Some(selection) = owned_selection
            && !selection_is_owned(source, selection.key)
        {
            super::spirit_vfx::set_window_background_vfx(None);
            owned_selection = None;
            crate::log_info!(
                target: "gfx";
                "trueos-spirit: window selection released owner={:?} window={} app={} action=restore-idle-vfx\n",
                selection.key.owner,
                selection.key.window.raw(),
                selection.profile.app,
            );
        }

        let Ok(window) = with_timeout(
            Duration::from_millis(SELECTION_MONITOR_MS),
            crate::ui4::wait_for_window_first_presentation(),
        )
        .await
        else {
            continue;
        };
        let Some(profile) = profile_for_window(window) else {
            continue;
        };
        let Some((screen_width, screen_height)) = crate::intel::active_scanout_dimensions() else {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: window selection deferred app={} owner={:?} window={} reason=no-active-scanout\n",
                profile.app,
                window.owner,
                window.id.raw(),
            );
            continue;
        };
        let approach_ms = match super::lilly_cursor::queue_window_approach(
            window.placement.x,
            window.placement.y,
            screen_width,
            screen_height,
        ) {
            Ok(duration_ms) => duration_ms,
            Err(error) => {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: window selection deferred app={} owner={:?} window={} reason=software-cursor-approach error={:?}\n",
                    profile.app,
                    window.owner,
                    window.id.raw(),
                    error,
                );
                continue;
            }
        };

        let settle_deadline = Instant::now()
            + Duration::from_millis(u64::from(approach_ms).saturating_add(CURSOR_SETTLE_GRACE_MS));
        let approach_complete = loop {
            match super::lilly_cursor::window_approach_complete() {
                Ok(true) => break true,
                Ok(false) if Instant::now() < settle_deadline => {
                    Timer::after(Duration::from_millis(CURSOR_SETTLE_POLL_MS)).await;
                }
                Ok(false) => break false,
                Err(error) => {
                    crate::log_warn!(
                        target: "gfx";
                        "trueos-spirit: window selection deferred app={} owner={:?} window={} reason=software-cursor-status error={:?}\n",
                        profile.app,
                        window.owner,
                        window.id.raw(),
                        error,
                    );
                    break false;
                }
            }
        };
        if !approach_complete {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: window selection deferred app={} owner={:?} window={} reason=software-cursor-timeout approach_ms={} grace_ms={}\n",
                profile.app,
                window.owner,
                window.id.raw(),
                approach_ms,
                CURSOR_SETTLE_GRACE_MS,
            );
            continue;
        }

        // The cursor station emits the final virtual HID sample when its
        // quadratic stroke retires. Give the 60 Hz UI4 input service one turn
        // to consume that sample before focus ownership is attached to it.
        Timer::after(Duration::from_millis(INPUT_BROKER_SETTLE_MS)).await;
        let changed = match crate::ui4::select_window_for_cursor(source, window.owner, window.id) {
            Ok(changed) => changed,
            Err(error) => {
                crate::log_warn!(
                    target: "gfx";
                    "trueos-spirit: window selection rejected app={} owner={:?} window={} error={:?} action=retain-current-selection\n",
                    profile.app,
                    window.owner,
                    window.id.raw(),
                    error,
                );
                continue;
            }
        };
        let key = crate::ui4::CursorFrameKey::new(window.owner, window.id);
        if !selection_is_owned(source, key) {
            crate::log_warn!(
                target: "gfx";
                "trueos-spirit: window selection rejected app={} owner={:?} window={} reason=selection-ownership-not-latched\n",
                profile.app,
                window.owner,
                window.id.raw(),
            );
            continue;
        }

        super::spirit_vfx::set_window_background_vfx(Some(profile.effect));
        owned_selection = Some(SpiritWindowSelection { key, profile });
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: window selected app={} producer={} owner={:?} window={} publish_serial={} vfx={} changed={} cursor={}:{}:{} selection=direct-ui4-focus/no-click first_frame=post-surflive hardware_cur_pos=unchanged\n",
            profile.app,
            window.producer_name,
            window.owner,
            window.id.raw(),
            window.publish_serial,
            profile.effect.ui_name(),
            changed,
            source.controller_id,
            source.slot_id,
            source.ep_target,
        );
    }
}

fn selection_is_owned(
    source: crate::ui4::Ui4CursorSource,
    key: crate::ui4::CursorFrameKey,
) -> bool {
    crate::ui4::source_selected(source) && crate::ui4::selected_frame() == Some(key)
}

fn profile_for_window(window: crate::ui4::WindowSnapshot) -> Option<WindowVfxProfile> {
    if window.owner == crate::ui4::WindowOwner::GRIDPAPER_SERVICE
        || window
            .producer_name
            .eq_ignore_ascii_case("gridpaper-service")
        || window.producer_name.eq_ignore_ascii_case("gridpaper")
    {
        return Some(WindowVfxProfile {
            app: "gridpaper",
            effect: SpiritVfxBackgroundEffect::CyberGrid,
        });
    }
    if window.owner == crate::ui4::WindowOwner::VIDEO_PLAYER
        || window.producer_name.eq_ignore_ascii_case("video-player")
        || window.producer_name.eq_ignore_ascii_case("vid")
    {
        return Some(WindowVfxProfile {
            app: "vid",
            effect: SpiritVfxBackgroundEffect::BokehField,
        });
    }
    let crate::ui4::WindowOwner::Vm(vm_id) = window.owner else {
        return None;
    };
    let archive = crate::hv::app_vm_archive(vm_id)?;
    if contains_ascii_case_insensitive(archive.as_str(), "weather")
        || contains_ascii_case_insensitive(archive.as_str(), "frog")
    {
        return Some(WindowVfxProfile {
            app: "weather",
            effect: SpiritVfxBackgroundEffect::NebulaSmoke,
        });
    }
    None
}

fn contains_ascii_case_insensitive(value: &str, needle: &str) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}
