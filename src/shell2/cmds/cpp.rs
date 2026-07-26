use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const CPP_DEMO_DEFAULT_DURATION_MS: u64 = 30_000;
const CPP_AUDIO_DEFAULT_DURATION_MS: u64 = 0;
const CPP_AUDIO_DEFAULT_CADENCE_MS: u64 = 50;
static CPP_FONT_PROBE_IN_FLIGHT: AtomicBool = AtomicBool::new(false);

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "cpp [gallery|aurora|julia|sdf|voronoi|retro-sun|audio|particle]");
    print_shell_line(
        io,
        "cpp start [gallery|aurora|julia|sdf|voronoi|retro-sun|audio|particle] [duration_ms] [cadence_ms] [publish_every]",
    );
    print_shell_line(io, "cpp list");
    print_shell_line(io, "cpp status");
    print_shell_line(io, "cpp stop");
    print_shell_line(io, "cpp font [stamp|status]");
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
    let particle = preset == crate::ui4::GpgpuPreviewPreset::CppParticle;
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
    let detail = if audio {
        String::from(
            " pcm=post-mix/pre-hda-s16le-stereo-48k fft=2048-mid-side bands=64 walker=horizontal-pairs/50pct",
        )
    } else if particle {
        particle_work_detail()
    } else {
        String::new()
    };
    match crate::ui4::request_gpgpu_preview_start(config) {
        Ok(serial) => {
            let status = crate::ui4::gpgpu_preview_status();
            print_shell_line(
                io,
                alloc::format!(
                    "cpp start: queued=1 request={} mode={} service_online={} duration_ms={} cadence_ms={} publish_every={} frontend=cpp-for-opencl backend=intel-igc-aot runtime_compiler=0 artifact={} zebin_sha256={} target=8086:4680-r0C kernel={} simd=16 ui4_window=1 buffering=double plane=slot1-direct maximize={}{} stop=\"cpp stop\"",
                    serial,
                    cpp_mode_label(preset),
                    status.online as u8,
                    duration_ms,
                    cadence_ms,
                    publish_every,
                    artifact_name(preset),
                    artifact_hash(preset),
                    kernel_name(preset),
                    "dynamic-frame/reconciled",
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
        "cpp suite: sources=cpp_demo_rgba8.clcpp+cpp_audio_visualizer_rgba8.clcpp+particle_craft.clcpp frontend=cpp-for-opencl backend=intel-igc-aot build_time_only=1 exact_target=8086:4680-r0C",
    );
    print_shell_line(
        io,
        "cpp spirit: two native C++ artifacts drive the live Spirit cursor-plane path; use \"cpp spirit list\"",
    );
}

fn particle_candidate_tests() -> u64 {
    u64::from(crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_WIDTH)
        * u64::from(crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_HEIGHT)
        * u64::from(crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES)
}

fn particle_work_detail() -> String {
    alloc::format!(
        " preset=arc-forge particles={} state=8KiB params=v1/64B passes=step+pixel-gather samples={}x{} render_divisor={} candidate_tests={}",
        crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_RENDER_DIVISOR,
        particle_candidate_tests(),
    )
}

fn particle_list_detail() -> String {
    alloc::format!(
        "cpp demo: mode=particle preset=arc-forge explores=persistent-state/two-pass-dependency/soft-cores+velocity-tails+pointer-attraction particles={} native_extent={}x{} samples={}x{} render_divisor={} candidate_tests={} resizable=1",
        crate::intel::gpgpu::PARTICLE_CRAFT_DEFAULT_PARTICLES,
        crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_FRAME_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_WIDTH,
        crate::intel::gpgpu::PARTICLE_CRAFT_SAMPLE_HEIGHT,
        crate::intel::gpgpu::PARTICLE_CRAFT_RENDER_DIVISOR,
        particle_candidate_tests(),
    )
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    let active_cpp = status.desired_running && is_cpp_preset(status.config.preset);
    let audio = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppAudio;
    let particle = status.config.preset == crate::ui4::GpgpuPreviewPreset::CppParticle;
    let upload = if audio {
        crate::intel::gpgpu::cpp_audio_visualizer_rgba8_upload_status()
    } else if particle {
        crate::intel::gpgpu::particle_craft_upload_status()
    } else {
        crate::intel::gpgpu::cpp_demo_rgba8_upload_status()
    };
    let audio_status = crate::aud::audio_visualizer::status();
    let particle_detail = if particle {
        particle_work_detail()
    } else {
        String::new()
    };
    print_shell_line(
        io,
        alloc::format!(
            "cpp status: active={} online={} phase={} request={} applied={} mode={} frame={} window={} extent={}x{} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} marker=0x{:08X} submit_ms={} artifact={} resident={} verified={} gpu=0x{:X} zebin_sha256={} runtime_compiler=0 maximize={} pcm_tap={} pcm_sequence={} pcm_frames={} signal={} rms={:.4} peak={:.4} low={:.3} mid={:.3} high={:.3} beat={:.3} error={}{}",
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
            "dynamic-frame/reconciled",
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
            particle_detail,
        )
        .as_str(),
    );
}

fn artifact_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
        crate::intel::gpgpu::CPP_AUDIO_VISUALIZER_RGBA8_ADLS_ARTIFACT.name
    } else if preset == crate::ui4::GpgpuPreviewPreset::CppParticle {
        crate::intel::gpgpu::PARTICLE_CRAFT_ADLS_ARTIFACT.name
    } else {
        crate::intel::gpgpu::CPP_DEMO_RGBA8_ADLS_ARTIFACT.name
    }
}

fn kernel_name(preset: crate::ui4::GpgpuPreviewPreset) -> &'static str {
    if preset == crate::ui4::GpgpuPreviewPreset::CppParticle {
        "particle_craft_step+particle_craft_render_rgba8"
    } else {
        artifact_name(preset)
    }
}

fn artifact_hash(preset: crate::ui4::GpgpuPreviewPreset) -> String {
    if preset == crate::ui4::GpgpuPreviewPreset::CppAudio {
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
    print_shell_line(
        io,
        alloc::format!(
            "cpp font status: online={} queued={} active_ticket={} active_stage={} lane_retries={} retain_submitted={} retain_completed={} stamp_submitted={} stamp_completed={} failed={} carrier=bsp-controller+leased-blocking-lane ownership=gpu-vm-r8+gpu-vm-rgba8 completion=ticket-signal",
            status.online as u8,
            status.queued,
            status.active_ticket.map(|ticket| ticket.raw()).unwrap_or(0),
            status.active_stage,
            status.lane_retries,
            status.submitted_retain,
            status.completed_retain,
            status.submitted_stamp,
            status.completed_stamp,
            status.failed,
        )
        .as_str(),
    );
}

fn queue_font_service_stamp(spawner: &Spawner, io: &'static dyn ShellBackend2) {
    if CPP_FONT_PROBE_IN_FLIGHT
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        print_shell_line(io, "cpp font stamp: queued=0 reason=probe-in-flight");
        return;
    }
    if !crate::r::font_kernel_service::status().online {
        CPP_FONT_PROBE_IN_FLIGHT.store(false, Ordering::Release);
        print_shell_line(io, "cpp font stamp: queued=0 reason=font-service-offline");
        return;
    }
    let request = crate::r::font_kernel_service::FontStampRequest {
        scene: crate::r::font_kernel_service::RetainSceneRequest {
            runs: alloc::vec![crate::r::font_kernel_service::RetainedFontRun {
                text: String::from("TRUEOS retained GPU font"),
                position: [18.0, 58.0],
                font_pixels: 36.0,
                slant: 0.0,
            }],
            font: crate::intel::gpu_font::GpuFontFace::Default,
            viewport_width: 512,
            viewport_height: 96,
            raster_width: 512,
            raster_height: 96,
            positioning: crate::r::font_kernel_service::RetainedFontPositioning::SceneOrigin,
        },
        foreground: crate::intel::gpu_font::GpuFontRgba::new(80, 225, 255, 255),
    };
    let pending = match crate::r::font_kernel_service::submit_stamp(request) {
        Ok(pending) => pending,
        Err(error) => {
            CPP_FONT_PROBE_IN_FLIGHT.store(false, Ordering::Release);
            print_shell_line(
                io,
                alloc::format!("cpp font stamp: queued=0 reason={error:?}").as_str(),
            );
            return;
        }
    };
    let ticket = pending.ticket().raw();
    match cpp_font_stamp_probe_task(io, pending) {
        Ok(task) => {
            spawner.spawn(task);
            print_shell_line(
                io,
                alloc::format!(
                    "cpp font stamp: queued=1 ticket={} text=\"TRUEOS retained GPU font\" raster=512x96 path=skrifa->gpu-vm-r8->cpp-igc->guc-rcs->gpu-vm-rgba8",
                    ticket,
                )
                .as_str(),
            );
        }
        Err(_) => {
            CPP_FONT_PROBE_IN_FLIGHT.store(false, Ordering::Release);
            print_shell_line(io, "cpp font stamp: queued=0 reason=probe-task-unavailable");
        }
    }
}

#[embassy_executor::task]
async fn cpp_font_stamp_probe_task(
    io: &'static dyn ShellBackend2,
    pending: crate::r::font_kernel_service::PendingFontStamp,
) {
    let ticket = pending.ticket().raw();
    match pending.wait().await {
        Ok(buffer) => {
            let surface = buffer.surface();
            match buffer.readback_tight_rgba() {
                Some(bytes) => {
                    let mut checksum = 0xcbf29ce484222325u64;
                    let mut covered_pixels = 0usize;
                    for pixel in bytes.chunks_exact(4) {
                        for &byte in pixel {
                            checksum ^= u64::from(byte);
                            checksum = checksum.wrapping_mul(0x100000001b3);
                        }
                        covered_pixels += usize::from(pixel[3] != 0);
                    }
                    print_shell_line(
                        io,
                        alloc::format!(
                            "cpp font stamp complete: ticket={} ok={} gpu=0x{:X} extent={}x{} pitch={} submits={} walkers={} covered_pixels={} checksum=fnv1a64:{:016x} runtime_compiler=0",
                            ticket,
                            (covered_pixels != 0) as u8,
                            surface.gpu,
                            surface.width,
                            surface.height,
                            surface.pitch_bytes,
                            buffer.submits(),
                            buffer.active_walkers(),
                            covered_pixels,
                            checksum,
                        )
                        .as_str(),
                    );
                }
                None => print_shell_line(
                    io,
                    alloc::format!(
                        "cpp font stamp complete: ticket={} ok=0 reason=readback-unavailable",
                        ticket,
                    )
                    .as_str(),
                ),
            }
        }
        Err(error) => print_shell_line(
            io,
            alloc::format!("cpp font stamp complete: ticket={} ok=0 reason={error:?}", ticket,)
                .as_str(),
        ),
    }
    CPP_FONT_PROBE_IN_FLIGHT.store(false, Ordering::Release);
}

fn font_service(spawner: &Spawner, io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    match args.next() {
        None => queue_font_service_stamp(spawner, io),
        Some(command) if command.eq_ignore_ascii_case("stamp") => {
            if expect_no_more(io, args) {
                queue_font_service_stamp(spawner, io);
            }
        }
        Some(command) if command.eq_ignore_ascii_case("status") => {
            if expect_no_more(io, args) {
                print_font_service_status(io);
            }
        }
        Some(_) => usage(io),
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
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
    } else if command.eq_ignore_ascii_case("font") {
        font_service(spawner, io, args);
    } else if command.eq_ignore_ascii_case("spirit") {
        spirit(io, args);
    } else if let Some(preset) = parse_mode(command) {
        start(io, preset, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{parse_mode, particle_candidate_tests};

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
    fn particle_default_reports_the_reduced_candidate_work() {
        assert_eq!(particle_candidate_tests(), 8_192_000);
    }
}
