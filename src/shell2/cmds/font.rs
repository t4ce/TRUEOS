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

const DEMO_COLOR_CHECK_RGBA: [GpuFontRgba; 9] = [
    GpuFontRgba::new(255, 0, 0, 255),
    GpuFontRgba::new(0, 255, 0, 255),
    GpuFontRgba::new(0, 0, 255, 255),
    GpuFontRgba::new(0, 255, 255, 255),
    GpuFontRgba::new(255, 0, 255, 255),
    GpuFontRgba::new(255, 255, 0, 255),
    GpuFontRgba::new(255, 255, 255, 255),
    GpuFontRgba::new(128, 128, 128, 255),
    GpuFontRgba::new(0, 0, 0, 255),
];

static PERSISTENT_ENGINE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

struct FontCommand {
    rows: Vec<String>,
    multi_row: bool,
    persistent: bool,
    font: GpuFontFace,
    native_scale: u32,
    color_program: GpuFontColorProgram,
}

struct ColorProgramOptions {
    static_color: Option<GpuFontRgba>,
    from: Option<GpuFontRgba>,
    to: Option<GpuFontRgba>,
    channels: GpuFontColorChannels,
    duration_ms: u32,
    timing: GpuFontColorTiming,
    iteration: GpuFontColorIteration,
    transition_option_seen: bool,
}

struct PersistentFontDemo {
    name: &'static str,
    entries: &'static [(&'static str, [f32; 2])],
    font: GpuFontFace,
    native_scale: u32,
    color_program: GpuFontColorProgram,
}

impl ColorProgramOptions {
    const fn new() -> Self {
        Self {
            static_color: None,
            from: None,
            to: None,
            channels: GpuFontColorChannels::RGBA,
            duration_ms: 1_000,
            timing: GpuFontColorTiming::Linear,
            iteration: GpuFontColorIteration::Alternate,
            transition_option_seen: false,
        }
    }

    fn consume(&mut self, part: &str) -> Result<bool, &'static str> {
        if part.starts_with("rgba=") {
            return Err("rgba-queue-removed-use-color-or-from-to");
        }
        if let Some(encoded) = part.strip_prefix("color=") {
            if self.static_color.replace(parse_rgba(encoded)?).is_some() {
                return Err("color-duplicate");
            }
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("from=") {
            self.from = Some(parse_rgba(encoded)?);
            self.transition_option_seen = true;
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("to=") {
            self.to = Some(parse_rgba(encoded)?);
            self.transition_option_seen = true;
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("channels=") {
            self.channels = parse_color_channels(encoded)?;
            self.transition_option_seen = true;
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("duration=") {
            self.duration_ms = parse_color_duration_ms(encoded)?;
            self.transition_option_seen = true;
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("timing=") {
            self.timing = parse_color_timing(encoded)?;
            self.transition_option_seen = true;
            return Ok(true);
        }
        if let Some(encoded) = part.strip_prefix("iteration=") {
            self.iteration = parse_color_iteration(encoded)?;
            self.transition_option_seen = true;
            return Ok(true);
        }
        Ok(false)
    }

    fn finish(self) -> Result<GpuFontColorProgram, &'static str> {
        if self.transition_option_seen {
            if self.static_color.is_some() {
                return Err("color-conflicts-with-transition");
            }
            Ok(GpuFontColorProgram::Transition(GpuFontColorTransition {
                from: self.from.ok_or("transition-from-required")?,
                to: self.to.ok_or("transition-to-required")?,
                channels: self.channels,
                duration_ms: self.duration_ms,
                timing: self.timing,
                iteration: self.iteration,
            }))
        } else {
            Ok(GpuFontColorProgram::Static(self.static_color.unwrap_or(GPU_FONT_LEGACY_BLUE)))
        }
    }
}

fn demo_transition(
    from: GpuFontRgba,
    to: GpuFontRgba,
    channels: GpuFontColorChannels,
    duration_ms: u32,
    timing: GpuFontColorTiming,
) -> GpuFontColorProgram {
    GpuFontColorProgram::Transition(GpuFontColorTransition {
        from,
        to,
        channels,
        duration_ms,
        timing,
        iteration: GpuFontColorIteration::Alternate,
    })
}

fn persistent_font_demo(id: u8) -> Option<PersistentFontDemo> {
    let demo = match id {
        1 => PersistentFontDemo {
            name: "blue-breathe",
            entries: &[("True OS", [0.0, 0.0])],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(0, 64, 255, 48),
                GpuFontRgba::new(0, 64, 255, 255),
                GpuFontColorChannels::ALPHA,
                2_800,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        2 => PersistentFontDemo {
            name: "quiet-linear",
            entries: &[("resident", [0.0, 0.0]), ("geometry", [72.0, 92.0])],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(220, 232, 255, 72),
                GpuFontRgba::new(220, 232, 255, 230),
                GpuFontColorChannels::ALPHA,
                3_500,
                GpuFontColorTiming::Linear,
            ),
        },
        3 => PersistentFontDemo {
            name: "kernel-scatter",
            entries: &[
                ("kernel", [0.0, 0.0]),
                ("font", [150.0, 82.0]),
                ("service", [28.0, 174.0]),
            ],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(64, 128, 255, 255),
                GpuFontRgba::new(64, 216, 255, 255),
                GpuFontColorChannels::RGB,
                4_000,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        4 => PersistentFontDemo {
            name: "warm-diagonal",
            entries: &[("hello", [0.0, 0.0]), ("world", [142.0, 104.0])],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(224, 152, 64, 144),
                GpuFontRgba::new(255, 208, 112, 255),
                GpuFontColorChannels::RGBA,
                4_500,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        5 => PersistentFontDemo {
            name: "noto-bilingual",
            entries: &[("你好", [0.0, 0.0]), ("True OS", [132.0, 112.0])],
            font: GpuFontFace::NotoSansSc,
            native_scale: 6,
            color_program: demo_transition(
                GpuFontRgba::new(36, 96, 210, 255),
                GpuFontRgba::new(28, 176, 156, 255),
                GpuFontColorChannels::RGB,
                5_000,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        6 => PersistentFontDemo {
            name: "rgba-steps",
            entries: &[
                ("GPU", [0.0, 0.0]),
                ("RGBA", [132.0, 76.0]),
                ("frame", [42.0, 166.0]),
            ],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(112, 96, 224, 112),
                GpuFontRgba::new(176, 144, 255, 255),
                GpuFontColorChannels::RGBA,
                3_800,
                GpuFontColorTiming::Linear,
            ),
        },
        7 => PersistentFontDemo {
            name: "alpha-breath",
            entries: &[("alpha", [0.0, 0.0]), ("breath", [126.0, 112.0])],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(236, 240, 255, 32),
                GpuFontRgba::new(236, 240, 255, 224),
                GpuFontColorChannels::ALPHA,
                5_500,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        8 => PersistentFontDemo {
            name: "resident-proof",
            entries: &[
                ("one mesh", [0.0, 0.0]),
                ("one draw", [58.0, 102.0]),
                ("no upload", [116.0, 204.0]),
            ],
            font: GpuFontFace::Default,
            native_scale: 5,
            color_program: demo_transition(
                GpuFontRgba::new(96, 152, 184, 255),
                GpuFontRgba::new(112, 216, 224, 255),
                GpuFontColorChannels::RGB,
                6_000,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        9 => PersistentFontDemo {
            name: "trueos-authority",
            entries: &[("TRUE OS §", [0.0, 0.0]), ("iGPU resident", [38.0, 132.0])],
            font: GpuFontFace::Default,
            native_scale: 6,
            color_program: demo_transition(
                GpuFontRgba::new(112, 72, 208, 176),
                GpuFontRgba::new(64, 152, 255, 255),
                GpuFontColorChannels::RGBA,
                6_500,
                GpuFontColorTiming::EaseInOutSine,
            ),
        },
        _ => return None,
    };
    Some(demo)
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if rest.trim().eq_ignore_ascii_case("persist stop") {
        match crate::intel::gpu_font::stop_persistent_font_animation() {
            Ok(stopped) => {
                print_shell_line(io, format!("font persist: stopped={}", stopped as u8).as_str())
            }
            Err(reason) => {
                print_shell_line(io, format!("font persist: stopped=0 reason={reason}").as_str())
            }
        }
        return ParseOutcome::Handled;
    }
    if rest.trim().eq_ignore_ascii_case("persist status") {
        print_persistent_status(io);
        return ParseOutcome::Handled;
    }
    if let Some(after_persist) = strip_keyword(rest.trim(), "persist")
        && let Some(after_demo) = strip_keyword(after_persist.trim_start(), "demo")
    {
        let selector = after_demo.trim();
        if selector == "list" {
            print_shell_line(
                io,
                "font persist demos: 1=blue-breathe 2=quiet-linear 3=kernel-scatter 4=warm-diagonal 5=noto-bilingual 6=rgba-steps 7=alpha-breath 8=resident-proof 9=trueos-authority; all=`font persist demo 1-9`; exact opaque RGBA check=`font persist demo colors`",
            );
            return ParseOutcome::Handled;
        }
        if selector == "1-9" {
            run_persistent_demo_grid(spawner, io, false);
            return ParseOutcome::Handled;
        }
        if selector.eq_ignore_ascii_case("colors") {
            run_persistent_demo_grid(spawner, io, true);
            return ParseOutcome::Handled;
        }
        let id = match selector.parse::<u8>() {
            Ok(id @ 1..=9) => id,
            _ => {
                print_shell_line(io, "font persist demo: installed=0 reason=expected-id-1-to-9");
                return ParseOutcome::Handled;
            }
        };
        run_persistent_demo(spawner, io, id);
        return ParseOutcome::Handled;
    }
    if let Some(after_persist) = strip_keyword(rest.trim(), "persist")
        && let Some(after_set) = strip_keyword(after_persist.trim_start(), "set")
    {
        let color_program = match parse_color_program_control(after_set.trim_start()) {
            Ok(program) => program,
            Err(reason) => {
                print_shell_line(io, format!("font persist: updated=0 reason={reason}").as_str());
                print_usage(io);
                return ParseOutcome::Handled;
            }
        };
        match crate::intel::gpu_font::set_persistent_font_color_program(color_program) {
            Ok(status) => print_shell_line(
                io,
                format!(
                    "font persist: updated=1 id={} generation={} program={} geometry_uploads=0 elapsed_ms={}",
                    status.id,
                    status.generation,
                    status.color_program.name(),
                    status.elapsed_ms,
                )
                .as_str(),
            ),
            Err(reason) => {
                print_shell_line(io, format!("font persist: updated=0 reason={reason}").as_str())
            }
        }
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
    let mut color = ColorProgramOptions::new();
    let mut color_option_seen = false;
    for part in input.split_whitespace() {
        if color.consume(part)? {
            color_option_seen = true;
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
    if !persistent && color_option_seen {
        return Err("color-requires-persist");
    }
    Ok((font, scale, color.finish()?))
}

fn parse_color_program_control(input: &str) -> Result<GpuFontColorProgram, &'static str> {
    if input.is_empty() {
        return Err("color-program-required");
    }
    let mut color = ColorProgramOptions::new();
    for part in input.split_whitespace() {
        if !color.consume(part)? {
            return Err("unexpected-color-program-argument");
        }
    }
    color.finish()
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
    let entry = GpuFontJobEntry {
        text: request,
        position: [0.0, 0.0],
    };
    let job = GpuFontJob {
        entries: core::slice::from_ref(&entry),
        font: command.font,
        native_scale: command.native_scale,
    };
    match install_persistent_job(spawner, job, command.color_program) {
        Ok(status) => print_shell_line(
            io,
            persistent_install_message(status, command.font.id(), command.native_scale).as_str(),
        ),
        Err(reason) => {
            print_shell_line(io, format!("font persist: installed=0 reason={reason}").as_str())
        }
    }
}

fn run_persistent_demo(spawner: &Spawner, io: &'static dyn ShellBackend2, id: u8) {
    let Some(demo) = persistent_font_demo(id) else {
        print_shell_line(io, "font persist demo: installed=0 reason=expected-id-1-to-9");
        return;
    };
    let entries: Vec<GpuFontJobEntry<'_>> = demo
        .entries
        .iter()
        .map(|(text, position)| GpuFontJobEntry {
            text: GpuFontTextRequest::SingleLine(text),
            position: *position,
        })
        .collect();
    let job = GpuFontJob {
        entries: entries.as_slice(),
        font: demo.font,
        native_scale: demo.native_scale,
    };
    match install_persistent_job(spawner, job, demo.color_program) {
        Ok(status) => {
            print_shell_line(
                io,
                format!(
                    "font persist demo: installed=1 demo={} name={} entries={} geometry_uploads=1 animation={}",
                    id,
                    demo.name,
                    demo.entries.len(),
                    demo.color_program.name(),
                )
                .as_str(),
            );
            print_shell_line(
                io,
                persistent_install_message(status, demo.font.id(), demo.native_scale).as_str(),
            );
        }
        Err(reason) => print_shell_line(
            io,
            format!("font persist demo: installed=0 demo={id} reason={reason}").as_str(),
        ),
    }
}

fn run_persistent_demo_grid(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    exact_color_check: bool,
) {
    if let Err(reason) = ensure_persistent_engine_task(spawner) {
        print_shell_line(
            io,
            format!("font persist demo grid: installed=0 reason={reason}").as_str(),
        );
        return;
    }
    if let Err(reason) = crate::intel::gpu_font::stop_persistent_font_animation() {
        print_shell_line(
            io,
            format!("font persist demo grid: installed=0 reason={reason}").as_str(),
        );
        return;
    }

    let mut resident = Vec::with_capacity(9);
    for index in 0..9usize {
        let Some(demo) = persistent_font_demo(index as u8 + 1) else {
            print_shell_line(io, "font persist demo grid: installed=0 reason=demo-catalog");
            return;
        };
        let entries: Vec<GpuFontJobEntry<'_>> = demo
            .entries
            .iter()
            .map(|(text, position)| GpuFontJobEntry {
                text: GpuFontTextRequest::SingleLine(text),
                position: *position,
            })
            .collect();
        let job = GpuFontJob {
            entries: entries.as_slice(),
            font: demo.font,
            native_scale: demo.native_scale,
        };
        let tag = match crate::intel::gpu_font::persistent_font_demo_grid_tag(index) {
            Ok(tag) => tag,
            Err(reason) => {
                print_shell_line(
                    io,
                    format!("font persist demo grid: installed=0 reason={reason}").as_str(),
                );
                return;
            }
        };
        let lease = match crate::intel::gpu_font::persist_font_job(tag, job) {
            Ok(lease) => lease,
            Err(reason) => {
                print_shell_line(
                    io,
                    format!("font persist demo grid: installed=0 reason={reason}").as_str(),
                );
                return;
            }
        };
        let color_program = if exact_color_check {
            GpuFontColorProgram::Static(DEMO_COLOR_CHECK_RGBA[index])
        } else {
            demo.color_program
        };
        resident.push((lease, color_program));
    }

    match crate::intel::gpu_font::install_persistent_font_demo_grid(resident, exact_color_check) {
        Ok(()) if exact_color_check => print_shell_line(
            io,
            "font persist color-check: installed=1 layout=3x3 mapping=red,green,blue/cyan,magenta,yellow/white,gray,black alpha=255 expected=exact-rgba render-target-proof=logged",
        ),
        Ok(()) => print_shell_line(
            io,
            "font persist demo grid: installed=1 demos=1-9 cells=9 layout=3x3 geometry_uploads=9 clock=monotonic-elapsed overlay_commits=1-per-update",
        ),
        Err(reason) => print_shell_line(
            io,
            format!("font persist demo grid: installed=0 reason={reason}").as_str(),
        ),
    }
}

fn install_persistent_job(
    spawner: &Spawner,
    job: GpuFontJob<'_>,
    color_program: GpuFontColorProgram,
) -> Result<crate::intel::gpu_font::PersistentGpuFontAnimationStatus, &'static str> {
    ensure_persistent_engine_task(spawner)?;
    // Persistent shell jobs have single-active replacement semantics. Wait for
    // the current synchronous frame, retire it, unmap it and only then reuse
    // the one attributable residency tag. A quarantined draw deliberately
    // blocks replacement rather than leaking another resident allocation.
    crate::intel::gpu_font::stop_persistent_font_animation()?;
    let tag = match crate::intel::gpu_font::next_persistent_font_animation_tag() {
        Ok(tag) => tag,
        Err(reason) => return Err(reason),
    };
    let lease = match crate::intel::gpu_font::persist_font_job(tag, job) {
        Ok(lease) => lease,
        Err(reason) => return Err(reason),
    };
    crate::intel::gpu_font::install_persistent_font_animation(lease, color_program)
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
    if let Some(status) = crate::intel::gpu_font::persistent_font_demo_grid_status() {
        print_shell_line(
            io,
            format!(
                "font persist: active=1 mode=demo-grid cells={} layout=3x3 engine_frame_requests={} presented_frames={} failures={} halted_cells={} exact_color_check={} color_proof_pixels={} color_proof_mismatches={} clock=monotonic-elapsed",
                status.cells,
                status.engine_frame_requests,
                status.presented_frames,
                status.failures,
                status.halted_cells,
                status.exact_color_check as u8,
                status.color_proof_pixels,
                status.color_proof_mismatches,
            )
            .as_str(),
        );
        return;
    }
    match crate::intel::gpu_font::persistent_font_animation_status() {
        Some(status) => print_shell_line(io, persistent_status_message(status).as_str()),
        None => print_shell_line(io, "font persist: active=0"),
    }
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "font: `font \"text\" [font_id 1|2] [scale 1..8]`; demos: `font persist demo 1` .. `font persist demo 9`, all: `font persist demo 1-9`, RGBA proof: `font persist demo colors`, catalogue: `font persist demo list`; persistent static: `font persist \"text\" [font_id] [scale] color=RRGGBBAA`; transition options: `from=RRGGBBAA to=RRGGBBAA channels=a|rgb|rgba duration=2s timing=linear|sine iteration=once|loop|alternate`; update active geometry without upload: `font persist set <color|transition options>`; control: `font persist status|stop`; rows: `font [persist] rows \"row 1\" \"row 2\" ...`",
    );
}
