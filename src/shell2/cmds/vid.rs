use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::intel::media::hw_vid::{
    H264_BOOT_PROBE_STREAM_PATH, H264_MEDIA_URL_PROBE_URL, H264PlaybackCacheMode,
    H264PlaybackOptions,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const VID_SLOT: &str = "vid";
const DEFAULT_REVERSE: bool = false;
const DEFAULT_CACHE: H264PlaybackCacheMode = H264PlaybackCacheMode::Off;
const DEFAULT_STUDY: bool = false;
const DEFAULT_FILL: bool = false;
const DEFAULT_DIAGNOSTICS: bool = false;
const DEFAULT_NORESET_LITE: bool = true;
const DEFAULT_LOOP: bool = false;

#[derive(Copy, Clone)]
enum VidSource {
    TrueosFs,
    Online,
}

impl VidSource {
    const fn name(self) -> &'static str {
        match self {
            Self::TrueosFs => "trueosfs",
            Self::Online => "online",
        }
    }

    const fn asset(self) -> &'static str {
        match self {
            Self::TrueosFs => H264_BOOT_PROBE_STREAM_PATH,
            Self::Online => H264_MEDIA_URL_PROBE_URL,
        }
    }
}

#[derive(Copy, Clone)]
struct VidCommand {
    source: VidSource,
    options: H264PlaybackOptions,
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let Some(command) = parse_options(io, &mut args) else {
        return ParseOutcome::Handled;
    };
    let options = command.options;

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, VID_SLOT);
    set_matrix_target_active(&target, true);
    match vid_task(target.clone(), command) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: queued source={} asset={} fps={} mode={} cache={} study={} fill={} diag={} warm={} loop={}",
                    command.source.name(),
                    command.source.asset(),
                    options.fps(),
                    options.name(),
                    options.cache_mode().name(),
                    options.stripe_study() as u8,
                    options.show_cache_fill() as u8,
                    options.diagnostics() as u8,
                    options.noreset_lite() as u8,
                    options.loop_playback() as u8
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

fn parse_options(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> Option<VidCommand> {
    let fps = match args.next() {
        Some(raw) => match raw.parse::<u16>() {
            Ok(value @ 1..=144) => value,
            _ => {
                usage(io);
                return None;
            }
        },
        None => {
            usage(io);
            return None;
        }
    };

    let mut reverse = DEFAULT_REVERSE;
    let mut cache = DEFAULT_CACHE;
    let mut cache_set = false;
    let mut study = DEFAULT_STUDY;
    let mut fill = DEFAULT_FILL;
    let mut diagnostics = DEFAULT_DIAGNOSTICS;
    let mut noreset_lite = DEFAULT_NORESET_LITE;
    let mut loop_playback = DEFAULT_LOOP;
    let mut source = VidSource::TrueosFs;

    for arg in args {
        if arg.eq_ignore_ascii_case("reverse") || arg.eq_ignore_ascii_case("rev") {
            reverse = true;
        } else if arg.eq_ignore_ascii_case("forward") || arg.eq_ignore_ascii_case("fwd") {
            reverse = false;
        } else if arg.eq_ignore_ascii_case("study") {
            study = true;
            cache = H264PlaybackCacheMode::Full;
            cache_set = true;
        } else if arg.eq_ignore_ascii_case("nostudy") {
            study = false;
        } else if arg.eq_ignore_ascii_case("fill") {
            fill = true;
        } else if arg.eq_ignore_ascii_case("nofill") {
            fill = false;
        } else if arg.eq_ignore_ascii_case("debug") || arg.eq_ignore_ascii_case("diag") {
            diagnostics = true;
        } else if arg.eq_ignore_ascii_case("quiet") || arg.eq_ignore_ascii_case("fast") {
            diagnostics = false;
        } else if arg.eq_ignore_ascii_case("warm")
            || arg.eq_ignore_ascii_case("noreset")
            || arg.eq_ignore_ascii_case("noreset-lite")
        {
            noreset_lite = true;
        } else if arg.eq_ignore_ascii_case("cold") || arg.eq_ignore_ascii_case("reset") {
            noreset_lite = false;
        } else if arg.eq_ignore_ascii_case("loop") {
            loop_playback = true;
        } else if arg.eq_ignore_ascii_case("once") || arg.eq_ignore_ascii_case("noloop") {
            loop_playback = false;
        } else if arg.eq_ignore_ascii_case("browser")
            || arg.eq_ignore_ascii_case("latest")
            || arg.eq_ignore_ascii_case("web")
        {
            print_shell_line(
                io,
                "vid: browser/web source removed; use Surf for browsing or `vid <fps> online` for the built-in network video probe",
            );
            usage(io);
            return None;
        } else if arg.eq_ignore_ascii_case("fs")
            || arg.eq_ignore_ascii_case("trueosfs")
            || arg.eq_ignore_ascii_case("local")
        {
            source = VidSource::TrueosFs;
        } else if arg.eq_ignore_ascii_case("online")
            || arg.eq_ignore_ascii_case("net")
            || arg.eq_ignore_ascii_case("url")
        {
            source = VidSource::Online;
        } else if let Some(raw) = arg.strip_prefix("cache=") {
            cache = parse_cache(raw)?;
            cache_set = true;
        } else if arg.eq_ignore_ascii_case("full") {
            cache = H264PlaybackCacheMode::Full;
            cache_set = true;
        } else if arg.eq_ignore_ascii_case("tail") {
            cache = H264PlaybackCacheMode::Tail;
            cache_set = true;
        } else if arg.eq_ignore_ascii_case("off") {
            cache = H264PlaybackCacheMode::Off;
            cache_set = true;
        } else {
            usage(io);
            return None;
        }
    }

    let _ = cache_set;
    if !reverse && matches!(cache, H264PlaybackCacheMode::Tail) {
        cache = H264PlaybackCacheMode::Off;
    }

    Some(VidCommand {
        source,
        options: H264PlaybackOptions::new(
            fps,
            reverse,
            cache,
            study,
            fill,
            diagnostics,
            noreset_lite,
            loop_playback,
        ),
    })
}

fn parse_cache(raw: &str) -> Option<H264PlaybackCacheMode> {
    if raw.eq_ignore_ascii_case("full") {
        Some(H264PlaybackCacheMode::Full)
    } else if raw.eq_ignore_ascii_case("tail") {
        Some(H264PlaybackCacheMode::Tail)
    } else if raw.eq_ignore_ascii_case("off") || raw.eq_ignore_ascii_case("none") {
        Some(H264PlaybackCacheMode::Off)
    } else {
        None
    }
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "vid: usage `vid <fps 1..144> [trueosfs|online] [loop|once] [reverse|forward] [cache=full|tail|off] [study] [fill] [quiet|debug] [warm|cold]`",
    );
    print_shell_line(
        io,
        "vid: examples `vid 60`, `vid 60 online`, `vid 60 loop`, `vid 60 debug cold`, `vid 15 reverse`",
    );
}

#[embassy_executor::task(pool_size = 1)]
async fn vid_task(target: MatrixTarget, command: VidCommand) {
    let options = command.options;
    print_matrix_target_line(
        &target,
        alloc::format!(
            "vid: start source={} asset={} fps={} mode={} cache={} study={} fill={} diag={} warm={} loop={}",
            command.source.name(),
            command.source.asset(),
            options.fps(),
            options.name(),
            options.cache_mode().name(),
            options.stripe_study() as u8,
            options.show_cache_fill() as u8,
            options.diagnostics() as u8,
            options.noreset_lite() as u8,
            options.loop_playback() as u8
        )
        .as_str(),
    );

    let mut lap = 0usize;
    loop {
        lap = lap.saturating_add(1);
        let result = match command.source {
            VidSource::TrueosFs => {
                crate::intel::media::hw_vid::run_shell_vid_playback(options).await
            }
            VidSource::Online => {
                crate::intel::media::hw_vid::run_online_vid_playback(options).await
            }
        };
        match result {
            Ok(report) => print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: done lap={} submitted={} target_fps={} elapsed_ms={} effective_fps={}.{:02} waited={} late={} wait_ms={} avg_decode_us={} max_decode_us={} max_late_ms={} avg_process_us={} avg_reset_us={} avg_zero_clear_us={} avg_zero_us={} avg_scratch_zero_us={} avg_output_clear_us={} avg_missing_clear_us={} avg_scratch_flush_us={} avg_build_ctx_us={} avg_poll_us={} avg_post_us={} avg_present_us={}",
                    lap,
                    report.submitted,
                    report.target_fps,
                    report.elapsed_ms,
                    report.effective_fps_x100 / 100,
                    report.effective_fps_x100 % 100,
                    report.waited_frames,
                    report.late_frames,
                    report.total_wait_ms,
                    report.avg_decode_us,
                    report.max_decode_us,
                    report.max_late_ms,
                    report.avg_process_us,
                    report.avg_reset_us,
                    report.avg_zero_clear_us,
                    report.avg_zero_us,
                    report.avg_scratch_zero_us,
                    report.avg_output_clear_us,
                    report.avg_missing_clear_us,
                    report.avg_scratch_flush_us,
                    report.avg_build_ctx_us,
                    report.avg_poll_us,
                    report.avg_post_us,
                    report.avg_present_us
                )
                .as_str(),
            ),
            Err(err) => {
                print_matrix_target_line(&target, alloc::format!("vid: {err}").as_str());
                break;
            }
        }
        if !options.loop_playback() {
            break;
        }
    }
    set_matrix_target_active(&target, false);
}
