use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const CPP_DEMO_DEFAULT_DURATION_MS: u64 = 30_000;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "cpp [gallery|aurora|julia|sdf|voronoi]");
    print_shell_line(
        io,
        "cpp start [gallery|aurora|julia|sdf|voronoi] [duration_ms] [cadence_ms] [publish_every]",
    );
    print_shell_line(io, "cpp list");
    print_shell_line(io, "cpp status");
    print_shell_line(io, "cpp stop");
}

fn parse_mode(raw: &str) -> Option<crate::ui4::GpgpuPreviewPreset> {
    if raw.eq_ignore_ascii_case("gallery") || raw.eq_ignore_ascii_case("all") {
        Some(crate::ui4::GpgpuPreviewPreset::CppGallery)
    } else if raw.eq_ignore_ascii_case("aurora") {
        Some(crate::ui4::GpgpuPreviewPreset::CppAurora)
    } else if raw.eq_ignore_ascii_case("julia") {
        Some(crate::ui4::GpgpuPreviewPreset::CppJulia)
    } else if raw.eq_ignore_ascii_case("sdf") {
        Some(crate::ui4::GpgpuPreviewPreset::CppSdf)
    } else if raw.eq_ignore_ascii_case("voronoi") {
        Some(crate::ui4::GpgpuPreviewPreset::CppVoronoi)
    } else {
        None
    }
}

const fn is_cpp_preset(preset: crate::ui4::GpgpuPreviewPreset) -> bool {
    matches!(
        preset,
        crate::ui4::GpgpuPreviewPreset::CppGallery
            | crate::ui4::GpgpuPreviewPreset::CppAurora
            | crate::ui4::GpgpuPreviewPreset::CppJulia
            | crate::ui4::GpgpuPreviewPreset::CppSdf
            | crate::ui4::GpgpuPreviewPreset::CppVoronoi
    )
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn parse_u64(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
    default: u64,
) -> Option<u64> {
    match args.next() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => Some(value),
            Err(_) => {
                usage(io);
                None
            }
        },
        None => Some(default),
    }
}

fn parse_u32(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
    default: u32,
) -> Option<u32> {
    match args.next() {
        Some(raw) => match raw.parse::<u32>() {
            Ok(value) => Some(value),
            Err(_) => {
                usage(io);
                None
            }
        },
        None => Some(default),
    }
}

fn start(
    io: &'static dyn ShellBackend2,
    preset: crate::ui4::GpgpuPreviewPreset,
    args: &mut SplitWhitespace<'_>,
) {
    let Some(duration_ms) = parse_u64(io, args, CPP_DEMO_DEFAULT_DURATION_MS) else {
        return;
    };
    let Some(cadence_ms) = parse_u64(io, args, crate::ui4::GPGPU_PREVIEW_DEFAULT_CADENCE_MS) else {
        return;
    };
    let Some(publish_every) = parse_u32(io, args, crate::ui4::GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY)
    else {
        return;
    };
    if !expect_no_more(io, args) {
        return;
    }

    let config = crate::ui4::GpgpuPreviewConfig {
        preset,
        duration_ms,
        cadence_ms,
        publish_every,
    };
    match crate::ui4::request_gpgpu_preview_start(config) {
        Ok(serial) => {
            let status = crate::ui4::gpgpu_preview_status();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp start: queued=1 request={} mode={} service_online={} duration_ms={} cadence_ms={} publish_every={} frontend=cpp-for-opencl backend=intel-igc-aot runtime_compiler=0 artifact={} zebin_sha256={} target=8086:4680-r0C kernel=cpp_demo_rgba8 simd=16 ui4_window=1 buffering=double plane=slot1-direct stop=\"cpp stop\"",
                    serial,
                    cpp_mode_label(preset),
                    status.online as u8,
                    duration_ms,
                    cadence_ms,
                    publish_every,
                    crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.name,
                    artifact_hash(),
                )
                .as_str(),
            );
        }
        Err(reason) => print_shell_line(
            io,
            alloc::format!("cpp start: queued=0 mode={} reason={reason}", cpp_mode_label(preset))
                .as_str(),
        ),
    }
}

const fn cpp_mode_label(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    match preset {
        crate::ui4::GpgpuPreviewPreset::CppGallery => "gallery",
        crate::ui4::GpgpuPreviewPreset::CppAurora => "aurora",
        crate::ui4::GpgpuPreviewPreset::CppJulia => "julia",
        crate::ui4::GpgpuPreviewPreset::CppSdf => "sdf",
        crate::ui4::GpgpuPreviewPreset::CppVoronoi => "voronoi",
        _ => "not-cpp",
    }
}

fn print_list(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "cpp demo: mode=gallery id=0 explores=multi-mode-dispatch/one-stable-ABI panels=aurora+julia+sdf+voronoi",
    );
    print_shell_line(
        io,
        "cpp demo: mode=aurora id=1 explores=native-transcendentals/vector-fields/animation",
    );
    print_shell_line(
        io,
        "cpp demo: mode=julia id=2 explores=bounded-iteration/branching/complex-arithmetic",
    );
    print_shell_line(
        io,
        "cpp demo: mode=sdf id=3 explores=signed-distance-geometry/antialiasing/composition",
    );
    print_shell_line(
        io,
        "cpp demo: mode=voronoi id=4 explores=integer-hashing/neighbour-search/procedural-cells",
    );
    print_shell_line(
        io,
        "cpp suite: source=cpp_demo_rgba8.clcpp frontend=cpp-for-opencl backend=intel-igc-aot build_time_only=1 exact_target=8086:4680-r0C",
    );
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    let active_cpp = status.desired_running && is_cpp_preset(status.config.preset);
    let upload = crate::intel::gpgpu::cpp_demo_rgba8_upload_status();
    print_shell_line(
        io,
        alloc::format!(
            "cpp status: active={} online={} phase={} request={} applied={} mode={} frame={} window={} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} marker=0x{:08X} submit_ms={} artifact={} resident={} verified={} gpu=0x{:X} zebin_sha256={} runtime_compiler=0 error={}",
            active_cpp as u8,
            status.online as u8,
            status.phase.label(),
            status.request_serial,
            status.applied_serial,
            if is_cpp_preset(status.config.preset) {
                cpp_mode_label(status.config.preset)
            } else {
                "none"
            },
            status.frame.map(|frame| frame.raw()).unwrap_or(0),
            status.window.map(|window| window.raw()).unwrap_or(0),
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.completed,
            status.metrics.published,
            status.metrics.dropped_busy,
            status.metrics.failed,
            status.metrics.late,
            status.metrics.elapsed_ms,
            status.metrics.last_marker,
            status.metrics.last_submit_ms,
            crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.name,
            upload.is_some() as u8,
            upload.is_some_and(|artifact| artifact.verified) as u8,
            upload.map(|artifact| artifact.gpu).unwrap_or(0),
            artifact_hash(),
            status.last_error,
        )
        .as_str(),
    );
}

fn artifact_hash() -> String {
    let mut out = String::with_capacity(64);
    for byte in crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.bin_sha256 {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(command) = args.next() else {
        start(io, crate::ui4::GpgpuPreviewPreset::CppGallery, args);
        return ParseOutcome::Handled;
    };

    if command.eq_ignore_ascii_case("start") {
        let preset = match args.clone().next().and_then(parse_mode) {
            Some(preset) => {
                let _ = args.next();
                preset
            }
            None => crate::ui4::GpgpuPreviewPreset::CppGallery,
        };
        start(io, preset, args);
    } else if command.eq_ignore_ascii_case("list") {
        if expect_no_more(io, args) {
            print_list(io);
        }
    } else if command.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_status(io);
        }
    } else if command.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, args) {
            let status = crate::ui4::gpgpu_preview_status();
            if status.desired_running && is_cpp_preset(status.config.preset) {
                let serial = crate::ui4::request_gpgpu_preview_stop();
                print_shell_line(
                    io,
                    alloc::format!("cpp stop: queued=1 request={serial}").as_str(),
                );
            } else {
                print_shell_line(io, "cpp stop: queued=0 reason=no-cpp-demo-active");
            }
        }
    } else if let Some(preset) = parse_mode(command) {
        start(io, preset, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
