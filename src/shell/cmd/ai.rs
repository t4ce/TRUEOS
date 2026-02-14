use alloc::string::String;
use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use alloc::borrow::ToOwned;

use crate::shell::{CommandAction, ShellBackend};
use crate::shell::interface::ShellIo;
use crate::shell::output::ReverseOutput;
use crate::shell::statusbar;
use crate::v::net::https::post_https_json_async;

pub fn cmd_ai(_ctx: &mut crate::shell::cmd::registry::ShellCommandCtx, args: Option<&crate::shell::cmd::registry::ParsedArgs>) -> CommandAction {
    let first = args.and_then(|a| a.get_str(0)).unwrap_or("");
    let mut s: heapless::String<384> = heapless::String::new();
    let _ = s.push_str(first);
    CommandAction::OpenAiChat {
        first: s,
    }
}

pub async fn run_ai_wizard(
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    history: &mut Vec<String>,
    initial_prompt: &str,
) {
    const API_KEY: &str = "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";
    const URL: &str = "https://api.openai.com/v1/responses";
    
    // Ensure correct scroll region (Row 3..Bottom)
    crate::shell::output::apply_shell_scroll_region(io, term_rows);

    let out = ReverseOutput::new(io, term_cols, term_rows, history);

    if !initial_prompt.trim().is_empty() {
        {
             let mut echoed = String::new();
             echoed.push_str("ai> ");
             echoed.push_str(initial_prompt);
             out.echo_command(echoed.as_str());
        }
        process_input(&out, io, term_cols, term_rows, initial_prompt, URL, API_KEY).await;
    }

    let mut line = String::new();

    loop {
        // Position cursor at the prompt line (Row 2), clear it, and show prompt
        io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
        io.write_str(crate::ecma48::CLEAR_LINE);
        io.write_str("ai> ");
        io.write_str(&line);

        // Wait for input
        let b = loop {
             if let Some(Byte) = io.read_byte() {
                 break Byte;
             }
             Timer::after(Duration::from_millis(10)).await;
        };

        match b {
            b'\r' | b'\n' => {
                let input = line.trim().to_owned();
                line.clear();
                
                if input.eq_ignore_ascii_case("exit") {
                    break;
                }
                if !input.is_empty() {
                    {
                        let mut echoed = String::new();
                        echoed.push_str("ai> ");
                        echoed.push_str(&input);
                        out.echo_command(echoed.as_str());
                    }
                    process_input(&out, io, term_cols, term_rows, &input, URL, API_KEY).await;
                }
            }
            0x7F | 0x08 => { // Backspace
                     if !line.is_empty() {
                        line.pop();
                     }
            }
            c => {
                line.push(c as char);
            }
        }
    }
}

async fn process_input(
    out: &ReverseOutput<'_>,
    io: &dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    input: &str,
    url: &str,
    key: &str
) {
    let mut esc = String::new();
    json_escape_into(&mut esc, input);
    let body = alloc::format!(
        "{{\"model\": \"gpt-5.2\", \"input\": \"{}\", \"text\": {{\"verbosity\": \"medium\"}}, \"tools\": [{{ \"type\": \"web_search\" }}]}}",
        esc
    );

    // out.write_str("ai: thinking...\n");
    statusbar::set_right_active("thinking");
    statusbar::refresh(io, term_cols, term_rows);

    // 10s timeout, max 64KB response
    let res = post_https_json_async(url, body, Some(key), 10_000, 64 * 1024).await;
    
    statusbar::set_right_active("");
    statusbar::refresh(io, term_cols, term_rows);

    match res {
        Ok(bytes) => {
            if let Ok(s) = core::str::from_utf8(&bytes) {
                // OpenAI responses are JSON. We should try to extract the text.
                if let Some(content) = extract_json_string_field(s, "\"text\":") {
                    out.write_str("ai: ");
                    out.write_str(&content);
                    out.write_str("\n");
                } else {
                     // Fallback: print the whole thing
                     out.write_str("ai: [raw] ");
                     out.write_str(s);
                     out.write_str("\n");
                }
            } else {
                out.write_str("ai: error decoding utf8\n");
            }
        }
        Err(e) => {
            out.write_fmt(format_args!("ai: network error {:?}\n", e));
        }
    }
}

fn json_escape_into(out: &mut String, s: &str) {
    use core::fmt::Write;
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
}

fn extract_json_string_field(json: &str, needle: &str) -> Option<String> {
    let i = json.find(needle)?;
    let mut j = i + needle.len();
    while j < json.len() {
        let b = json.as_bytes()[j];
        if b == b'"' { break; }
        if b != b' ' && b != b':' && b != b'\n' && b != b'\r' { return None; }
        j += 1;
    }
    if j >= json.len() { return None; }
    
    j += 1;
    let bytes = json.as_bytes();
    let mut out = String::new();
    while j < bytes.len() {
        let b = bytes[j];
        if b == b'"' { return Some(out); }
        if b == b'\\' {
            j += 1;
            if j >= bytes.len() { break; }
            match bytes[j] {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => {
                    // Primitive \uXXXX support
                    if j + 4 < bytes.len() {
                        if let Ok(hex_str) = core::str::from_utf8(&bytes[j+1..j+5]) {
                             if let Ok(cp) = u32::from_str_radix(hex_str, 16) {
                                 if let Some(ch) = core::char::from_u32(cp) {
                                     out.push(ch);
                                     j += 4;
                                 }
                             }
                        }
                    }
                }
                _ => {}
            }
            j += 1;
            continue;
        }
        out.push(b as char);
        j += 1;
    }
    None
}

