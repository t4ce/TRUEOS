use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const CPP_DEMO_DEFAULT_DURATION_MS: u64 = 30_000;
const CPP_AUDIO_DEFAULT_DURATION_MS: u64 = 0;
const CPP_AUDIO_DEFAULT_CADENCE_MS: u64 = 50;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "cpp [gallery|aurora|julia|sdf|voronoi|audio]");
    print_shell_line(
        io,
        "cpp start [gallery|aurora|julia|sdf|voronoi|audio] [duration_ms] [cadence_ms] [publish_every]",
    );
    print_shell_line(io, "cpp list");
    print_shell_line(io, "cpp status");
    print_shell_line(io, "cpp stop");
    print_shell_line(io, "cpp spirit [status|list|clean]");
    print_shell_line(io, "cpp spirit show <background_id> <shader_id>");
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
    } else if raw.eq_ignore_ascii_case("audio")
        || raw.eq_ignore_ascii_case("av")
        || raw.eq_ignore_ascii_case("visualizer")
    {
        Some(crate::ui4::GpgpuPreviewPreset::CppAudio)
    } else {
        None
    }
}

const fn is_cpp_preset(preset: crate::ui4::GpgpuPreviewPreset) -> bool {
    preset.is_cpp()
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
    let audio = preset == crate::ui4::GpgpuPreviewPreset::CppAudio;
    let default_duration_ms = if audio {
        CPP_AUDIO_DEFAULT_DURATION_MS
    } else {
        CPP_DEMO_DEFAULT_DURATION_MS
    };
    let default_cadence_ms = if audio {
        CPP_AUDIO_DEFAULT_CADENCE_MS
    } else {
        crate::ui4::GPGPU_PREVIEW_DEFAULT_CADENCE_MS
    };
    let Some(duration_ms) = parse_u64(io, args, default_duration_ms) else {
        return;
    };
    let Some(cadence_ms) = parse_u64(io, args, default_cadence_ms) else {
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
                    "cpp start: queued=1 request={} mode={} service_online={} duration_ms={} cadence_ms={} publish_every={} frontend=cpp-for-opencl backend=intel-igc-aot runtime_compiler=0 artifact={} zebin_sha256={} target=8086:4680-r0C kernel={} simd=16 ui4_window=1 buffering=double plane=slot1-direct maximize=dynamic-frame{} stop=\"cpp stop\"",
                    serial,
                    cpp_mode_label(preset),
                    status.online as u8,
                    duration_ms,
                    cadence_ms,
                    publish_every,
                    artifact_name(preset),
                    artifact_hash(preset),
                    kernel_name(preset),
                    if audio {
                        " pcm=post-mix/pre-hda-s16le-stereo-48k fft=2048-mid-side bands=64 walker=horizontal-pairs/50pct"
                    } else {
                        ""
                    },
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
        crate::ui4::GpgpuPreviewPreset::CppAudio => "audio",
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
        "cpp demo: mode=audio explores=one-composed-instrument/waveform+phase+64-band-spectrum+bass-bloom+beat-rings+particles pcm=exact-pre-hda-tee fft=2048-mid-side walker=50pct",
    );
    print_shell_line(
        io,
        "cpp suite: sources=cpp_demo_rgba8.clcpp+cpp_audio_visualizer_rgba8.clcpp frontend=cpp-for-opencl backend=intel-igc-aot build_time_only=1 exact_target=8086:4680-r0C",
    );
    print_shell_line(
        io,
        "cpp spirit: two ABI-twin C++ artifacts preserve the live Spirit cursor-plane path; use \"cpp spirit list\"",
    );
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    let active_cpp = status.desired_running && is_cpp_preset(status.config.preset);
    let audio = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppAudio;
    let upload = if audio {
        crate::intel::gpgpu::cpp_audio_visualizer_rgba8_upload_status()
    } else {
        crate::intel::gpgpu::cpp_demo_rgba8_upload_status()
    };
    let audio_status = crate::aud::audio_visualizer::status();
    print_shell_line(
        io,
        alloc::format!(
            "cpp status: active={} online={} phase={} request={} applied={} mode={} frame={} window={} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} marker=0x{:08X} submit_ms={} artifact={} resident={} verified={} gpu=0x{:X} zebin_sha256={} runtime_compiler=0 maximize=dynamic-frame pcm_tap={} pcm_sequence={} pcm_frames={} signal={} rms={:.4} peak={:.4} low={:.3} mid={:.3} high={:.3} beat={:.3} error={}",
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
            artifact_name(status.config.preset),
            upload.is_some() as u8,
            upload.is_some_and(|artifact| artifact.verified) as u8,
            upload.map(|artifact| artifact.gpu).unwrap_or(0),
            artifact_hash(status.config.preset),
            audio_status.enabled as u8,
            audio_status.sequence,
            audio_status.captured_frames,
            audio_status.active as u8,
            audio_status.rms,
            audio_status.peak,
            audio_status.low,
            audio_status.mid,
            audio_status.high,
            audio_status.beat,
            status.last_error,
        )
        .as_str(),
    );
}

fn artifact_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
        crate::intel::gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.name
    } else {
        crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.name
    }
}

fn kernel_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    artifact_name(preset)
}

fn artifact_hash(preset: crate::ui4::GpgpuPreviewPreset) -> String {
    if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
        format_hash(crate::intel::gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.bin_sha256)
    } else {
        format_hash(crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.bin_sha256)
    }
}

fn format_hash(hash: [u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in hash {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn print_spirit_list(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "cpp spirit backgrounds: 0=transparent 2=energy-ring 3=magic-circle 4=nebula-smoke 5=cyber-grid 6=portal-vortex 7=speed-lines 8=bokeh-field 9=water-ripples 10=pixel-burst 11=magic-time-circle",
    );
    print_shell_line(
        io,
        "cpp spirit shaders 0-7: 0=original-clean 1=aura-bloom 2=neon-edge 3=fire-rim 4=ice-shimmer 5=hologram 6=rgb-glitch 7=dissolve",
    );
    print_shell_line(
        io,
        "cpp spirit shaders 8-15: 8=ghost-trail 9=electric-arc 10=rainbow-prism 11=hit-flash 12=pixel-wave 13=toon-ink 14=liquid-warp 15=dream-bloom",
    );
    print_shell_line(
        io,
        "cpp spirit examples: \"cpp spirit show 3 9\" = magic-circle + electric-arc; \"cpp spirit show 11 1\" = UTC magic-time-circle + aura-bloom",
    );
}

fn print_spirit_status(io: &'static dyn ShellBackend2) {
    let (revision, panel) = crate::spirit::spirit_vfx::control_panel_snapshot();
    let background_upload = crate::intel::gpgpu::spirit_vfx_background_rgba8_upload_status();
    let sprite_upload = crate::intel::gpgpu::spirit_vfx_sprite_rgba8_upload_status();
    print_shell_line(
        io,
        alloc::format!(
            "cpp spirit status: revision={} background={}({}) shader={}({}) frontend=cpp-for-opencl backend=intel-igc-aot runtime_compiler=0 target=8086:4680-r0C background_resident={} background_verified={} background_gpu=0x{:X} background_sha256={} sprite_resident={} sprite_verified={} sprite_gpu=0x{:X} sprite_sha256={} walkers=clean:1/effect:2 presentation=spirit-cursor-plane",
            revision,
            panel.alpha_background.effect as u8,
            panel.alpha_background.effect.ui_name(),
            panel.sprite_shader.effect as u8,
            panel.sprite_shader.effect.ui_name(),
            background_upload.is_some() as u8,
            background_upload.is_some_and(|artifact| artifact.verified) as u8,
            background_upload.map(|artifact| artifact.gpu).unwrap_or(0),
            format_hash(
                crate::intel::gpgpu::SPIRIT_VFX_BACKGROUND_RGBA8_ADLS_ARTIFACT
                    .bin_sha256,
            ),
            sprite_upload.is_some() as u8,
            sprite_upload.is_some_and(|artifact| artifact.verified) as u8,
            sprite_upload.map(|artifact| artifact.gpu).unwrap_or(0),
            format_hash(
                crate::intel::gpgpu::SPIRIT_VFX_SPRITE_RGBA8_ADLS_ARTIFACT
                    .bin_sha256,
            ),
        )
        .as_str(),
    );
}

fn select_spirit(io: &'static dyn ShellBackend2, background_id: u8, shader_id: u8) {
    match crate::spirit::spirit_vfx::select_cpp_repass(background_id, shader_id) {
        Ok(revision) => {
            let (_, panel) = crate::spirit::spirit_vfx::control_panel_snapshot();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp spirit show: applied=1 revision={} background={}({}) shader={}({}) params=authored-defaults colors=authored-palette frontend=cpp-for-opencl runtime_compiler=0 presentation=live-spirit",
                    revision,
                    panel.alpha_background.effect as u8,
                    panel.alpha_background.effect.ui_name(),
                    panel.sprite_shader.effect as u8,
                    panel.sprite_shader.effect.ui_name(),
                )
                .as_str(),
            );
        }
        Err(reason) => print_shell_line(
            io,
            alloc::format!(
                "cpp spirit show: applied=0 background={} shader={} reason={reason:?}",
                background_id,
                shader_id,
            )
            .as_str(),
        ),
    }
}

fn parse_spirit_ids(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(background) = args.next().and_then(|raw| raw.parse::<u8>().ok()) else {
        usage(io);
        return;
    };
    let Some(shader) = args.next().and_then(|raw| raw.parse::<u8>().ok()) else {
        usage(io);
        return;
    };
    if expect_no_more(io, args) {
        select_spirit(io, background, shader);
    }
}

fn spirit(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(command) = args.next() else {
        select_spirit(io, 3, 9);
        return;
    };
    if command.eq_ignore_ascii_case("list") {
        if expect_no_more(io, args) {
            print_spirit_list(io);
        }
    } else if command.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_spirit_status(io);
        }
    } else if command.eq_ignore_ascii_case("clean") {
        if expect_no_more(io, args) {
            let revision = crate::spirit::spirit_vfx::reset_cpp_repass();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp spirit clean: applied=1 revision={} background=0 shader=0",
                    revision,
                )
                .as_str(),
            );
        }
    } else if command.eq_ignore_ascii_case("show") {
        parse_spirit_ids(io, args);
    } else if let Ok(background) = command.parse::<u8>() {
        let Some(shader) = args.next().and_then(|raw| raw.parse::<u8>().ok()) else {
            usage(io);
            return;
        };
        if expect_no_more(io, args) {
            select_spirit(io, background, shader);
        }
    } else {
        usage(io);
    }
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
    } else if command.eq_ignore_ascii_case("spirit") {
        spirit(io, args);
    } else if let Some(preset) = parse_mode(command) {
        start(io, preset, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
