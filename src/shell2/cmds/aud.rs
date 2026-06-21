use embassy_executor::Spawner;

use super::super::{
    CommandSessionInputResult, MatrixTarget, ShellBackend2, matrix_target_for_backend,
    print_matrix_target_line, print_shell_line, set_matrix_target_active,
    switch_matrix_target_slot,
};
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const AUD_PATH: &str = "aud.m4a";
const AUD_SLOT: &str = "aud";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "aud: usage `aud`");
        return ParseOutcome::Handled;
    }

    let _ = spawner;
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, AUD_SLOT);
    print_matrix_target_line(&target, format!("aud: queued {}", AUD_PATH).as_str());
    crate::aud::pcm_lane::set_paused(false);
    set_matrix_target_active(&target, true);

    if let Err(err) = crate::aud::file_service::submit_default(target.clone()) {
        set_matrix_target_active(&target, false);
        print_matrix_target_line(&target, format!("aud: service {err}").as_str());
    }

    print_matrix_target_line(
        &target,
        "aud: controls `pause`, `play`, `stop`, `vol 0..100`, `status`, `exit`",
    );

    ParseOutcome::StartSession(CommandSessionKind::AudControl)
}

pub(crate) fn handle_session_input(
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    let cmd = submitted.trim();
    if cmd.is_empty() {
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("exit")
        || cmd.eq_ignore_ascii_case("quit")
        || cmd.eq_ignore_ascii_case("q")
    {
        print_matrix_target_line(target, "aud: control session closed");
        return CommandSessionInputResult::CompleteIdle;
    }

    if cmd.eq_ignore_ascii_case("pause") {
        crate::aud::pcm_lane::set_paused(true);
        print_matrix_target_line(target, "aud: paused");
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("play") || cmd.eq_ignore_ascii_case("resume") {
        crate::aud::pcm_lane::set_paused(false);
        print_matrix_target_line(target, "aud: playing");
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("start") {
        crate::aud::pcm_lane::set_paused(false);
        print_matrix_target_line(target, format!("aud: queued {}", AUD_PATH).as_str());
        set_matrix_target_active(target, true);
        if let Err(err) = crate::aud::file_service::submit_default(target.clone()) {
            set_matrix_target_active(target, false);
            print_matrix_target_line(target, format!("aud: service {err}").as_str());
        }
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("stop") {
        let generation = crate::aud::pcm_lane::request_stop();
        print_matrix_target_line(target, format!("aud: stop requested gen={generation}").as_str());
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("status") {
        print_status(target);
        return CommandSessionInputResult::KeepRunning;
    }

    if let Some(rest) = cmd
        .strip_prefix("vol ")
        .or_else(|| cmd.strip_prefix("volume "))
    {
        match rest.trim().parse::<u16>() {
            Ok(percent) => {
                let percent = crate::aud::pcm_lane::set_volume_percent(percent);
                print_matrix_target_line(target, format!("aud: volume {percent}%").as_str());
            }
            Err(_) => print_matrix_target_line(target, "aud: usage `vol 0..100`"),
        }
        return CommandSessionInputResult::KeepRunning;
    }

    print_matrix_target_line(
        target,
        "aud: controls `pause`, `play`, `stop`, `vol 0..100`, `status`, `exit`",
    );
    CommandSessionInputResult::KeepRunning
}

fn print_status(target: &MatrixTarget) {
    let paused = crate::aud::pcm_lane::paused();
    let volume = crate::aud::pcm_lane::volume_percent();
    let pending = crate::aud::pcm_lane::urgent_pending();
    print_matrix_target_line(
        target,
        alloc::format!("aud: status paused={} volume={}% pending={}", paused, volume, pending)
            .as_str(),
    );
}
