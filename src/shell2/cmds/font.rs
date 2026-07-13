use alloc::{format, string::String, vec::Vec};

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpu_font::GpuFontTextRequest;
use crate::shell2::shell2_cmd::ParseOutcome;

const MIN_NATIVE_SCALE: u32 = 1;
const MAX_NATIVE_SCALE: u32 = 8;

struct FontCommand {
    rows: Vec<String>,
    multi_row: bool,
    native_scale: u32,
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
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
    match crate::intel::gpu_font::render_text_once(request, command.native_scale) {
        Ok(result) => print_shell_line(
            io,
            format!(
                "font: presented=1 layout={} text_chars={} rows={} native_scale={} native_size={}x{} glyphs={} vertices={} indices={} submits=1 completed={} ps={}",
                result.layout.name(),
                result.text_chars,
                result.rows,
                command.native_scale,
                64 * command.native_scale,
                64 * command.native_scale,
                result.summary.glyphs,
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
    let (multi_row, mut remaining) = if let Some(after_rows) = input.strip_prefix("rows") {
        if after_rows
            .chars()
            .next()
            .is_some_and(|ch| !ch.is_whitespace())
        {
            return Err("expected-quoted-text-or-rows");
        }
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

    let native_scale = parse_scale(remaining)?;
    Ok(FontCommand {
        rows,
        multi_row,
        native_scale,
    })
}

fn parse_quoted(input: &str) -> Result<(String, &str), &'static str> {
    let quoted = input.strip_prefix('"').ok_or("text-must-be-quoted")?;
    let mut text = String::new();
    let mut escaped = false;
    for (offset, ch) in quoted.char_indices() {
        if escaped {
            match ch {
                '"' | '\\' => text.push(ch),
                _ => return Err("unsupported-escape"),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let after_quote = 1 + offset + ch.len_utf8();
                return Ok((text, &input[after_quote..]));
            }
            ch => text.push(ch),
        }
    }
    if escaped {
        Err("unfinished-escape")
    } else {
        Err("missing-closing-quote")
    }
}

fn parse_scale(input: &str) -> Result<u32, &'static str> {
    let input = input.trim();
    if input.is_empty() {
        return Ok(crate::intel::render::FONT_STAMP_DEFAULT_NATIVE_SCALE);
    }
    let mut parts = input.split_whitespace();
    let scale = parts
        .next()
        .ok_or("scale-missing")?
        .parse::<u32>()
        .map_err(|_| "scale-invalid")?;
    if parts.next().is_some() {
        return Err("unexpected-argument");
    }
    if !(MIN_NATIVE_SCALE..=MAX_NATIVE_SCALE).contains(&scale) {
        return Err("scale-out-of-range-1-to-8");
    }
    Ok(scale)
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "font: usage `font \"text\" [scale 1..8]` or `font rows \"row 1\" \"row 2\" ... [scale 1..8]` (256 characters total)",
    );
}
