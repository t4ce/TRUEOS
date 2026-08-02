use alloc::{collections::VecDeque, string::String, vec::Vec};
use core::fmt::Write;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicUsize, Ordering};

use embassy_executor::Spawner;
use spin::Mutex;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line,
};
use crate::intel::gpu_font::{GpuFontFace, GpuFontRgba};
use crate::r::font_kernel_service::{
    FONT_STAMP_MAX_EXTENT, FONT_STAMP_MAX_GLYPHS, FontStampFit, FontStampLayer, FontStampRequest,
    FontStampedBuffer, RetainSceneRequest, RetainedFontPositioning, RetainedFontRun,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const CPP_DEMO_DEFAULT_DURATION_MS: u64 = 30_000;
const CPP_AUDIO_DEFAULT_DURATION_MS: u64 = 0;
const CPP_AUDIO_DEFAULT_CADENCE_MS: u64 = 50;
const CPP_FONT_OUTPUT_CAPACITY: usize = 8;
const CPP_FONT_DEFAULT_PIXELS: f32 = 36.0;
const CPP_FONT_DEFAULT_LINE_HEIGHT: f32 = 1.25;
const CPP_FONT_DEFAULT_RGBA: GpuFontRgba = GpuFontRgba::new(80, 225, 255, 255);
static CPP_FONT_OUTPUTS: Mutex<VecDeque<FontStampedBuffer>> = Mutex::new(VecDeque::new());
static CPP_FONT_OUTPUT_RESERVATIONS: AtomicUsize = AtomicUsize::new(0);

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "cpp [gallery|aurora|julia|sdf|voronoi|retro-sun|audio|particle|static30]",
    );
    print_shell_line(
        io,
        "cpp start [gallery|aurora|julia|sdf|voronoi|retro-sun|audio|particle|static30] [duration_ms] [cadence_ms] [publish_every]",
    );
    print_shell_line(io, "cpp list");
    print_shell_line(io, "cpp status");
    print_shell_line(io, "cpp stop");
    print_shell_line(
        io,
        "cpp font stamp \"text\" [size=36] [font=auto|1|2|3] [color=RRGGBBAA] [x=0] [y=0] [line=1.25] [slant=-1..1] [canvas=WIDTHxHEIGHT] [-- \"overlay\" ...]",
    );
    print_shell_line(
        io,
        "cpp font present \"text\" [size=36] [font=auto|1|2|3] [color=RRGGBBAA] [x=24] [y=24] [line=1.25] [slant=-1..1] [canvas=640x160] [-- \"overlay\" ...]",
    );
    print_shell_line(io, "cpp font rush [start|stop]");
    print_shell_line(io, "cpp font [status|release <ticket|all>]");
    print_shell_line(io, "cpp spirit [status|list|clean]");
    print_shell_line(io, "cpp spirit show <background_id> <shader_id>");
    print_shell_line(io, "cpp svg start [basic|curves|holes]");
    print_shell_line(io, "cpp svg status");
    print_shell_line(io, "cpp svg stop");
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
    } else if raw.eq_ignore_ascii_case("retro")
        || raw.eq_ignore_ascii_case("sun")
        || raw.eq_ignore_ascii_case("retro-sun")
        || raw.eq_ignore_ascii_case("retrosun")
    {
        Some(crate::ui4::GpgpuPreviewPreset::CppRetroSun)
    } else if raw.eq_ignore_ascii_case("audio")
        || raw.eq_ignore_ascii_case("av")
        || raw.eq_ignore_ascii_case("visualizer")
    {
        Some(crate::ui4::GpgpuPreviewPreset::CppAudio)
    } else if raw.eq_ignore_ascii_case("particle")
        || raw.eq_ignore_ascii_case("particles")
        || raw.eq_ignore_ascii_case("arc-forge")
        || raw.eq_ignore_ascii_case("particle-craft")
    {
        Some(crate::ui4::GpgpuPreviewPreset::CppParticle)
    } else if raw.eq_ignore_ascii_case("static30") {
        Some(crate::ui4::GpgpuPreviewPreset::Static30)
    } else {
        None
    }
}

const fn is_cpp_preset(preset: crate::ui4::GpgpuPreviewPreset) -> bool {
    preset.is_cpp() || matches!(preset, crate::ui4::GpgpuPreviewPreset::Static30)
}

fn parse_svg_demo(raw: &str) -> Option<crate::intel::gpgpu::SvgOutlineProbeDemo> {
    if raw.eq_ignore_ascii_case("basic") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Basic)
    } else if raw.eq_ignore_ascii_case("curves") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Curves)
    } else if raw.eq_ignore_ascii_case("holes") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Holes)
    } else {
        None
    }
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn print_svg_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_svg_probe_status();
    print_shell_line(
        io,
        alloc::format!(
            "cpp svg status: online={} phase={} desired_running={} request={} applied={} demo={} frame={} window={} extent={}x{} attempted={} submitted={} published={} layers={} ops={} nonzero_pixels={} submit_ms={} buffering=double plane=universal-1 engine_ready_boundary=surflive error={}",
            status.online as u8,
            status.phase.label(),
            status.desired_running as u8,
            status.request_serial,
            status.applied_serial,
            status.config.demo.label(),
            status.frame.map(|frame| frame.raw()).unwrap_or(0),
            status.window.map(|window| window.raw()).unwrap_or(0),
            status.width,
            status.height,
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.published,
            status.metrics.layers,
            status.metrics.ops,
            status.metrics.nonzero_pixels,
            status.metrics.submit_ms,
            status.last_error,
        )
        .as_str(),
    );
}

fn svg(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(action) = args.next() else {
        usage(io);
        return;
    };

    if action.eq_ignore_ascii_case("start") {
        let demo = match args.next() {
            Some(raw) => match parse_svg_demo(raw) {
                Some(demo) => demo,
                None => {
                    usage(io);
                    return;
                }
            },
            None => crate::intel::gpgpu::SvgOutlineProbeDemo::Basic,
        };
        if !expect_no_more(io, args) {
            return;
        }

        let config = crate::ui4::GpgpuSvgProbeConfig { demo };
        match crate::ui4::request_gpgpu_svg_probe_start(config) {
            Ok(serial) => {
                let status = crate::ui4::gpgpu_svg_probe_status();
                print_shell_line(
                    io,
                    alloc::format!(
                        "cpp svg start: queued=1 request={} demo={} service_online={} ui4_consumer=retained-svg-outline frames=1 windows=1 buffering=double plane=universal-1 interaction=movable-fixed-size",
                        serial,
                        demo.label(),
                        status.online as u8,
                    )
                    .as_str(),
                );
            }
            Err(reason) => print_shell_line(
                io,
                alloc::format!("cpp svg start: queued=0 reason={reason}").as_str(),
            ),
        }
    } else if action.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_svg_status(io);
        }
    } else if action.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, args) {
            let serial = crate::ui4::request_gpgpu_svg_probe_stop();
            print_shell_line(
                io,
                alloc::format!("cpp svg stop: queued=1 request={serial}").as_str(),
            );
        }
    } else {
        usage(io);
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
    let particle = preset == crate::ui4::GpgpuPreviewPreset::CppParticle;
    let static30 = preset == crate::ui4::GpgpuPreviewPreset::Static30;
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
    let detail = if static30 {
        String::from(
            " windows=30 layout=6x5 plane_slots=1+2+3/10-each buffering=immutable-single publish_passes=1",
        )
    } else if audio {
        String::from(
            " pcm=post-mix/pre-hda-s16le-stereo-48k fft=2048-mid-side bands=64 walker=horizontal-pairs/50pct",
        )
    } else if particle {
        particle_work_detail(
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
        )
    } else {
        String::new()
    };
    match crate::ui4::request_gpgpu_preview_start(config) {
        Ok(serial) => {
            let status = crate::ui4::gpgpu_preview_status();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp start: queued=1 request={} mode={} service_online={} duration_ms={} cadence_ms={} publish_every={} frontend=cpp-for-opencl backend=intel-igc-aot runtime_compiler=0 artifact={} zebin_sha256={} target=8086:4680-r0C kernel={} simd=16 ui4_windows={} buffering={} plane={} maximize={}{} stop=\"cpp stop\"",
                    serial,
                    cpp_mode_label(preset),
                    status.online as u8,
                    duration_ms,
                    cadence_ms,
                    publish_every,
                    artifact_name(preset),
                    artifact_hash(preset),
                    kernel_name(preset),
                    if static30 { 30 } else { 1 },
                    if static30 { "single" } else { "double" },
                    if static30 {
                        "slots1+2+3/10-each"
                    } else {
                        "slot1-direct"
                    },
                    if static30 {
                        "movable-fixed-canvas"
                    } else {
                        "dynamic-frame/reconciled"
                    },
                    detail,
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
        crate::ui4::GpgpuPreviewPreset::CppRetroSun => "retro-sun",
        crate::ui4::GpgpuPreviewPreset::CppAudio => "audio",
        crate::ui4::GpgpuPreviewPreset::CppParticle => "particle",
        crate::ui4::GpgpuPreviewPreset::CppFont => "font",
        crate::ui4::GpgpuPreviewPreset::CppFontRush => "font-rush",
        crate::ui4::GpgpuPreviewPreset::Static30 => "static30",
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
        "cpp demo: mode=retro-sun id=5 standalone=1 gallery=0 explores=layered-synthwave/sun-cutout-bands/ocean-reflection/stars/CRT-post",
    );
    print_shell_line(
        io,
        "cpp demo: mode=audio explores=one-composed-instrument/waveform+phase+64-band-spectrum+bass-bloom+beat-rings+particles pcm=exact-pre-hda-tee fft=2048-mid-side walker=50pct",
    );
    print_shell_line(io, particle_list_detail().as_str());
    print_shell_line(
        io,
        "cpp demo: mode=static30 explores=font-kernel-service/30-retained-ui4-windows/three-plane-slots/immutable-single-publish path=skrifa->gpu-vm-r8->cpp-font-instance->ui4-frame",
    );
    print_shell_line(
        io,
        "cpp font rush: staged FontKernel plane probe; adds one layer every 3 seconds, runs 1000/500/250 ms update passes, cycles fonts 1/2/3 every minute, and stops with \"cpp font rush stop\"",
    );
    print_shell_line(
        io,
        "cpp suite: sources=cpp_demo_rgba8.clcpp+cpp_audio_visualizer_rgba8.clcpp+particle_craft.clcpp frontend=cpp-for-opencl backend=intel-igc-aot build_time_only=1 exact_target=8086:4680-r0C",
    );
    print_shell_line(
        io,
        "cpp spirit: two native C++ artifacts drive the live Spirit cursor-plane path; use \"cpp spirit list\"",
    );
}

fn particle_naive_candidate_tests(sample_width: u32, sample_height: u32) -> u64 {
    u64::from(sample_width)
        * u64::from(sample_height)
        * u64::from(crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES)
}

fn particle_work_detail(destination_width: u32, destination_height: u32) -> String {
    let (sample_width, sample_height) =
        crate::intel::gpgpu::particle_craft_sample_extent(destination_width, destination_height);
    let render_divisor =
        crate::intel::gpgpu::particle_craft_render_divisor(destination_width, destination_height);
    let (tile_columns, tile_rows) =
        crate::intel::gpgpu::particle_craft_tile_extent(destination_width, destination_height);
    alloc::format!(
        " preset=arc-forge particles={} state=8KiB bins={}B params=v1/64B passes=step+tile-bin+pixel-gather backing={}x{} samples={}x{} tiles={}x{} tile={}x{} mask={}b render_divisor={} presentation=dynamic-1:1-or-direct-plane-2x naive_tests={} bin_tests={} gather=tile-mask",
        crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        crate::intel::gpgpu::PARTICLE_CRAFT_TILE_MASK_BYTES,
        destination_width,
        destination_height,
        sample_width,
        sample_height,
        tile_columns,
        tile_rows,
        crate::intel::gpgpu::PARTICLE_CRAFT_TILE_SAMPLE_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_TILE_SAMPLE_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_TILE_MASK_WORDS * u32::BITS,
        render_divisor,
        particle_naive_candidate_tests(sample_width, sample_height),
        crate::intel::gpgpu::particle_craft_bin_candidate_tests(
            destination_width,
            destination_height,
            crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        ),
    )
}

fn particle_list_detail() -> String {
    alloc::format!(
        "cpp demo: mode=particle preset=arc-forge explores=persistent-state/three-pass-dependency/tile-binned-gather/soft-cores+velocity-tails+pointer-attraction particles={} native_extent={}x{} native_samples={}x{} render_divisor={} maximize=half-scanout-backing/direct-plane-2x native_naive_tests={} native_bin_tests={} resizable=1",
        crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_RENDER_DIVISOR,
        particle_naive_candidate_tests(
            crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_WIDTH,
            crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_HEIGHT,
        ),
        crate::intel::gpgpu::particle_craft_bin_candidate_tests(
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
            crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        ),
    )
}

fn font_rush_status_detail(
    members: impl IntoIterator<
        Item = (u8, bool, Option<u64>, Option<u32>, crate::ui4::GpgpuPreviewMetrics),
    >,
) -> String {
    let mut detail = String::from(" rush_slots=");
    let mut active = 0usize;
    for (plane_slot, is_active, frame, window, metrics) in members {
        let (Some(frame), Some(window)) = (frame, window) else {
            continue;
        };
        if active != 0 {
            detail.push(',');
        }
        let _ = write!(
            detail,
            "{}:active{}:frame{}:window{}:attempted{}:submitted{}:completed{}:published{}:scanout_live{}:scanout_superseded{}:drop_frame{}:drop_queue{}:drop_inflight{}:drop_cadence{}:late{}:font_wait_ms{}",
            plane_slot,
            is_active as u8,
            frame,
            window,
            metrics.attempted,
            metrics.submitted,
            metrics.completed,
            metrics.published,
            metrics.scanout_live,
            metrics.scanout_superseded,
            metrics.dropped_frame_busy,
            metrics.dropped_queue_full,
            metrics.dropped_in_flight,
            metrics.dropped_cadence,
            metrics.late,
            metrics.last_submit_ms,
        );
        active = active.saturating_add(usize::from(is_active));
    }
    if active == 0 {
        detail.push_str("none");
    }
    let _ = write!(detail, " rush_active_planes={active}");
    detail
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    let active_cpp = status.desired_running && is_cpp_preset(status.config.preset);
    let audio = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppAudio;
    let particle = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppParticle;
    let font_rush = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppFontRush;
    let font = matches!(
        status.config.preset,
        crate::ui4::GpgpuPreviewPreset::CppFont
            | crate::ui4::GpgpuPreviewPreset::CppFontRush
            | crate::ui4::GpgpuPreviewPreset::Static30
    );
    let upload = if font {
        crate::intel::gpgpu::font_instance_rgba8_upload_status()
    } else if audio {
        crate::intel::gpgpu::cpp_audio_visualizer_rgba8_upload_status()
    } else if particle {
        crate::intel::gpgpu::particle_craft_upload_status()
    } else {
        crate::intel::gpgpu::cpp_demo_rgba8_upload_status()
    };
    let audio_status = crate::aud::audio_visualizer::status();
    let mode_detail = if particle {
        let destination_width = if status.width == 0 {
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH
        } else {
            status.width
        };
        let destination_height = if status.height == 0 {
            crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT
        } else {
            status.height
        };
        particle_work_detail(destination_width, destination_height)
    } else if font_rush {
        font_rush_status_detail(
            status
                .members
                .iter()
                .filter(|member| member.preset == crate::ui4::GpgpuPreviewPreset::CppFontRush)
                .map(|member| {
                    (
                        member.plane_slot,
                        member.active,
                        member.frame.map(|frame| frame.raw()),
                        member.window.map(|window| window.raw()),
                        member.metrics,
                    )
                }),
        )
    } else {
        String::new()
    };
    let timing_label = if font_rush {
        "font_wait_ms"
    } else {
        "submit_ms"
    };
    print_shell_line(
        io,
        alloc::format!(
            "cpp status: active={} online={} phase={} request={} applied={} mode={} frame={} window={} extent={}x{} attempted={} submitted={} completed={} published={} scanout_live={} scanout_superseded={} dropped_busy={} dropped_frame_busy={} dropped_queue_full={} dropped_in_flight={} dropped_cadence={} failed={} late={} elapsed_ms={} marker=0x{:08X} {}={} artifact={} resident={} verified={} gpu=0x{:X} zebin_sha256={} runtime_compiler=0 maximize={} pcm_tap={} pcm_sequence={} pcm_frames={} signal={} rms={:.4} peak={:.4} low={:.3} mid={:.3} high={:.3} beat={:.3} error={}{}",
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
            status.width,
            status.height,
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.completed,
            status.metrics.published,
            status.metrics.scanout_live,
            status.metrics.scanout_superseded,
            status.metrics.dropped_busy,
            status.metrics.dropped_frame_busy,
            status.metrics.dropped_queue_full,
            status.metrics.dropped_in_flight,
            status.metrics.dropped_cadence,
            status.metrics.failed,
            status.metrics.late,
            status.metrics.elapsed_ms,
            status.metrics.last_marker,
            timing_label,
            status.metrics.last_submit_ms,
            artifact_name(status.config.preset),
            upload.is_some() as u8,
            upload.is_some_and(|artifact| artifact.verified) as u8,
            upload.map(|artifact| artifact.gpu).unwrap_or(0),
            artifact_hash(status.config.preset),
            if font_rush {
                "fullscreen-layered/capability-bounded"
            } else if font {
                "movable-fixed-canvas"
            } else {
                "dynamic-frame/reconciled"
            },
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
            mode_detail,
        )
        .as_str(),
    );
}

fn artifact_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    if matches!(
        preset,
        crate::ui4::GpgpuPreviewPreset::CppFont
            | crate::ui4::GpgpuPreviewPreset::CppFontRush
            | crate::ui4::GpgpuPreviewPreset::Static30
    ) {
        crate::intel::gpgpu::FONT_INSTANCE_RGBA8_ADLS_ARTIFACT.name
    } else if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
        crate::intel::gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.name
    } else if preset == crate::ui4::GpgpuPreviewPreset::CppParticle {
        crate::intel::gpgpu::PARTICLE_CRAFT_ADLS_ARTIFACT.name
    } else {
        crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.name
    }
}

fn kernel_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    if preset == crate::ui4::GpgpuPreviewPreset::CppParticle {
        "particle_craft_step+particle_craft_bin_tiles+particle_craft_render_rgba8"
    } else {
        artifact_name(preset)
    }
}

fn artifact_hash(preset: crate::ui4::GpgpuPreviewPreset) -> String {
    if matches!(
        preset,
        crate::ui4::GpgpuPreviewPreset::CppFont
            | crate::ui4::GpgpuPreviewPreset::CppFontRush
            | crate::ui4::GpgpuPreviewPreset::Static30
    ) {
        format_hash(crate::intel::gpgpu::FONT_INSTANCE_RGBA8_ADLS_ARTIFACT.bin_sha256)
    } else if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
        format_hash(crate::intel::gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.bin_sha256)
    } else if preset == crate::ui4::GpgpuPreviewPreset::CppParticle {
        format_hash(crate::intel::gpgpu::PARTICLE_CRAFT_ADLS_ARTIFACT.bin_sha256)
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

fn print_font_service_status(io: &'static dyn ShellBackend2) {
    let status = crate::r::font_kernel_service::status();
    let (outputs, output_bytes) = {
        let outputs = CPP_FONT_OUTPUTS.lock();
        (
            outputs.len(),
            outputs
                .iter()
                .fold(0usize, |total, output| total.saturating_add(output.surface().bytes)),
        )
    };
    print_shell_line(
        io,
        alloc::format!(
            "cpp font status: online={} queued={} outputs={} output_reservations={} output_bytes={} output_capacity={} active_ticket={} active_stage={} active_consumer={}:{} lane_waiters={} lane_peak={} lane_admissions={} lane_contentions={} lane_wait_ms={} lane_wait_max_ms={} lane_paths=retain:{},stamp:{} lane_retries={} gpu_retries={} retain_submitted={} retain_completed={} stamp_submitted={} stamp_completed={} failed={} caps=rgba8-{}px/uhd-pixels+{}glyphs carrier=bsp-controller+leased-blocking-lane gpu_lane=fair-fifo-font-only ownership=gpu-vm-r8+gpu-vm-rgba8 completion=ticket-signal",
            status.online as u8,
            status.queued,
            outputs,
            CPP_FONT_OUTPUT_RESERVATIONS.load(Ordering::Acquire),
            output_bytes,
            CPP_FONT_OUTPUT_CAPACITY,
            status.active_ticket.map(|ticket| ticket.raw()).unwrap_or(0),
            status.active_stage,
            status
                .active_consumer
                .map(|consumer| consumer.path.name())
                .unwrap_or("none"),
            status
                .active_consumer
                .map(|consumer| consumer.id)
                .unwrap_or(0),
            status.lane_waiters,
            status.lane_peak_waiters,
            status.lane_admissions,
            status.lane_contentions,
            status.lane_wait_ms,
            status.lane_wait_max_ms,
            status.retain_lane_admissions,
            status.stamp_lane_admissions,
            status.lane_retries,
            status.gpu_retries,
            status.submitted_retain,
            status.completed_retain,
            status.submitted_stamp,
            status.completed_stamp,
            status.failed,
            FONT_STAMP_MAX_EXTENT,
            FONT_STAMP_MAX_GLYPHS,
        )
        .as_str(),
    );
}

fn reserve_font_output() -> bool {
    let mut reserved = CPP_FONT_OUTPUT_RESERVATIONS.load(Ordering::Acquire);
    loop {
        if reserved >= CPP_FONT_OUTPUT_CAPACITY {
            return false;
        }
        match CPP_FONT_OUTPUT_RESERVATIONS.compare_exchange_weak(
            reserved,
            reserved + 1,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => return true,
            Err(observed) => reserved = observed,
        }
    }
}

fn release_font_output_reservation(count: usize) {
    if count != 0 {
        CPP_FONT_OUTPUT_RESERVATIONS.fetch_sub(count, Ordering::AcqRel);
    }
}

/// Transfer one completed shell stamp to an in-kernel consumer.
///
/// The returned allocation remains valid until that consumer drops it. This is
/// the programmatic counterpart to the stable handle printed by the command.
pub(crate) fn take_font_stamp_output(handle: u64) -> Option<FontStampedBuffer> {
    let mut outputs = CPP_FONT_OUTPUTS.lock();
    let index = outputs
        .iter()
        .position(|output| output.ticket().raw() == handle)?;
    let output = outputs.remove(index)?;
    drop(outputs);
    release_font_output_reservation(1);
    Some(output)
}

#[derive(Debug)]
struct ParsedFontStamp {
    request: FontStampRequest,
    glyphs: usize,
    rows: usize,
}

fn tokenize_font_stamp(input: &str) -> Result<Vec<String>, &'static str> {
    let mut tokens = Vec::new();
    let mut chars = input.chars().peekable();
    while chars.peek().is_some() {
        while chars.peek().is_some_and(|ch| ch.is_whitespace()) {
            chars.next();
        }
        let Some(first) = chars.next() else {
            break;
        };
        let mut token = String::new();
        if first == '"' {
            let mut closed = false;
            while let Some(ch) = chars.next() {
                match ch {
                    '"' => {
                        closed = true;
                        break;
                    }
                    '\\' => {
                        let escaped = chars.next().ok_or("unfinished-escape")?;
                        match escaped {
                            '"' | '\\' => token.push(escaped),
                            'n' => token.push('\n'),
                            'r' => token.push('\r'),
                            't' => token.push('\t'),
                            'u' => {
                                if chars.next() != Some('{') {
                                    return Err("unicode-escape-open-brace");
                                }
                                let mut value = 0u32;
                                let mut digits = 0usize;
                                loop {
                                    let scalar = chars.next().ok_or("unfinished-unicode-escape")?;
                                    if scalar == '}' {
                                        break;
                                    }
                                    let digit = scalar.to_digit(16).ok_or("unicode-escape-hex")?;
                                    digits = digits.saturating_add(1);
                                    if digits > 6 {
                                        return Err("unicode-escape-too-long");
                                    }
                                    value = value
                                        .checked_mul(16)
                                        .and_then(|value| value.checked_add(digit))
                                        .ok_or("unicode-escape-range")?;
                                }
                                if digits == 0 {
                                    return Err("unicode-escape-empty");
                                }
                                token.push(
                                    char::from_u32(value).ok_or("unicode-escape-invalid-scalar")?,
                                );
                            }
                            _ => return Err("unsupported-escape"),
                        }
                    }
                    ch => token.push(ch),
                }
            }
            if !closed {
                return Err("missing-closing-quote");
            }
            if chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                return Err("quoted-token-must-end-at-whitespace");
            }
        } else {
            token.push(first);
            while chars.peek().is_some_and(|ch| !ch.is_whitespace()) {
                token.push(chars.next().expect("peeked font token character"));
            }
        }
        tokens.push(token);
    }
    Ok(tokens)
}

fn parse_stamp_rgba(encoded: &str) -> Result<GpuFontRgba, &'static str> {
    let encoded = encoded
        .strip_prefix('#')
        .or_else(|| encoded.strip_prefix("0x"))
        .unwrap_or(encoded);
    if encoded.len() != 8 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("color-expected-RRGGBBAA");
    }
    u32::from_str_radix(encoded, 16)
        .map(GpuFontRgba::from_rgba_u32)
        .map_err(|_| "color-invalid")
}

fn parse_stamp_canvas(encoded: &str) -> Result<(u32, u32), &'static str> {
    let (width, height) = encoded
        .split_once(['x', 'X'])
        .ok_or("canvas-expected-WIDTHxHEIGHT")?;
    let width = width.parse::<u32>().map_err(|_| "canvas-width-invalid")?;
    let height = height.parse::<u32>().map_err(|_| "canvas-height-invalid")?;
    if width == 0
        || height == 0
        || width > FONT_STAMP_MAX_EXTENT
        || height > FONT_STAMP_MAX_EXTENT
        || u64::from(width) * u64::from(height)
            > crate::r::font_kernel_service::FONT_STAMP_MAX_PIXELS
    {
        return Err("canvas-over-4k-softcap");
    }
    Ok((width, height))
}

fn font_prefers_noto(text: &str) -> bool {
    text.chars().any(|ch| {
        matches!(
            ch as u32,
            0x2E80..=0xA4CF
                | 0xF900..=0xFAFF
                | 0xFE30..=0xFE4F
                | 0xFF00..=0xFFEF
                | 0x20000..=0x2FA1F
        )
    })
}

fn parse_font_stamp(input: &str, present: bool) -> Result<ParsedFontStamp, &'static str> {
    let tokens = tokenize_font_stamp(input)?;
    if tokens.is_empty() {
        return Err("text-must-be-quoted");
    }
    let mut layers: Vec<(Vec<RetainedFontRun>, GpuFontFace, GpuFontRgba)> = Vec::new();
    let mut cursor = 0usize;
    let mut canvas = None;
    let mut glyphs = 0usize;
    let mut rows = 0usize;
    while cursor < tokens.len() {
        if tokens[cursor] == "--" || tokens[cursor].contains('=') {
            return Err("overlay-text-missing");
        }
        let text = &tokens[cursor];
        cursor += 1;
        let mut font = None;
        let mut font_pixels = CPP_FONT_DEFAULT_PIXELS;
        let mut foreground = CPP_FONT_DEFAULT_RGBA;
        let mut x = if present { 24.0 } else { 0.0 };
        let mut y = if present { 24.0 } else { 0.0 };
        let mut line_height = CPP_FONT_DEFAULT_LINE_HEIGHT;
        let mut slant = 0.0f32;
        while cursor < tokens.len() && tokens[cursor] != "--" {
            let option = &tokens[cursor];
            cursor += 1;
            let (key, value) = option.split_once('=').ok_or("option-expected-key=value")?;
            match key {
                "font" => {
                    if value.eq_ignore_ascii_case("auto") {
                        font = None;
                    } else {
                        let id = value.parse::<u32>().map_err(|_| "font-id-invalid")?;
                        font = Some(GpuFontFace::from_id(id).ok_or("font-id-out-of-range-1-to-3")?);
                    }
                }
                "size" => {
                    font_pixels = value.parse::<f32>().map_err(|_| "size-invalid")?;
                    if !font_pixels.is_finite() || !(4.0..=2048.0).contains(&font_pixels) {
                        return Err("size-out-of-range-4-to-2048");
                    }
                }
                "color" => foreground = parse_stamp_rgba(value)?,
                "x" => x = value.parse::<f32>().map_err(|_| "x-invalid")?,
                "y" => y = value.parse::<f32>().map_err(|_| "y-invalid")?,
                "line" => {
                    line_height = value.parse::<f32>().map_err(|_| "line-invalid")?;
                    if !line_height.is_finite() || !(0.5..=4.0).contains(&line_height) {
                        return Err("line-out-of-range-0.5-to-4");
                    }
                }
                "slant" => {
                    slant = value.parse::<f32>().map_err(|_| "slant-invalid")?;
                    if !slant.is_finite() || !(-1.0..=1.0).contains(&slant) {
                        return Err("slant-out-of-range-minus1-to-1");
                    }
                }
                "canvas" => {
                    let parsed = parse_stamp_canvas(value)?;
                    if canvas.is_some_and(|current| current != parsed) {
                        return Err("canvas-conflict");
                    }
                    canvas = Some(parsed);
                }
                _ => return Err("unknown-stamp-option"),
            }
        }
        if !x.is_finite() || !y.is_finite() {
            return Err("position-invalid");
        }
        let layer_glyphs = text.chars().filter(|ch| *ch != '\n' && *ch != '\r').count();
        glyphs = glyphs
            .checked_add(layer_glyphs)
            .ok_or("glyph-softcap-4096")?;
        if layer_glyphs == 0 || glyphs > FONT_STAMP_MAX_GLYPHS {
            return Err("glyph-softcap-4096");
        }
        let font = font.unwrap_or_else(|| {
            if font_prefers_noto(text) {
                GpuFontFace::NotoSansSc
            } else {
                GpuFontFace::Default
            }
        });
        let mut runs = Vec::new();
        for (row, line) in text.split(['\n', '\r']).enumerate() {
            rows = rows.saturating_add(1);
            if line.is_empty() {
                continue;
            }
            runs.push(RetainedFontRun {
                text: String::from(line),
                position: [x, y + row as f32 * font_pixels * line_height],
                font_pixels,
                slant,
            });
        }
        if runs.is_empty() {
            return Err("font-coverage-empty");
        }
        if let Some((previous_runs, previous_font, previous_foreground)) = layers.last_mut()
            && *previous_font == font
            && *previous_foreground == foreground
        {
            previous_runs.extend(runs);
        } else {
            layers.push((runs, font, foreground));
        }
        if cursor < tokens.len() {
            cursor += 1;
            if cursor == tokens.len() {
                return Err("overlay-text-missing");
            }
        }
    }
    let (width, height, fit) = canvas
        .map(|(width, height)| (width, height, FontStampFit::Canvas))
        .unwrap_or_else(|| {
            if present {
                (640, 160, FontStampFit::Canvas)
            } else {
                (3840, 2160, FontStampFit::Tight)
            }
        });
    let layers = layers
        .into_iter()
        .map(|(runs, font, foreground)| FontStampLayer {
            scene: RetainSceneRequest {
                runs,
                font,
                viewport_width: width,
                viewport_height: height,
                raster_width: width,
                raster_height: height,
                positioning: RetainedFontPositioning::SceneOrigin,
            },
            foreground,
        })
        .collect();
    Ok(ParsedFontStamp {
        request: FontStampRequest { layers, fit },
        glyphs,
        rows,
    })
}

fn queue_font_service_stamp(spawner: &Spawner, io: &'static dyn ShellBackend2, input: &str) {
    if !crate::r::font_kernel_service::status().online {
        print_shell_line(io, "cpp font stamp: queued=0 reason=font-service-offline");
        return;
    }
    let parsed = match parse_font_stamp(input, false) {
        Ok(parsed) => parsed,
        Err(reason) => {
            print_shell_line(
                io,
                alloc::format!("cpp font stamp: queued=0 reason={reason}").as_str(),
            );
            usage(io);
            return;
        }
    };
    let layers = parsed.request.layers.len();
    let fit = parsed.request.fit;
    if !reserve_font_output() {
        print_shell_line(io, "cpp font stamp: queued=0 reason=output-capacity");
        return;
    }
    let pending = match crate::r::font_kernel_service::submit_stamp(parsed.request) {
        Ok(pending) => pending,
        Err(error) => {
            release_font_output_reservation(1);
            print_shell_line(
                io,
                alloc::format!("cpp font stamp: queued=0 reason={error:?}").as_str(),
            );
            return;
        }
    };
    let ticket = pending.ticket().raw();
    match cpp_font_stamp_task(matrix_target_for_backend(io), pending) {
        Ok(task) => {
            spawner.spawn(task);
            print_shell_line(
                io,
                alloc::format!(
                    "cpp font stamp: queued=1 ticket={} layers={} rows={} glyphs={} fit={} output=async-owned-rgba8 context=kernel-gpgpu-font path=skrifa->gpu-vm-r8->cpp-igc->guc-font-rcs->gpu-vm-rgba8",
                    ticket,
                    layers,
                    parsed.rows,
                    parsed.glyphs,
                    match fit {
                        FontStampFit::Canvas => "canvas",
                        FontStampFit::Tight => "tight",
                    },
                )
                .as_str(),
            );
        }
        Err(_) => {
            release_font_output_reservation(1);
            print_shell_line(io, "cpp font stamp: queued=0 reason=completion-task-capacity");
        }
    }
}

fn queue_font_service_present(io: &'static dyn ShellBackend2, input: &str) {
    if !crate::r::font_kernel_service::status().online {
        print_shell_line(io, "cpp font present: queued=0 reason=font-service-offline");
        return;
    }
    let parsed = match parse_font_stamp(input, true) {
        Ok(parsed) => parsed,
        Err(reason) => {
            print_shell_line(
                io,
                alloc::format!("cpp font present: queued=0 reason={reason}").as_str(),
            );
            usage(io);
            return;
        }
    };
    let layers = parsed.request.layers.len();
    let extent = parsed.request.layers[0].scene.raster_width;
    let height = parsed.request.layers[0].scene.raster_height;
    match crate::ui4::request_cpp_font_preview_start(parsed.request) {
        Ok(serial) => print_shell_line(
            io,
            alloc::format!(
                "cpp font present: queued=1 request={} layers={} rows={} glyphs={} extent={}x{} output=ui4-font-scene context=kernel-gpgpu-font path=skrifa->gpu-vm-r8->cpp-igc->guc-font-rcs->ui4-rgba8 stop=\"cpp stop\"",
                serial, layers, parsed.rows, parsed.glyphs, extent, height,
            )
            .as_str(),
        ),
        Err(reason) => print_shell_line(
            io,
            alloc::format!("cpp font present: queued=0 reason={reason}").as_str(),
        ),
    }
}

fn queue_font_service_rush(io: &'static dyn ShellBackend2) {
    match crate::ui4::request_cpp_font_rush_start() {
        Ok(serial) => print_shell_line(
            io,
            alloc::format!(
                "cpp font rush: queued=1 request={} fonts=1,2,3 font_cycle_ms=60000 cadence_passes_ms=1000,500,250 cadence_final_ms=250 layer_add_ms=3000 pass_boundary=all-4-layers-scanout-live-for-3000ms glyph_layout=1+2+4+16 planes=ui4-display-capability-enumerated consumers=independent-per-plane consumer_pending_limit=1 service_model=fifo-32+one-font-context-in-flight per_plane_batch=clear+batched-coverage+batched-region-stamp path=gpu-clear->skrifa->gpu-vm-r8->coverage-audit->guc-font-rcs->ui4-rgba8->display-plane-direct compositor_jobs=0 rgba_cpu_readback=0 coverage_audit_cpu_readback=1 duration=until-stopped stop=\"cpp font rush stop\"",
                serial,
            )
            .as_str(),
        ),
        Err(reason) => {
            let usage = crate::ui4::ui4_live_resource_usage();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp font rush: queued=0 reason={} active_frames={} active_sessions={} live_windows={} display_idle={} fully_retired={}",
                    reason,
                    usage.active_frames,
                    usage.active_sessions,
                    usage.live_windows,
                    usage.is_display_idle() as u8,
                    usage.is_fully_retired() as u8,
                )
                .as_str(),
            )
        }
    }
}

fn stop_font_service_rush(io: &'static dyn ShellBackend2) {
    match crate::ui4::request_cpp_font_rush_stop() {
        Ok(serial) => print_shell_line(
            io,
            alloc::format!("cpp font rush stop: queued=1 request={serial}").as_str(),
        ),
        Err(reason) => print_shell_line(
            io,
            alloc::format!("cpp font rush stop: queued=0 reason={reason}").as_str(),
        ),
    }
}

#[embassy_executor::task(pool_size = 32)]
async fn cpp_font_stamp_task(
    output_target: MatrixTarget,
    pending: crate::r::font_kernel_service::PendingFontStamp,
) {
    let ticket = pending.ticket().raw();
    match pending.wait().await {
        Ok(buffer) => {
            let surface = buffer.surface();
            let origin = buffer.origin_px();
            let glyphs = buffer.glyphs();
            let submits = buffer.submits();
            let walkers = buffer.active_walkers();
            let retained = {
                let mut outputs = CPP_FONT_OUTPUTS.lock();
                outputs.push_back(buffer);
                outputs.len()
            };
            print_matrix_target_line(
                &output_target,
                alloc::format!(
                    "cpp font stamp complete: ticket={} ok=1 handle={} gpu=0x{:X} extent={}x{} pitch={} logical_origin={},{} glyphs={} submits={} walkers={} retained_outputs={} rgba=premultiplied-rgba8 alpha=coverage-multiplied source_over=1 cpu_readback=0 runtime_compiler=0",
                    ticket,
                    ticket,
                    surface.gpu,
                    surface.width,
                    surface.height,
                    surface.pitch_bytes,
                    origin[0],
                    origin[1],
                    glyphs,
                    submits,
                    walkers,
                    retained,
                )
                .as_str(),
            );
        }
        Err(error) => {
            release_font_output_reservation(1);
            print_matrix_target_line(
                &output_target,
                alloc::format!("cpp font stamp complete: ticket={} ok=0 reason={error:?}", ticket,)
                    .as_str(),
            );
        }
    }
}

fn release_font_output(io: &'static dyn ShellBackend2, target: &str) {
    if target.eq_ignore_ascii_case("all") {
        let mut outputs = CPP_FONT_OUTPUTS.lock();
        let released = outputs.len();
        outputs.clear();
        drop(outputs);
        release_font_output_reservation(released);
        print_shell_line(
            io,
            alloc::format!("cpp font release: released={} target=all", released).as_str(),
        );
        return;
    }
    let Ok(ticket) = target.parse::<u64>() else {
        print_shell_line(io, "cpp font release: released=0 reason=ticket-invalid");
        return;
    };
    let Some(output) = take_font_stamp_output(ticket) else {
        print_shell_line(io, "cpp font release: released=0 reason=handle-not-found");
        return;
    };
    drop(output);
    print_shell_line(io, alloc::format!("cpp font release: released=1 handle={ticket}").as_str());
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FontRushAction {
    Start,
    Stop,
}

fn parse_font_rush_action(input: &str) -> Option<FontRushAction> {
    let input = input.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("start") {
        Some(FontRushAction::Start)
    } else if input.eq_ignore_ascii_case("stop") {
        Some(FontRushAction::Stop)
    } else {
        None
    }
}

fn font_service(spawner: &Spawner, io: &'static dyn ShellBackend2, input: &str) {
    let input = input.trim();
    let (command, rest) = input
        .split_once(char::is_whitespace)
        .map(|(command, rest)| (command, rest.trim_start()))
        .unwrap_or((input, ""));
    if command.eq_ignore_ascii_case("stamp") {
        queue_font_service_stamp(spawner, io, rest);
    } else if command.eq_ignore_ascii_case("present") {
        queue_font_service_present(io, rest);
    } else if command.eq_ignore_ascii_case("rush") {
        match parse_font_rush_action(rest) {
            Some(FontRushAction::Start) => queue_font_service_rush(io),
            Some(FontRushAction::Stop) => stop_font_service_rush(io),
            None => usage(io),
        }
    } else if command.eq_ignore_ascii_case("status") {
        if rest.is_empty() {
            print_font_service_status(io);
        } else {
            usage(io);
        }
    } else if command.eq_ignore_ascii_case("release") {
        let mut args = rest.split_whitespace();
        match (args.next(), args.next()) {
            (Some(target), None) => release_font_output(io, target),
            _ => usage(io),
        }
    } else {
        usage(io);
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    input: &str,
) -> ParseOutcome {
    let input = input.trim();
    let mut args = input.split_whitespace();
    let Some(command) = args.next() else {
        start(io, crate::ui4::GpgpuPreviewPreset::CppGallery, &mut args);
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
        start(io, preset, &mut args);
    } else if command.eq_ignore_ascii_case("list") {
        if expect_no_more(io, &mut args) {
            print_list(io);
        }
    } else if command.eq_ignore_ascii_case("status") {
        if expect_no_more(io, &mut args) {
            print_status(io);
        }
    } else if command.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, &mut args) {
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
    } else if command.eq_ignore_ascii_case("font") {
        let font_input = input[command.len()..].trim_start();
        font_service(spawner, io, font_input);
    } else if command.eq_ignore_ascii_case("spirit") {
        spirit(io, &mut args);
    } else if command.eq_ignore_ascii_case("svg") {
        svg(io, &mut args);
    } else if let Some(preset) = parse_mode(command) {
        start(io, preset, &mut args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{
        FontRushAction, FontStampFit, font_rush_status_detail, parse_font_rush_action,
        parse_font_stamp, parse_mode, parse_svg_demo, particle_naive_candidate_tests,
    };

    #[test]
    fn retro_sun_aliases_select_the_standalone_preset() {
        for alias in ["retro-sun", "retro", "sun", "retrosun"] {
            assert_eq!(parse_mode(alias), Some(crate::ui4::GpgpuPreviewPreset::CppRetroSun),);
        }
    }

    #[test]
    fn particle_aliases_select_the_stateful_two_pass_preset() {
        for alias in ["particle", "particles", "particle-craft", "arc-forge"] {
            assert_eq!(parse_mode(alias), Some(crate::ui4::GpgpuPreviewPreset::CppParticle),);
        }
    }

    #[test]
    fn static30_selects_the_font_kernel_window_grid_preset() {
        for spelling in ["static30", "STATIC30", "Static30"] {
            assert_eq!(parse_mode(spelling), Some(crate::ui4::GpgpuPreviewPreset::Static30),);
        }
        assert!(super::is_cpp_preset(crate::ui4::GpgpuPreviewPreset::Static30));
        assert_eq!(super::cpp_mode_label(crate::ui4::GpgpuPreviewPreset::Static30), "static30",);
    }

    #[test]
    fn particle_default_reports_the_reduced_candidate_work() {
        assert_eq!(particle_naive_candidate_tests(320, 200), 8_192_000);
    }

    #[test]
    fn retained_svg_demos_remain_available_under_cpp() {
        for demo in ["basic", "curves", "holes"] {
            assert!(parse_svg_demo(demo).is_some());
        }
        assert!(parse_svg_demo("preview").is_none());
    }

    #[test]
    fn font_rush_accepts_only_start_and_targeted_stop() {
        assert_eq!(parse_font_rush_action(""), Some(FontRushAction::Start));
        assert_eq!(parse_font_rush_action("  "), Some(FontRushAction::Start));
        assert_eq!(parse_font_rush_action("start"), Some(FontRushAction::Start));
        assert_eq!(parse_font_rush_action("START"), Some(FontRushAction::Start));
        assert_eq!(parse_font_rush_action("stop"), Some(FontRushAction::Stop));
        assert_eq!(parse_font_rush_action("STOP"), Some(FontRushAction::Stop));
        assert_eq!(parse_font_rush_action("go"), None);
        assert_eq!(parse_font_rush_action("stop now"), None);
    }

    #[test]
    fn font_rush_remains_a_cpp_preset() {
        let preset = crate::ui4::GpgpuPreviewPreset::CppFontRush;
        assert!(super::is_cpp_preset(preset));
        assert_eq!(super::cpp_mode_label(preset), "font-rush");
    }

    #[test]
    fn font_rush_status_reports_every_hardware_plane_consumer() {
        let metrics = crate::ui4::GpgpuPreviewMetrics::default();
        let members = [
            (0, true, Some(10), Some(20), metrics),
            (1, true, Some(11), Some(21), metrics),
            (2, false, Some(12), Some(22), metrics),
            (3, false, Some(13), Some(23), metrics),
        ];

        assert_eq!(
            font_rush_status_detail(members),
            " rush_slots=0:active1:frame10:window20:attempted0:submitted0:completed0:published0:scanout_live0:scanout_superseded0:drop_frame0:drop_queue0:drop_inflight0:drop_cadence0:late0:font_wait_ms0,1:active1:frame11:window21:attempted0:submitted0:completed0:published0:scanout_live0:scanout_superseded0:drop_frame0:drop_queue0:drop_inflight0:drop_cadence0:late0:font_wait_ms0,2:active0:frame12:window22:attempted0:submitted0:completed0:published0:scanout_live0:scanout_superseded0:drop_frame0:drop_queue0:drop_inflight0:drop_cadence0:late0:font_wait_ms0,3:active0:frame13:window23:attempted0:submitted0:completed0:published0:scanout_live0:scanout_superseded0:drop_frame0:drop_queue0:drop_inflight0:drop_cadence0:late0:font_wait_ms0 rush_active_planes=2",
        );
    }

    #[test]
    fn font_stamp_parses_multiline_unicode_and_overlay_layers() {
        let parsed = parse_font_stamp(
            "\"Hello\\n中国\" size=42 color=FF0000CC -- \"overlay\" x=12 y=-4 font=3",
            false,
        )
        .expect("valid layered font stamp");

        assert_eq!(parsed.request.fit, FontStampFit::Tight);
        assert_eq!(parsed.request.layers.len(), 2);
        assert_eq!(parsed.request.layers[0].scene.runs.len(), 2);
        assert_eq!(parsed.request.layers[0].scene.font.id(), 2);
        assert_eq!(parsed.request.layers[1].scene.font.id(), 3);
        assert_eq!(parsed.rows, 3);
    }

    #[test]
    fn font_stamp_canvas_and_glyph_softcaps_are_enforced() {
        let parsed =
            parse_font_stamp("\"canvas\" canvas=3840x2160", false).expect("valid UHD canvas stamp");
        assert_eq!(parsed.request.fit, FontStampFit::Canvas);

        let oversized = alloc::format!("\"{}\"", "x".repeat(4097));
        assert_eq!(parse_font_stamp(oversized.as_str(), false).unwrap_err(), "glyph-softcap-4096");
        assert_eq!(
            parse_font_stamp("\"x\" canvas=4096x4096", false).unwrap_err(),
            "canvas-over-4k-softcap"
        );
    }

    #[test]
    fn font_present_defaults_to_a_visible_canvas() {
        let parsed = parse_font_stamp("\"HI\"", true).expect("valid font presentation");
        assert_eq!(parsed.request.fit, FontStampFit::Canvas);
        let scene = &parsed.request.layers[0].scene;
        assert_eq!((scene.raster_width, scene.raster_height), (640, 160));
        assert_eq!(scene.runs[0].position, [24.0, 24.0]);
    }
}
