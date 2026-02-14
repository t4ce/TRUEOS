use alloc::string::String;
use alloc::vec::Vec;
use embassy_time::{Duration, Timer};

use crate::shell::{CommandAction, ShellBackend};
use crate::shell::interface::ShellIo;
use crate::v::net::wss::WssConnection;
use crate::shell::output::ReverseOutput;

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
    const MODEL: &str = "gpt-5.2";
    const API_KEY: &str = "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";
    const BETA_HDR: &str = "OpenAI-Beta: realtime=v1";

    let out = ReverseOutput::new(io, term_cols, term_rows, history);
    
    // Initial status
    out.write_str("ai: connecting to ");
    out.write_str(MODEL);
    out.write_str("... (Ctrl-C to disconnect)\n");

    let url = alloc::format!("wss://api.openai.com/v1/realtime?model={}", MODEL);
    let auth = alloc::format!("Authorization: Bearer {}", API_KEY);
    let headers: [&str; 2] = [auth.as_str(), BETA_HDR];

    let mut wss = match WssConnection::connect_with_headers(url.as_str(), &headers).await {
        Ok(c) => c,
        Err(e) => {
            out.write_fmt(format_args!("ai: connection failed {:?}\n", e));
            return;
        }
    };
    
    // Session Init
    let session_update = "{\"type\":\"session.update\",\"session\":{\"modalities\":[\"text\"],\"instructions\":\"You are a concise shell assistant running inside TRUEOS.\"}}";
    if let Err(_) = wss.send(session_update) {
         out.write_str("ai: session handshake failed\n");
    }

    out.write_str("ai: connected\n");
    
    // If there's an initial prompt, send it
    if !initial_prompt.trim().is_empty() {
        send_user_message(&mut wss, initial_prompt).await;
    }

    io.write_str("ai> ");

    let mut line = String::new();
    let mut response_buf = String::new();
    let mut saw_cr = false;

    loop {
        // 1. Network Processing
        while let Some(frame) = wss.recv() {
            if frame.contains("\"type\":\"response.create\"") {
                response_buf.clear();
            }

            if let Some(delta) = extract_json_string_field(&frame, "\"delta\":")
                .or_else(|| extract_json_string_field(&frame, "\"text\":"))
            {
                response_buf.push_str(&delta);
            }

            if frame.contains("\"type\":\"response.output_text.done\"")
                || frame.contains("\"type\":\"response.done\"")
                || frame.contains("\"type\":\"response.completed\"")
            {
                if !response_buf.trim().is_empty() {
                    log_entry(&out, "ai: ", response_buf.trim());
                    response_buf.clear();
                    io.write_str("ai> ");
                }
            }
        }

        // 2. Input Processing
        if let Some(b) = io.read_byte() {
            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';

            match b {
                0x03 => { // Ctrl-C
                    out.write_str("ai: disconnected\n");
                    return;
                }
                b'\r' | b'\n' => {
                    let input = String::from(line.trim());
                    line.clear();
                    // Do NOT print newline to IO if we want to keep prompt fixed?
                    // Usually enter prints newline.
                    // "user curser stays in promt row"
                    // If we print \r\n, we go down.
                    // If we clear line and reprint prompt, we stay.
                    
                    io.write_str("\r"); // CR
                    io.write_str(crate::ecma48::CLEAR_LINE);
                    
                    if input.eq_ignore_ascii_case(".exit") || input.eq_ignore_ascii_case("exit") {
                        out.write_str("ai: bye\n");
                        return;
                    }

                    if !input.is_empty() {
                        log_entry(&out, "you: ", &input);
                        
                        send_user_message(&mut wss, &input).await;
                        response_buf.clear();
                    }
                    
                    io.write_str("ai> ");
                }
                0x08 | 0x7F => { // Backspace
                    if !line.is_empty() {
                        line.pop();
                        io.write_str("\x08 \x08");
                    }
                }
                c => {
                    line.push(c as char);
                    io.write_byte(c);
                }
            }
        } else {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}


fn log_entry(out: &ReverseOutput, prefix: &str, msg: &str) {
    if msg.is_empty() { return; }
    // ReverseOutput expects simple strings and handles alignment/scrolling
    out.write_str(prefix);
    out.write_str(msg);
    out.write_str("\n");
}

async fn send_user_message(wss: &mut WssConnection, text: &str) {
    let mut esc = String::new();
    json_escape_into(&mut esc, text);
    let msg = alloc::format!(
        "{{\"type\":\"conversation.item.create\",\"item\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}}}",
        esc
    );
    let _ = wss.send(&msg);
    let _ = wss.send("{\"type\":\"response.create\"}");
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
    if !json.as_bytes().get(j).copied().is_some_and(|b| b == b'"') { return None; }
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

