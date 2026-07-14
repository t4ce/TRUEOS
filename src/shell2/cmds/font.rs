use alloc::{format, string::String, vec::Vec};

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpu_font::{GPU_FONT_LEGACY_BLUE, GpuFontFace, GpuFontRgba, GpuFontTextRequest};
use crate::shell2::shell2_cmd::ParseOutcome;

const MIN_SIZE_PERCENT: u32 = 1;
const MAX_SIZE_PERCENT: u32 = 100;
const DEFAULT_SIZE_PERCENT: u32 = 100;

struct FontCommand {
    rows: Vec<String>,
    multi_row: bool,
    font: GpuFontFace,
    size_percent: u32,
    color: GpuFontRgba,
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let command = match parse_request(rest) {
        Ok(command) => command,
        Err(reason) => {
            print_shell_line(io, format!("font: stamped=0 reason={reason}").as_str());
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
    match crate::intel::gpu_font::stamp_text_once_with_font_centered(
        request,
        command.font,
        command.size_percent,
        command.color,
    ) {
        Ok(result) => print_shell_line(
            io,
            format!(
                "font: stamped={} target=primary-native scanout={}x{} placement=centered fit=contain size={}percent dst={},{} stamp={}x{} source={}x{} font_id={} font={} file={} layout={} text_chars={} rows={} rgba={:02X}{:02X}{:02X}{:02X} glyphs={} glyph_hits={} glyph_misses={} vertices={} indices={} submits=1 completed={} ps={} persistent=0 transport=kernel-direct",
                result.stamped as u8,
                result.scanout_width,
                result.scanout_height,
                result.size_percent,
                result.dst_x,
                result.dst_y,
                result.stamp_width,
                result.stamp_height,
                result.source_width,
                result.source_height,
                command.font.id(),
                result.summary.font_name,
                result.summary.font_file,
                result.layout.name(),
                result.text_chars,
                result.rows,
                command.color.r,
                command.color.g,
                command.color.b,
                command.color.a,
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
            print_shell_line(io, format!("font: stamped=0 reason={reason}").as_str());
        }
    }

    ParseOutcome::Handled
}

fn parse_request(rest: &str) -> Result<FontCommand, &'static str> {
    let input = rest.trim();
    if matches!(
        input.split_whitespace().next(),
        Some("demo" | "set" | "status" | "stop" | "persist")
    ) {
        return Err("persistent-scene-controls-removed");
    }
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

    let (font, size_percent, color) = parse_options(remaining)?;
    Ok(FontCommand {
        rows,
        multi_row,
        font,
        size_percent,
        color,
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

fn parse_options(input: &str) -> Result<(GpuFontFace, u32, GpuFontRgba), &'static str> {
    let mut font = GpuFontFace::Default;
    let mut size_percent = DEFAULT_SIZE_PERCENT;
    let mut color = GPU_FONT_LEGACY_BLUE;
    let mut font_seen = false;
    let mut size_seen = false;
    let mut color_seen = false;
    for part in input.split_whitespace() {
        if let Some(encoded) = part.strip_prefix("color=") {
            if color_seen {
                return Err("color-duplicate");
            }
            color = parse_rgba(encoded)?;
            color_seen = true;
            continue;
        }
        if let Some(encoded) = part
            .strip_prefix("font=")
            .or_else(|| part.strip_prefix("font_id="))
        {
            if font_seen {
                return Err("font-duplicate");
            }
            let id = encoded.parse::<u32>().map_err(|_| "font-id-invalid")?;
            font = GpuFontFace::from_id(id).ok_or("font-id-out-of-range-1-to-2")?;
            font_seen = true;
            continue;
        }
        if let Some(encoded) = part.strip_prefix("size=") {
            if size_seen {
                return Err("size-duplicate");
            }
            size_percent = encoded.parse::<u32>().map_err(|_| "size-percent-invalid")?;
            size_seen = true;
            continue;
        }
        if part.starts_with("scale=") {
            return Err("scale-removed-use-size-percent");
        }
        if part.starts_with("from=")
            || part.starts_with("to=")
            || part.starts_with("channels=")
            || part.starts_with("duration=")
            || part.starts_with("timing=")
            || part.starts_with("iteration=")
        {
            return Err("animated-color-removed-use-color");
        }
        if size_seen {
            return Err("unexpected-argument");
        }
        size_percent = part
            .parse::<u32>()
            .map_err(|_| "unexpected-argument-expected-size-percent")?;
        size_seen = true;
    }
    if !(MIN_SIZE_PERCENT..=MAX_SIZE_PERCENT).contains(&size_percent) {
        return Err("size-percent-out-of-range-1-to-100");
    }
    Ok((font, size_percent, color))
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

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "font: centered aspect-fit stamp: `font \"text\" [1..100] [font=1|2] [color=RRGGBBAA]`; size defaults to 100 percent of the scanout fit and may also be written `size=N`; rows: `font rows \"row 1\" \"row 2\" ...`; geometry is transient and no Draw3D scene or TCP connection is used",
    );
}
