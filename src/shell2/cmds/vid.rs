use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::intel::media::hw_vid::{
    H264_BOOT_PROBE_STREAM_PATH, H264PlaybackCacheMode, H264PlaybackOptions,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const VID_SLOT: &str = "vid";
const DEFAULT_REVERSE: bool = false;
const DEFAULT_CACHE: H264PlaybackCacheMode = H264PlaybackCacheMode::Off;
const DEFAULT_STUDY: bool = false;
const DEFAULT_FILL: bool = false;

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let Some(options) = parse_options(io, &mut args) else {
        return ParseOutcome::Handled;
    };

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, VID_SLOT);
    set_matrix_target_active(&target, true);
    match vid_task(target.clone(), options) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(
                &target,
                alloc::format!(
                    "vid: queued {} fps={} mode={} cache={} study={} fill={}",
                    H264_BOOT_PROBE_STREAM_PATH,
                    options.fps(),
                    options.name(),
                    options.cache_mode().name(),
                    options.stripe_study() as u8,
                    options.show_cache_fill() as u8
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
) -> Option<H264PlaybackOptions> {
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

    if reverse && !cache_set {
        cache = H264PlaybackCacheMode::Full;
    }
    if !reverse && matches!(cache, H264PlaybackCacheMode::Tail) {
        cache = H264PlaybackCacheMode::Off;
    }

    Some(H264PlaybackOptions::new(fps, reverse, cache, study, fill))
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
        "vid: usage `vid <fps 1..144> [reverse|forward] [cache=full|tail|off] [study] [fill]`",
    );
    print_shell_line(
        io,
        "vid: examples `vid 30`, `vid 15 reverse`, `vid 60 reverse cache=off fill`",
    );
}

#[embassy_executor::task(pool_size = 1)]
async fn vid_task(target: MatrixTarget, options: H264PlaybackOptions) {
    print_matrix_target_line(
        &target,
        alloc::format!(
            "vid: start asset={} fps={} mode={} cache={} study={} fill={}",
            H264_BOOT_PROBE_STREAM_PATH,
            options.fps(),
            options.name(),
            options.cache_mode().name(),
            options.stripe_study() as u8,
            options.show_cache_fill() as u8
        )
        .as_str(),
    );

    match crate::intel::media::hw_vid::run_shell_vid_playback(options).await {
        Ok(()) => print_matrix_target_line(&target, "vid: done"),
        Err(err) => print_matrix_target_line(&target, alloc::format!("vid: {err}").as_str()),
    }
    set_matrix_target_active(&target, false);
}
