use super::output::Output;
use alloc::format;

fn usage(io: &Output) {
    Output::line(io, "vcpy status|fillcheck|monocheck");
}

fn execute(io: &Output, rest: &str) {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (Some(action), None) if action.eq_ignore_ascii_case("monocheck") => {
            match crate::r::services::vcpy_service::mono_check() {
                Ok(()) => Output::line(
                    io,
                    "vcpy: monocheck passed glyphs=2 pixels=132 guards=380 command=xy-mono-src-copy-blt colors=foreground+background fallback=0",
                ),
                Err(reason) => {
                    Output::line(io, format!("vcpy: monocheck failed reason={reason}").as_str())
                }
            }
        }
        (Some(action), None) if action.eq_ignore_ascii_case("fillcheck") => {
            match crate::r::services::vcpy_service::fill_check() {
                Ok(()) => Output::line(
                    io,
                    "vcpy: fillcheck passed fills=2 pixels_per_fill=51 right_and_bottom_guards=77 colors=80402010,00000000 fallback=0",
                ),
                Err(reason) => {
                    Output::line(io, format!("vcpy: fillcheck failed reason={reason}").as_str())
                }
            }
        }
        (Some(action), None) if action.eq_ignore_ascii_case("status") => {
            let status = crate::r::services::vcpy_service::status();
            Output::line(
                io,
                format!(
                    "vcpy: consumer=gridpaper copies={} bytes={} fallbacks={} failures={}",
                    status.consumer_copies,
                    status.consumer_bytes,
                    status.consumer_fallbacks,
                    status.consumer_failures,
                )
                .as_str(),
            );
            Output::line(
                io,
                format!(
                    "vcpy: consumer=resident-scene copies={} bytes={} fallbacks={} failures={}",
                    status.scene_copies,
                    status.scene_bytes,
                    status.scene_fallbacks,
                    status.scene_failures,
                )
                .as_str(),
            );
            for (name, consumer) in [
                ("font-clear", crate::r::services::vcpy_service::RgbaFillConsumer::Font),
                #[cfg(feature = "gridpaper")]
                ("gridpaper-clear", crate::r::services::vcpy_service::RgbaFillConsumer::Gridpaper),
                ("ui4-base", crate::r::services::vcpy_service::RgbaFillConsumer::Ui4),
            ] {
                let [fills, bytes, fallbacks, failures] =
                    crate::r::services::vcpy_service::fill_stats(consumer);
                Output::line(
                    io,
                    format!(
                        "vcpy: consumer={} fills={} bytes={} fallbacks={} failures={}",
                        name, fills, bytes, fallbacks, failures,
                    )
                    .as_str(),
                );
            }
            let snapshot = crate::intel::media::wd_xyuv8888::snapshot_copy_stats();
            Output::line(io, format!(
                "vcpy: consumer=wd-snapshot copies={} bytes={} failures={} prepare_us={} admission_us={} submit_us={} retire_us={} acquire_us={} request_to_ready_us={} poll_ms=1",
                snapshot.copies, snapshot.bytes, snapshot.failures, snapshot.prepare_us,
                snapshot.admission_us, snapshot.submit_us, snapshot.retire_us,
                snapshot.acquire_us, snapshot.request_to_ready_us,
            ).as_str());
            if let Some(failure) = crate::intel::guc_bcs0_last_timeout() {
                let activity = failure.activity;
                Output::line(
                    io,
                    format!(
                        "vcpy: breadcrumbs ring=0x{:08X}/0xBC505247 batch=0x{:08X}/0xBC504242",
                        failure.ring_cookie, failure.batch_cookie,
                    )
                    .as_str(),
                );
                Output::line(io, format!(
                    "vcpy: pre-quarantine marker=0x{:08X}/0x{:08X} saved_head={} published_tail={} head=0x{:X} tail=0x{:X} start=0x{:X} ctl=0x{:X} acthd=0x{:X} ipeir=0x{:X} ipehr=0x{:X} eir=0x{:X}",
                    failure.observed, failure.expected, activity.guc_saved_head,
                    activity.guc_published_tail, activity.head, activity.tail,
                    activity.start, activity.ctl, activity.acthd,
                    activity.ipeir, activity.ipehr, activity.eir,
                ).as_str());
                Output::line(io, format!(
                    "vcpy: pre-quarantine mode=0x{:08X} mi_mode=0x{:08X} ctx_ctl=0x{:08X} el_ctl=0x{:08X} el_status=0x{:08X}:0x{:08X}",
                    activity.mode, activity.mi_mode, activity.context_control,
                    activity.execlist_control, activity.execlist_status_hi,
                    activity.execlist_status_lo,
                ).as_str());
                if let Some(context) = failure.context {
                    Output::line(io, format!(
                        "vcpy: guc context={} enabled={} pending_enable={} faulted={} submissions={} hwlrca=0x{:08X}:0x{:08X}",
                        context.context_id, context.enabled as u8,
                        context.pending_enable as u8, context.faulted as u8,
                        context.submissions,
                        context.hwlrca_hi, context.hwlrca_lo,
                    ).as_str());
                } else {
                    Output::line(io, "vcpy: guc context=absent-before-quarantine");
                }
            }
        }
        _ => usage(io),
    }
}

use super::{
    MatrixTarget,
    helper::{self, Action, Input, Screen},
    picker::{self, Event, Picker},
    tui::{self, Frontend},
};
use alloc::{string::String, vec::Vec};
use trueos_time::{Duration, Timer};
const ACTIONS: [&str; 3] = ["status", "fillcheck", "monocheck"];
pub(super) fn start(frontend: Frontend) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("vcpy", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("VCPY task pool is full.")
    })?;
    spawner.spawn(task);
    Ok(())
}
#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: MatrixTarget) {
    let mut picker = Picker::default();
    let mut screen = Screen::default();
    let mut result: Option<Vec<String>> = None;
    let mut scroll = 0usize;
    let mut input = Input::default();
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let (cols, rows) = (surface.cols as usize, surface.rows as usize);
        let now = crate::chronos::monotonic_nanos();
        if result.is_some() {
            let mut actions = input.feed(&bytes, now);
            if let Some(action) = input.timeout(now) {
                actions.push(action);
            }
            for action in actions {
                match action {
                    Action::Up => scroll = scroll.saturating_sub(1),
                    Action::Down => scroll = scroll.saturating_add(1),
                    Action::Choose | Action::Quit | Action::Click(1) => {
                        result = None;
                        scroll = 0;
                        break;
                    }
                    _ => {}
                }
            }
        } else if let Some(event) = picker.input(&bytes, ACTIONS.len(), rows, now) {
            match event {
                Event::Choose(index) => {
                    let output = Output::collect(cols);
                    execute(&output, ACTIONS[index]);
                    result = Some(output.finish());
                    scroll = 0;
                }
                Event::Quit => tui::native_return(&target),
                _ => {}
            }
        }
        if tui::native_visible(&target) {
            let lines = if let Some(content) = &result {
                picker::page(
                    "VCPY  copy and fill checks",
                    "Result",
                    &picker::wrap(content.clone(), cols),
                    &mut scroll,
                    rows,
                )
            } else {
                picker.frame(
                    "VCPY  copy and fill checks",
                    "Select a status report or hardware check.",
                    &ACTIONS,
                    rows,
                )
            };
            screen.paint(&target, &lines, cols, rows);
        }
        Timer::after(Duration::from_millis(20)).await;
    }
}
