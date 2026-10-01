use alloc::format;

use trueos_executor::Spawner;

use super::super::{print_shell_line, ShellBackend2};
use super::super::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vcpy start|stop|status|fillcheck");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (Some(action), None) if action.eq_ignore_ascii_case("start") => {
            match crate::r::services::vcpy_service::start(spawner) {
                Ok(()) => print_shell_line(io, "vcpy: service started (250 ms cadence)"),
                Err(reason) => print_shell_line(io, format!("vcpy: start failed: {reason}").as_str()),
            }
        }
        (Some(action), None) if action.eq_ignore_ascii_case("stop") => {
            crate::r::services::vcpy_service::stop();
            print_shell_line(io, "vcpy: stop requested");
        }
        (Some(action), None) if action.eq_ignore_ascii_case("fillcheck") => {
            match crate::r::services::vcpy_service::fill_check() {
                Ok(()) => print_shell_line(io, "vcpy: fillcheck passed fills=2 pixels_per_fill=51 right_and_bottom_guards=77 colors=80402010,00000000 fallback=0"),
                Err(reason) => print_shell_line(io, format!("vcpy: fillcheck failed reason={reason}").as_str()),
            }
        }
        (Some(action), None) if action.eq_ignore_ascii_case("status") => {
            let status = crate::r::services::vcpy_service::status();
            let demo = crate::ui4::vcpy_demo::status();
            print_shell_line(
                io,
                format!(
                    "vcpy: running={} polls={} failures={} rows={}/20 pending={} pinned={} marker_retired={} copy_cadence_ms=250",
                    status.running as u8,
                    status.ticks,
                    status.failures,
                    demo.published_rows,
                    demo.pending as u8,
                    demo.pinned as u8,
                    demo.marker_retired as u8,
                )
                .as_str(),
            );
            print_shell_line(io, format!(
                "vcpy: consumer=gridpaper copies={} bytes={} fallbacks={} failures={}",
                status.consumer_copies, status.consumer_bytes,
                status.consumer_fallbacks, status.consumer_failures,
            ).as_str());
            print_shell_line(io, format!(
                "vcpy: consumer=resident-scene copies={} bytes={} fallbacks={} failures={}",
                status.scene_copies, status.scene_bytes,
                status.scene_fallbacks, status.scene_failures,
            ).as_str());
            for (name, consumer) in [
                ("font-clear", crate::r::services::vcpy_service::RgbaFillConsumer::Font),
                ("gridpaper-clear", crate::r::services::vcpy_service::RgbaFillConsumer::Gridpaper),
            ] {
                let [fills, bytes, fallbacks, failures] = crate::r::services::vcpy_service::fill_stats(consumer);
                print_shell_line(io, format!(
                    "vcpy: consumer={} fills={} bytes={} fallbacks={} failures={}",
                    name, fills, bytes, fallbacks, failures,
                ).as_str());
            }
            let snapshot = crate::intel::media::wd_xyuv8888::snapshot_copy_stats();
            print_shell_line(io, format!(
                "vcpy: consumer=wd-snapshot copies={} bytes={} failures={} prepare_us={} admission_us={} submit_us={} retire_us={} acquire_us={} request_to_ready_us={} poll_ms=1",
                snapshot.copies, snapshot.bytes, snapshot.failures, snapshot.prepare_us,
                snapshot.admission_us, snapshot.submit_us, snapshot.retire_us,
                snapshot.acquire_us, snapshot.request_to_ready_us,
            ).as_str());
            if let Some(failure) = crate::intel::guc_bcs0_last_timeout() {
                let activity = failure.activity;
                print_shell_line(io, format!(
                    "vcpy: breadcrumbs ring=0x{:08X}/0xBC505247 batch=0x{:08X}/0xBC504242",
                    failure.ring_cookie, failure.batch_cookie,
                ).as_str());
                print_shell_line(io, format!(
                    "vcpy: pre-quarantine marker=0x{:08X}/0x{:08X} saved_head={} published_tail={} head=0x{:X} tail=0x{:X} start=0x{:X} ctl=0x{:X} acthd=0x{:X} ipeir=0x{:X} ipehr=0x{:X} eir=0x{:X}",
                    failure.observed, failure.expected, activity.guc_saved_head,
                    activity.guc_published_tail, activity.head, activity.tail,
                    activity.start, activity.ctl, activity.acthd,
                    activity.ipeir, activity.ipehr, activity.eir,
                ).as_str());
                print_shell_line(io, format!(
                    "vcpy: pre-quarantine mode=0x{:08X} mi_mode=0x{:08X} ctx_ctl=0x{:08X} el_ctl=0x{:08X} el_status=0x{:08X}:0x{:08X}",
                    activity.mode, activity.mi_mode, activity.context_control,
                    activity.execlist_control, activity.execlist_status_hi,
                    activity.execlist_status_lo,
                ).as_str());
                if let Some(context) = failure.context {
                    print_shell_line(io, format!(
                        "vcpy: guc context={} enabled={} pending_enable={} faulted={} submissions={} hwlrca=0x{:08X}:0x{:08X}",
                        context.context_id, context.enabled as u8,
                        context.pending_enable as u8, context.faulted as u8,
                        context.submissions,
                        context.hwlrca_hi, context.hwlrca_lo,
                    ).as_str());
                } else {
                    print_shell_line(io, "vcpy: guc context=absent-before-quarantine");
                }
            }
        }
        _ => usage(io),
    }
    ParseOutcome::Handled
}
