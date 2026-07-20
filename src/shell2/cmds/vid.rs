use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const VID_SLOT: &str = "vid";

#[derive(Copy, Clone)]
struct VidCommand {
    loop_playback: bool,
}

struct VidUi4Session {
    active: bool,
}

impl VidUi4Session {
    fn begin() -> Option<Self> {
        crate::ui4::begin_shell_decoded_video_player().then_some(Self { active: true })
    }

    fn close(mut self) -> bool {
        let stopped = crate::ui4::stop_decoded_nv12_stream("shell2-vid-done");
        self.active = false;
        stopped
    }
}

impl Drop for VidUi4Session {
    fn drop(&mut self) {
        if self.active {
            let _ = crate::ui4::stop_decoded_nv12_stream("shell2-vid-task-drop");
            self.active = false;
        }
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let loop_playback = match (args.next(), args.next()) {
        (None, None) => false,
        (Some(arg), None) if arg.eq_ignore_ascii_case("loop") => true,
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    };
    let command = VidCommand { loop_playback };
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, VID_SLOT);
    set_matrix_target_active(&target, true);
    match vid_task(target.clone(), command) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: queued source=kernel-embedded asset={} fps=60 loop={}",
                    crate::intel::media::hw_vid::UI4_FRAMED_VIDEO_ASSET,
                    loop_playback as u8,
                )
                .as_str(),
            );
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(&target, alloc::format!("vid: task failed {err:?}").as_str());
        }
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vid: usage `vid [loop]`");
}

#[embassy_executor::task(pool_size = 1)]
async fn vid_task(target: MatrixTarget, command: VidCommand) {
    print_matrix_target_line(
        &target,
        alloc::format!(
            "vid: start source=kernel-embedded asset={} fps=60 loop={} path=vd_box+guc_simd16+ui4_double_frame",
            crate::intel::media::hw_vid::UI4_FRAMED_VIDEO_ASSET,
            command.loop_playback as u8,
        )
        .as_str(),
    );
    let Some(ui4_session) = VidUi4Session::begin() else {
        print_matrix_target_line(
            &target,
            "vid: UI4 video lifetime is already owned by another playback task",
        );
        set_matrix_target_active(&target, false);
        return;
    };
    crate::log_info!(
        target: "ui4";
        "shell2/vid: stage=ui4-lifetime-reserved next=embedded-annexb-decode frame-allocation=deferred-until-first-decoded-frame\n"
    );

    let mut lap = 0usize;
    loop {
        lap = lap.saturating_add(1);
        match crate::intel::media::hw_vid::run_ui4_framed_video_playback().await {
            Ok(report) => print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: done lap={} submitted={} skipped_unsupported={} target_fps={} elapsed_ms={} effective_fps={}.{:02} avg_decode_us={} avg_present_us={}",
                    lap,
                    report.submitted,
                    report.skipped_unsupported,
                    report.target_fps,
                    report.elapsed_ms,
                    report.effective_fps_x100 / 100,
                    report.effective_fps_x100 % 100,
                    report.avg_decode_us,
                    report.avg_present_us,
                )
                .as_str(),
            ),
            Err(err) => {
                print_matrix_target_line(&target, alloc::format!("vid: {err}").as_str());
                break;
            }
        }
        if !command.loop_playback {
            break;
        }
    }

    let stopped = ui4_session.close();
    print_matrix_target_line(
        &target,
        alloc::format!("vid: ui4 video-frame stopped={}", stopped as u8).as_str(),
    );
    set_matrix_target_active(&target, false);
}
