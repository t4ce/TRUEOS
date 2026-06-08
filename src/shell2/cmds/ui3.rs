use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const UI3_PIXI_TASK_NAME: &str = "ui3-pixi-service";

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "ui3 pixi status");
    print_shell_line(io, "ui3 pixi start");
    print_shell_line(io, "ui3 pixi stop");
}

fn pixi_task_index() -> Option<usize> {
    crate::r::spawn_service::task_index_by_name(UI3_PIXI_TASK_NAME)
}

fn pixi_status(io: &'static dyn ShellBackend2) {
    let Some(index) = pixi_task_index() else {
        print_shell_line(io, "ui3 pixi: task missing");
        return;
    };
    let started = crate::r::spawn_service::task_started_by_index(index);
    let ready = crate::ui3::pixi_service_ready();
    let renders = crate::ui3::pixi_service_render_count();
    let ops = crate::ui3::pixi_service_op_count();
    let frames = crate::ui3::pixi_service_frame_count();
    let draws = crate::ui3::pixi_service_draw_count();
    let msg = alloc::format!(
        "ui3 pixi: task={} started={} ready={} renders={} ops={} frames={} draws={}",
        index,
        started as u8,
        ready as u8,
        renders,
        ops,
        frames,
        draws
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(topic) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };
    if !topic.eq_ignore_ascii_case("pixi") {
        usage(io);
        return ParseOutcome::Handled;
    }

    let Some(action) = args.next() else {
        pixi_status(io);
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        usage(io);
        return ParseOutcome::Handled;
    }

    let Some(index) = pixi_task_index() else {
        print_shell_line(io, "ui3 pixi: task missing");
        return ParseOutcome::Handled;
    };

    match action {
        action if action.eq_ignore_ascii_case("status") => pixi_status(io),
        action if action.eq_ignore_ascii_case("start") => {
            crate::r::spawn_service::enable_task_by_index(index);
            let msg =
                alloc::format!("ui3 pixi: enabled task={} name={}", index, UI3_PIXI_TASK_NAME);
            print_shell_line(io, msg.as_str());
        }
        action if action.eq_ignore_ascii_case("stop") => {
            crate::r::spawn_service::disable_task_by_index(index);
            let requested = crate::r::spawn_service::request_task_stop_by_index(index);
            let msg = alloc::format!(
                "ui3 pixi: disabled task={} stop_requested={}",
                index,
                requested as u8
            );
            print_shell_line(io, msg.as_str());
        }
        _ => usage(io),
    }

    ParseOutcome::Handled
}
