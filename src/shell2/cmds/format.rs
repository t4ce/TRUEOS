use core::str::SplitWhitespace;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{MatrixTarget, ShellBackend2, print_matrix_target_line, print_shell_line, set_matrix_target_active};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if args.next().is_some() {
        print_shell_line(io, "format: usage `format`");
        return ParseOutcome::Handled;
    }

    let Some(root) = crate::v::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "format: no TRUEOSFS root mounted");
        return ParseOutcome::Handled;
    };
    let info = root.info();
    let msg = alloc::format!(
        "format: target id={} ({}) label={:?}",
        info.id.raw(),
        info.id,
        info.label,
    );
    print_shell_line(io, msg.as_str());
    print_shell_line(io, "format: destructive action");
    print_shell_line(io, "format: type `sure`");
    ParseOutcome::StartSession(CommandSessionKind::FormatSure)
}

pub(crate) fn handle_session_input(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    if !submitted.eq_ignore_ascii_case("sure") {
        print_matrix_target_line(target, "format: cancelled");
        return CommandSessionInputResult::CompleteIdle;
    }

    submit_format(spawner, io, target);
    CommandSessionInputResult::CompleteRunning
}

fn submit_format(spawner: &Spawner, io: &'static dyn ShellBackend2, target: &MatrixTarget) {
    let Some(root) = crate::v::fs::trueosfs::primary_root_handle() else {
        print_shell_line(io, "format: no TRUEOSFS root mounted");
        return;
    };

    let info = root.info();
    print_matrix_target_line(
        target,
        alloc::format!(
            "format: starting on mounted root id={} ({})",
            info.id.raw(),
            info.id
        )
        .as_str(),
    );

    set_matrix_target_active(target, true);
    if spawner.spawn(format_command_task(target.clone(), root)).is_err() {
        set_matrix_target_active(target, false);
        print_shell_line(io, "format: spawn failed");
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn format_command_task(target: MatrixTarget, root: crate::disc::block::DeviceHandle) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        let info = root.info();
        log(
            alloc::format!(
                "format: target id={} ({}) blocks={} bs={} writable={} label={:?}",
                info.id.raw(),
                info.id,
                info.block_count,
                info.block_size,
                info.writable,
                info.label,
            )
            .as_str(),
        );

        log("format: mode=disk");
        match crate::v::fs::trueosfs::format_blank_force_async(root).await {
            Ok(()) => log("format: ok"),
            Err(e) => log(alloc::format!("format: failed ({:?})", e).as_str()),
        }
    }
    .await;
    set_matrix_target_active(&target, false);
}
