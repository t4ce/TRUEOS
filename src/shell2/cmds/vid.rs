use alloc::{string::String, vec::Vec};

use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const VID_SLOT: &str = "vid";

struct VidCommand {
    source: VidSource,
    loop_playback: bool,
}

enum VidSource {
    TrueosFs(String),
    Online,
}

impl VidSource {
    const fn name(&self) -> &'static str {
        match self {
            Self::TrueosFs(_) => "trueosfs",
            Self::Online => "online-mp4",
        }
    }

    fn asset(&self) -> &str {
        match self {
            Self::TrueosFs(path) => path.as_str(),
            Self::Online => "fixed-online-avc1-mp4",
        }
    }

    const fn next_stage(&self) -> &'static str {
        match self {
            Self::TrueosFs(_) => "trueosfs-annexb-load-decode",
            Self::Online => "fixed-mp4-download-demux-decode",
        }
    }
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

fn parse_args(rest: &str) -> Result<Vec<String>, &'static str> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in rest.trim().chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                current.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch.is_whitespace() {
            if !current.is_empty() {
                args.push(current);
                current = String::new();
            }
            continue;
        }
        current.push(ch);
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if escaped {
        current.push('\\');
    }
    if !current.is_empty() {
        args.push(current);
    }
    Ok(args)
}

fn normalize_trueosfs_path(path: &str) -> Result<String, &'static str> {
    crate::r::path::FsPath::parse(path, false)
        .map(|path| path.to_relative_string())
        .map_err(|_| "bad TRUEOSFS path")
}

fn parse_command(rest: &str) -> Result<VidCommand, &'static str> {
    let args = parse_args(rest)?;
    let Some(source) = args.first() else {
        return Err("missing source");
    };
    let mut loop_playback = false;

    let source = if source.eq_ignore_ascii_case("fs") {
        let mut path = None;
        for arg in &args[1..] {
            if arg.eq_ignore_ascii_case("loop") {
                if loop_playback {
                    return Err("duplicate loop option");
                }
                loop_playback = true;
            } else if path.is_none() {
                path = Some(normalize_trueosfs_path(arg)?);
            } else {
                return Err("too many filesystem arguments");
            }
        }
        VidSource::TrueosFs(path.unwrap_or_else(|| {
            String::from(crate::intel::media::hw_vid::UI4_FRAMED_VIDEO_FS_DEFAULT_PATH)
        }))
    } else if source.eq_ignore_ascii_case("on") || source.eq_ignore_ascii_case("online") {
        for arg in &args[1..] {
            if arg.eq_ignore_ascii_case("loop") && !loop_playback {
                loop_playback = true;
            } else {
                return Err("online accepts only the loop option");
            }
        }
        VidSource::Online
    } else {
        return Err("source must be fs or on");
    };

    Ok(VidCommand {
        source,
        loop_playback,
    })
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let command = match parse_command(rest) {
        Ok(command) => command,
        Err(err) => {
            print_shell_line(io, alloc::format!("vid: {err}").as_str());
            usage(io);
            return ParseOutcome::Handled;
        }
    };
    let queued = alloc::format!(
        "vid: queued source={} asset={} fps=60 loop={}",
        command.source.name(),
        command.source.asset(),
        command.loop_playback as u8,
    );
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, VID_SLOT);
    set_matrix_target_active(&target, true);
    match vid_task(target.clone(), command) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(&target, queued.as_str());
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(&target, alloc::format!("vid: task failed {err:?}").as_str());
        }
    }
    ParseOutcome::Handled
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "vid: usage `vid fs [path] [loop]` | `vid on [loop]` (`online` also accepted)",
    );
}

#[embassy_executor::task(pool_size = 1)]
async fn vid_task(target: MatrixTarget, command: VidCommand) {
    print_matrix_target_line(
        &target,
        alloc::format!(
            "vid: start source={} asset={} fps=60 loop={} path=vd_box+guc_simd16+ui4_double_frame",
            command.source.name(),
            command.source.asset(),
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
        "shell2/vid: stage=ui4-lifetime-reserved source={} next={} frame-allocation=deferred-until-first-decoded-frame\n",
        command.source.name(),
        command.source.next_stage(),
    );

    let mut lap = 0usize;
    loop {
        lap = lap.saturating_add(1);
        let result = match &command.source {
            VidSource::TrueosFs(path) => {
                crate::intel::media::hw_vid::run_trueosfs_ui4_framed_video_playback(path.as_str())
                    .await
            }
            VidSource::Online => {
                crate::intel::media::hw_vid::run_online_ui4_framed_video_playback().await
            }
        };
        match result {
            Ok(report) => print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: done lap={} attempted={} retired={} presented={} first_failure_frame={} first_failure_error={} skipped_unsupported={} target_fps={} elapsed_ms={} effective_fps={}.{:02} avg_decode_us={} avg_present_us={} mode_transitions={} engine_resets={}",
                    lap,
                    report.attempted,
                    report.retired,
                    report.presented,
                    report.first_failure_frame,
                    report.first_failure_error,
                    report.skipped_unsupported,
                    report.target_fps,
                    report.elapsed_ms,
                    report.effective_fps_x100 / 100,
                    report.effective_fps_x100 % 100,
                    report.avg_decode_us,
                    report.avg_present_us,
                    report.mode_transitions,
                    report.engine_resets,
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
