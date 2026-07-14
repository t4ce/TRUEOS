use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpu_font::{
    GPU_FONT_LEGACY_BLUE, GpuFontColorChannels, GpuFontColorIteration, GpuFontColorProgram,
    GpuFontColorTiming, GpuFontColorTransition, GpuFontFace, GpuFontJob, GpuFontJobEntry,
    GpuFontRgba, GpuFontTextRequest,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const MIN_NATIVE_SCALE: u32 = 1;
const MAX_NATIVE_SCALE: u32 = 8;
const PERSISTENT_ENGINE_FRAME_MS: u64 = 16;
const MIN_COLOR_DURATION_MS: u32 = 16;
const MAX_COLOR_DURATION_MS: u32 = 600_000;

static PERSISTENT_ENGINE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

struct FontCommand {
    rows: Vec<String>,
    multi_row: bool,
    persistent: bool,
    font: GpuFontFace,
    native_scale: u32,
    color_program: GpuFontColorProgram,
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if rest.trim().eq_ignore_ascii_case("persist stop") {
        let stopped = crate::intel::gpu_font::stop_persistent_font_animation();
        print_shell_line(io, format!("font persist: stopped={}", stopped as u8).as_str());
        return ParseOutcome::Handled;
    }
    if rest.trim().eq_ignore_ascii_case("persist status") {
        print_persistent_status(io);
        return ParseOutcome::Handled;
    }

    let command = match parse_request(rest) {
        Ok(command) => command,
        Err(reason) => {
            print_shell_line(io, format!("font: error={}", reason).as_str());
            print_usage(io);
            return ParseOutcome::Handled;
        }
    };

    let row_refs: Vec<&str> = command.rows.iter().map(String::as_str).collect();
    let request = if command.multi_row {
        GpuFontTextRequest::Rows(row_refs.as_slice())
    } else {
        GpuFontTextRequest::SingleLine(row_refs[0])
    };
    if command.persistent {
        run_persistent_command(spawner, io, &command, request);
        return ParseOutcome::Handled;
    }
    match crate::intel::gpu_font::render_text_once_with_font(
        request,
        command.font,
        command.native_scale,
    ) {
        Ok(result) => print_shell_line(
            io,
            format!(
                "font: presented=1 font_id={} font={} file={} layout={} text_chars={} rows={} native_scale={} native_size={}x{} glyphs={} glyph_hits={} glyph_misses={} vertices={} indices={} submits=1 completed={} ps={}",
                command.font.id(),
                result.summary.font_name,
                result.summary.font_file,
                result.layout.name(),
                result.text_chars,
                result.rows,
                command.native_scale,
                64 * command.native_scale,
                64 * command.native_scale,
                result.summary.glyphs,
                result.summary.glyph_hits,
                result.summary.glyph_misses,
                result.summary.vertices,
                result.summary.indices,
                result.render.completed as u8,
                result.render.ps_observed as u8,
            )
            .as_str(),
        ),
        Err(reason) => {
            print_shell_line(io, format!("font: presented=0 reason={}", reason).as_str());
        }
    }

    ParseOutcome::Handled
}

fn parse_request(rest: &str) -> Result<FontCommand, &'static str> {
    let input = rest.trim();
    let (persistent, input) = if let Some(after_persist) = strip_keyword(input, "persist") {
        (true, after_persist.trim_start())
    } else {
        (false, input)
    };
    let (multi_row, mut remaining) = if let Some(after_rows) = strip_keyword(input, "rows") {
        (true, after_rows.trim_start())
    } else {
        (false, input)
    };

    let mut rows = Vec::new();
    while remaining.starts_with('"') {
        let (row, tail) = parse_quoted(remaining)?;
        rows.push(row);
        remaining = tail.trim_start();
        if !multi_row {
            break;
        }
    }
    if rows.is_empty() {
        return Err("text-must-be-quoted");
    }

    let (font, native_scale, color_program) =
        parse_font_scale_and_color_program(remaining, persistent)?;
    Ok(FontCommand {
        rows,
        multi_row,
        persistent,
        font,
        native_scale,
        color_program,
    })
}

fn strip_keyword<'a>(input: &'a str, keyword: &str) -> Option<&'a str> {
    let tail = input.strip_prefix(keyword)?;
    if tail.chars().next().is_some_and(|ch| !ch.is_whitespace()) {
        None
    } else {
        Some(tail)
    }
}

fn parse_quoted(input: &str) -> Result<(String, &str), &'static str> {
    let quoted = input.strip_prefix('"').ok_or("text-must-be-quoted")?;
    let mut text = String::new();
    let mut chars = quoted.char_indices();
    while let Some((offset, ch)) = chars.next() {
        match ch {
            '"' => {
                let after_quote = 1 + offset + ch.len_utf8();
                return Ok((text, &input[after_quote..]));
            }
            '\\' => {
                let Some((_, escaped)) = chars.next() else {
                    return Err("unfinished-escape");
                };
                match escaped {
                    '"' | '\\' => text.push(escaped),
                    'u' => text.push(parse_unicode_escape(&mut chars)?),
                    _ => return Err("unsupported-escape"),
                }
            }
            ch => text.push(ch),
        }
    }
    Err("missing-closing-quote")
}

fn parse_unicode_escape(
    chars: &mut impl Iterator<Item = (usize, char)>,
) -> Result<char, &'static str> {
    if chars.next().map(|(_, ch)| ch) != Some('{') {
        return Err("unicode-escape-open-brace");
    }
    let mut value = 0u32;
    let mut digits = 0usize;
    loop {
        let Some((_, ch)) = chars.next() else {
            return Err("unfinished-unicode-escape");
        };
        if ch == '}' {
            break;
        }
        let digit = ch.to_digit(16).ok_or("unicode-escape-hex")?;
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
    char::from_u32(value).ok_or("unicode-escape-invalid-scalar")
}

fn parse_font_scale_and_color_program(
    input: &str,
    persistent: bool,
) -> Result<(GpuFontFace, u32, GpuFontColorProgram), &'static str> {
    let mut font = GpuFontFace::Default;
    let mut scale = crate::intel::render::FONT_STAMP_DEFAULT_NATIVE_SCALE;
    let mut numeric_arguments = 0usize;
    let mut static_color = None;
    let mut from = None;
    let mut to = None;
    let mut channels = GpuFontColorChannels::RGBA;
    let mut duration_ms = 1_000;
    let mut timing = GpuFontColorTiming::Linear;
    let mut iteration = GpuFontColorIteration::Alternate;
    let mut transition_option_seen = false;
    for part in input.split_whitespace() {
        if part.starts_with("rgba=") {
            return Err("rgba-queue-removed-use-color-or-from-to");
        }
        if let Some(encoded) = part.strip_prefix("color=") {
            if !persistent {
                return Err("color-requires-persist");
            }
            if static_color.replace(parse_rgba(encoded)?).is_some() {
                return Err("color-duplicate");
            }
            continue;
        }
        if let Some(encoded) = part.strip_prefix("from=") {
            from = Some(parse_rgba(encoded)?);
            transition_option_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("to=") {
            to = Some(parse_rgba(encoded)?);
            transition_option_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("channels=") {
            channels = parse_color_channels(encoded)?;
            transition_option_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("duration=") {
            duration_ms = parse_color_duration_ms(encoded)?;
            transition_option_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("timing=") {
            timing = parse_color_timing(encoded)?;
            transition_option_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("iteration=") {
            iteration = parse_color_iteration(encoded)?;
            transition_option_seen = true;
            continue;
        }
        let number = part.parse::<u32>().map_err(|_| "unexpected-argument")?;
        match numeric_arguments {
            0 => {
                font = GpuFontFace::from_id(number).ok_or("font-id-out-of-range-1-to-2")?;
            }
            1 => scale = number,
            _ => return Err("unexpected-argument"),
        }
        numeric_arguments += 1;
    }
    if !(MIN_NATIVE_SCALE..=MAX_NATIVE_SCALE).contains(&scale) {
        return Err("scale-out-of-range-1-to-8");
    }
    if !persistent && (static_color.is_some() || transition_option_seen) {
        return Err("color-requires-persist");
    }
    let color_program = if transition_option_seen {
        if static_color.is_some() {
            return Err("color-conflicts-with-transition");
        }
        GpuFontColorProgram::Transition(GpuFontColorTransition {
            from: from.ok_or("transition-from-required")?,
            to: to.ok_or("transition-to-required")?,
            channels,
            duration_ms,
            timing,
            iteration,
        })
    } else {
        GpuFontColorProgram::Static(static_color.unwrap_or(GPU_FONT_LEGACY_BLUE))
    };
    Ok((font, scale, color_program))
}

fn parse_rgba(encoded: &str) -> Result<GpuFontRgba, &'static str> {
    let encoded = encoded
        .strip_prefix('#')
        .or_else(|| encoded.strip_prefix("0x"))
        .unwrap_or(encoded);
    if encoded.len() != 8 || !encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("color-expected-RRGGBBAA");
    }
    let packed = u32::from_str_radix(encoded, 16).map_err(|_| "color-invalid")?;
    Ok(GpuFontRgba::from_rgba_u32(packed))
}

fn parse_color_channels(encoded: &str) -> Result<GpuFontColorChannels, &'static str> {
    match encoded {
        "r" | "red" => Ok(GpuFontColorChannels::RED),
        "g" | "green" => Ok(GpuFontColorChannels::GREEN),
        "b" | "blue" => Ok(GpuFontColorChannels::BLUE),
        "a" | "alpha" => Ok(GpuFontColorChannels::ALPHA),
        "rgb" => Ok(GpuFontColorChannels::RGB),
        "rgba" | "all" => Ok(GpuFontColorChannels::RGBA),
        _ => Err("channels-expected-r-g-b-a-rgb-rgba"),
    }
}

fn parse_color_duration_ms(encoded: &str) -> Result<u32, &'static str> {
    let duration_ms = if let Some(millis) = encoded.strip_suffix("ms") {
        millis.parse::<u32>().map_err(|_| "duration-invalid")?
    } else if let Some(seconds) = encoded.strip_suffix('s') {
        seconds
            .parse::<u32>()
            .map_err(|_| "duration-invalid")?
            .checked_mul(1_000)
            .ok_or("duration-out-of-range")?
    } else {
        encoded.parse::<u32>().map_err(|_| "duration-invalid")?
    };
    if !(MIN_COLOR_DURATION_MS..=MAX_COLOR_DURATION_MS).contains(&duration_ms) {
        return Err("duration-out-of-range-16ms-to-600s");
    }
    Ok(duration_ms)
}

fn parse_color_timing(encoded: &str) -> Result<GpuFontColorTiming, &'static str> {
    match encoded {
        "linear" => Ok(GpuFontColorTiming::Linear),
        "sine" | "ease-in-out-sine" => Ok(GpuFontColorTiming::EaseInOutSine),
        _ => Err("timing-expected-linear-or-sine"),
    }
}

fn parse_color_iteration(encoded: &str) -> Result<GpuFontColorIteration, &'static str> {
    match encoded {
        "once" => Ok(GpuFontColorIteration::Once),
        "loop" | "infinite" => Ok(GpuFontColorIteration::Loop),
        "alternate" => Ok(GpuFontColorIteration::Alternate),
        _ => Err("iteration-expected-once-loop-alternate"),
    }
}

fn run_persistent_command(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    command: &FontCommand,
    request: GpuFontTextRequest<'_>,
) {
    if let Err(reason) = ensure_persistent_engine_task(spawner) {
        print_shell_line(io, format!("font persist: installed=0 reason={reason}").as_str());
        return;
    }
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
    };
    let job = GpuFontJob {
        entries: core::slice::from_ref(&entry),
        font: command.font,
        native_scale: command.native_scale,
    };
    let tag = match crate::intel::gpu_font::next_persistent_font_animation_tag() {
        Ok(tag) => tag,
        Err(reason) => {
            print_shell_line(io, format!("font persist: installed=0 reason={reason}").as_str());
            return;
        }
    };
    let lease = match crate::intel::gpu_font::persist_font_job(tag, job) {
        Ok(lease) => lease,
        Err(reason) => {
            print_shell_line(io, format!("font persist: installed=0 reason={reason}").as_str());
            return;
        }
    };
    match crate::intel::gpu_font::install_persistent_font_animation(lease, command.color_program) {
        Ok(status) => print_shell_line(
            io,
            persistent_install_message(status, command.font.id(), command.native_scale).as_str(),
        ),
        Err(reason) => {
            print_shell_line(io, format!("font persist: installed=0 reason={reason}").as_str())
        }
    }
}

fn ensure_persistent_engine_task(spawner: &Spawner) -> Result<(), &'static str> {
    if PERSISTENT_ENGINE_TASK_STARTED.swap(true, Ordering::AcqRel) {
        return Ok(());
    }
    match persistent_font_engine_task() {
        Ok(token) => {
            spawner.spawn(token);
            Ok(())
        }
        Err(_) => {
            PERSISTENT_ENGINE_TASK_STARTED.store(false, Ordering::Release);
            Err("engine-frame-task-unavailable")
        }
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn persistent_font_engine_task() {
    loop {
        crate::intel::gpu_font::submit_persistent_font_animation_engine_frame();
        Timer::after(EmbassyDuration::from_millis(PERSISTENT_ENGINE_FRAME_MS)).await;
    }
}

fn persistent_install_message(
    status: crate::intel::gpu_font::PersistentGpuFontAnimationStatus,
    font_id: u8,
    native_scale: u32,
) -> String {
    let prefix = format!(
        "font persist: installed=1 id={} generation={} font_id={} scale={} program={} cadence_ms={}",
        status.id,
        status.generation,
        font_id,
        native_scale,
        status.color_program.name(),
        PERSISTENT_ENGINE_FRAME_MS,
    );
    match status.color_program {
        GpuFontColorProgram::Static(rgba) => {
            format!("{prefix} color={}", rgba_hex(rgba))
        }
        GpuFontColorProgram::Transition(transition) => format!(
            "{prefix} from={} to={} channels={} duration_ms={} timing={} iteration={} clock=monotonic-elapsed",
            rgba_hex(transition.from),
            rgba_hex(transition.to),
            transition.channels.name(),
            transition.duration_ms,
            transition.timing.name(),
            transition.iteration.name(),
        ),
    }
}

fn persistent_status_message(
    status: crate::intel::gpu_font::PersistentGpuFontAnimationStatus,
) -> String {
    let common = format!(
        "font persist: active=1 id={} generation={} program={} elapsed_ms={} engine_frame_requests={} submitted_frames={} failures={} halted={} last_rgba={:?}",
        status.id,
        status.generation,
        status.color_program.name(),
        status.elapsed_ms,
        status.engine_frame_requests,
        status.submitted_frames,
        status.failures,
        status.halted as u8,
        status.last_submitted,
    );
    match status.color_program {
        GpuFontColorProgram::Static(rgba) => format!("{common} color={}", rgba_hex(rgba)),
        GpuFontColorProgram::Transition(transition) => format!(
            "{common} from={} to={} channels={} duration_ms={} timing={} iteration={}",
            rgba_hex(transition.from),
            rgba_hex(transition.to),
            transition.channels.name(),
            transition.duration_ms,
            transition.timing.name(),
            transition.iteration.name(),
        ),
    }
}

fn rgba_hex(rgba: GpuFontRgba) -> String {
    format!("{:02X}{:02X}{:02X}{:02X}", rgba.r, rgba.g, rgba.b, rgba.a)
}

fn print_persistent_status(io: &'static dyn ShellBackend2) {
    match crate::intel::gpu_font::persistent_font_animation_status() {
        Some(status) => print_shell_line(io, persistent_status_message(status).as_str()),
        None => print_shell_line(io, "font persist: active=0"),
    }
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "font: `font \"text\" [font_id 1|2] [scale 1..8]`; persistent static: `font persist \"text\" [font_id] [scale] color=RRGGBBAA`; transition: `from=RRGGBBAA to=RRGGBBAA channels=a|rgb|rgba duration=2s timing=linear|sine iteration=once|loop|alternate`; control: `font persist status|stop`; rows: `font [persist] rows \"row 1\" \"row 2\" ...`",
    );
}
