//! Native Shell3 controls for playback routes and output state.

use super::{
    MatrixTarget,
    helper::{self, Screen},
    picker::{Event, Picker},
    tui::{self, Frontend},
};
use crate::r::services::audio_playback::{
    self, MAX_PRODUCERS, OutputBucket, ProducerOwner, RouteSource,
};
use alloc::{format, string::String, vec, vec::Vec};
use trueos_time::{Duration, Timer};

pub(super) const SLOT: &str = "audio-link";

pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit(SLOT, frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("Audio Link menu task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}

#[derive(Clone, Copy)]
enum Choice {
    SetHttp(bool),
    Route(u64, OutputBucket),
}

fn source_label(source: RouteSource) -> String {
    match source.owner {
        ProducerOwner::Host => format!("#{} Host", source.id),
        ProducerOwner::Video(_) => format!("#{} Video", source.id),
        ProducerOwner::Vm { id, generation } => {
            let archive = crate::hv::blueprint_current_run_archive(id, generation);
            let name = archive.as_deref().and_then(|path| path.rsplit('/').next());
            match name {
                Some(name) => format!("#{} {} (VM {})", source.id, name, id),
                None => format!("#{} Blueprint VM {}", source.id, id),
            }
        }
    }
}

fn choices(
    sources: [Option<RouteSource>; MAX_PRODUCERS],
    http_enabled: bool,
) -> Vec<(Choice, String)> {
    let mut choices = vec![(
        Choice::SetHttp(!http_enabled),
        String::from(if http_enabled {
            "HTTP server [On]   Turn off"
        } else {
            "HTTP server [Off]  Turn on"
        }),
    )];
    for source in sources.into_iter().flatten() {
        let label = source_label(source);
        let state = if source.active { "" } else { "Recent clip " };
        for (bucket, name, assigned) in [
            (OutputBucket::Http, "HTTP", source.http),
            (OutputBucket::Analog, "Jack", source.analog),
        ] {
            let mark = if assigned { 'x' } else { ' ' };
            choices.push((
                Choice::Route(source.id, bucket),
                format!("{name} [{mark}] {state}{label}"),
            ));
        }
    }
    choices
}

#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut picker = Picker::default();
    let mut screen = Screen::default();
    let mut painted = Vec::<(Choice, String)>::new();
    let mut visible_surface = None;
    let mut notice = String::new();
    while let Some((bytes, notices)) = tui::native_read(&target) {
        let Some(frontend) = tui::native_frontend(&target) else {
            visible_surface = None;
            Timer::after(Duration::from_millis(100)).await;
            continue;
        };
        let surface = (frontend.id, frontend.cols, frontend.rows);
        if visible_surface != Some(surface) {
            visible_surface = Some(surface);
            picker = Picker::default();
            screen.invalidate();
            painted.clear();
            notice.clear();
        }
        if let Some(last) = notices.into_iter().last() {
            notice = last;
        }

        // Match input against the producer IDs on the last painted rows.
        // An expired stream never retargets the click to a reused slot.
        let now = crate::chronos::monotonic_nanos();
        match picker.input(&bytes, painted.len(), frontend.rows, now) {
            Some(Event::Quit) => {
                tui::native_write(&target, b"\x1b[?1000l\x1b[?1006l\x1b[?25h\x1b[0m");
                tui::native_return(&target);
                visible_surface = None;
                continue;
            }
            Some(Event::Choose(index)) => {
                notice.clear();
                if let Some(&(choice, _)) = painted.get(index) {
                    match choice {
                        Choice::SetHttp(enabled) => audio_playback::set_http_enabled(enabled),
                        Choice::Route(id, bucket) => {
                            if !audio_playback::toggle_route(id, bucket) {
                                notice = String::from("Playback stream ended.");
                            }
                        }
                    }
                }
            }
            _ => {}
        }

        let snapshot = audio_playback::routing_snapshot();
        let connected = if snapshot.analog_connected {
            "Connected"
        } else {
            "Disconnected"
        };
        let mut summary = format!("Mainboard jack: {connected}");
        painted = choices(snapshot.sources, snapshot.http_enabled);
        if !notice.is_empty() {
            summary.push_str("  ");
            summary.push_str(&notice);
        } else if !snapshot
            .sources
            .into_iter()
            .flatten()
            .any(|source| source.active)
        {
            summary.push_str("  No active playback streams.");
        }
        picker.selected = picker.selected.min(painted.len() - 1);
        let labels: Vec<_> = painted.iter().map(|(_, label)| label.as_str()).collect();
        let lines = picker.frame("Audio Link", &summary, &labels, frontend.rows);
        screen.paint(&target, &lines, frontend.cols, frontend.rows);
        Timer::after(Duration::from_millis(100)).await;
    }
}
