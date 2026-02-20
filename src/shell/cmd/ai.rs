use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use spin::Mutex;

use crate::wait::{Either, select2};

use crate::shell::aihttps::{SseHandler, post_json_async, post_sse_async};
use crate::shell::interface::ShellIo;
use crate::shell::output::ReverseOutput;
use crate::shell::statusbar;
use crate::shell::{CommandAction, ShellBackend, ShellMode};

fn log_utf8_chunks(prefix: &str, s: &str) {
    // Avoid log-line truncation by splitting into multiple lines.
    // Keep boundaries on UTF-8 char boundaries.
    const CHUNK: usize = 512;
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let mut end = (i + CHUNK).min(bytes.len());
        while end > i && !s.is_char_boundary(end) {
            end -= 1;
        }
        if end == i {
            // Should not happen for valid UTF-8, but avoid infinite loop.
            end = (i + 1).min(bytes.len());
        }
        crate::log!("{}{}\n", prefix, &s[i..end]);
        i = end;
    }
}

fn sse_event_type(data: &str) -> Option<String> {
    // SSE payloads from Responses API are JSON with a top-level: {"type":"..."}
    // Prefer the first match (the event type) rather than nested "type" fields.
    extract_json_string_field(data, "\"type\":")
}

fn should_log_sse_data(event_type: Option<&str>) -> bool {
    // Full SSE payload logging is very noisy and can interfere with the shell UI.
    // Keep a useful subset by default.
    const VERBOSE: bool = false;
    if VERBOSE {
        return true;
    }
    matches!(
        event_type,
        Some("response.completed") | Some("response.error")
    )
}

// Preserve multi-turn conversation state across `ai` invocations.
// Best-effort, resets on reboot.
static AI_PREVIOUS_RESPONSE_ID: Mutex<Option<String>> = Mutex::new(None);

fn ai_prev_get() -> Option<String> {
    AI_PREVIOUS_RESPONSE_ID.lock().clone()
}

fn ai_prev_set(v: Option<String>) {
    *AI_PREVIOUS_RESPONSE_ID.lock() = v;
}

#[inline]
fn draw_ai_prompt_row(io: &dyn ShellIo, line: &str) {
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!(
        "{}",
        crate::ecma48::color("§ ", crate::shell::PROMPT_RGB)
    ));
    // Ensure typed text is never tinted by prior row styling.
    io.write_str("\x1b[0m");
    io.write_str(line);
}

#[inline]
fn ai_status_wave(
    io: &dyn ShellIo,
    term_cols: usize,
    term_rows: usize,
    slot_id: u8,
    center: &str,
    step: usize,
    active: u8,
    idle: u8,
) {
    let _ = statusbar::set_center(slot_id, center);
    let _ = statusbar::set_right(slot_id, center);
    for i in 0..statusbar::INDICATOR_COUNT {
        let code = if i == (step % statusbar::INDICATOR_COUNT) {
            active
        } else {
            idle
        };
        let _ = statusbar::set_indicator(slot_id, i, code);
    }
    statusbar::refresh(io, term_cols, term_rows);
}

#[inline]
fn ai_status_solid(
    io: &dyn ShellIo,
    term_cols: usize,
    term_rows: usize,
    slot_id: u8,
    center: &str,
    color: u8,
) {
    let _ = statusbar::set_center(slot_id, center);
    let _ = statusbar::set_right(slot_id, center);
    for i in 0..statusbar::INDICATOR_COUNT {
        let _ = statusbar::set_indicator(slot_id, i, color);
    }
    statusbar::refresh(io, term_cols, term_rows);
}

async fn read_next_byte(io: &dyn ShellBackend) -> u8 {
    loop {
        if let Some(byte) = io.read_byte() {
            return byte;
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}

pub fn cmd_ai(
    _ctx: &mut crate::shell::cmd::registry::ShellCommandCtx,
    args: Option<&crate::shell::cmd::registry::ParsedArgs>,
) -> CommandAction {
    let first = args.and_then(|a| a.get_str(0)).unwrap_or("");
    let mut s: heapless::String<384> = heapless::String::new();
    let _ = s.push_str(first);
    CommandAction::OpenAiChat { first: s }
}

pub async fn run_ai_wizard(
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    spawner: &Spawner,
    history: &mut Vec<String>,
    initial_prompt: &str,
) {
    const API_KEY: &str = "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";
    const URL: &str = "https://api.openai.com/v1/responses";

    // Ensure correct scroll region (Row 3..Bottom)
    crate::shell::output::apply_shell_scroll_region(io, term_rows);
    // When entering AI mode, keep prompt row clean (`§ ` only).
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 1)));
    io.write_str(crate::ecma48::CLEAR_LINE);
    io.write_str(crate::ecma48::CURSOR_BLINKING_BLOCK);
    io.write_fmt(format_args!(
        "{}",
        crate::ecma48::color("§ ", crate::shell::PROMPT_RGB)
    ));

    let slot = match crate::matrix::alloc_slot("ai") {
        Some(s) => s,
        None => {
            // Best-effort: still run without statusbar.
            255
        }
    };
    if slot != 255 {
        let _ = statusbar::set_active_slot(slot);
        let _ = statusbar::set_left(slot, "ai");
        let _ = statusbar::set_right(slot, "idle");
        let _ = statusbar::set_center(slot, "ready");
        for i in 0..statusbar::INDICATOR_COUNT {
            let _ = statusbar::set_indicator(slot, i, 0);
        }
        statusbar::refresh(io, term_cols, term_rows);
    }

    let out = ReverseOutput::new(io, term_cols, term_rows, history);

    let mut previous_response_id: Option<String> = ai_prev_get();
    let mut pending_shell_outputs: Vec<ShellCallOutput> = Vec::new();
    let mut active_request: Option<
        core::pin::Pin<alloc::boxed::Box<dyn core::future::Future<Output = ()> + '_>>,
    > = None;

    if !initial_prompt.trim().is_empty() {
        // Do not echo here: the outer shell already echoed `ai <initial_prompt>`.
        active_request = Some(alloc::boxed::Box::pin(process_input(
            &out,
            io,
            term_cols,
            term_rows,
            slot,
            spawner,
            String::from(initial_prompt),
            URL,
            API_KEY,
            &mut previous_response_id,
            &mut pending_shell_outputs,
        )));
    }

    let mut line = String::new();

    loop {
        // Position cursor at the prompt line (Row 2), clear it, and show prompt
        draw_ai_prompt_row(io, line.as_str());

        let b = if let Some(req) = active_request.as_mut() {
            match select2(req.as_mut(), read_next_byte(io)).await {
                Either::First(()) => {
                    active_request = None;
                    continue;
                }
                Either::Second(byte) => byte,
            }
        } else {
            read_next_byte(io).await
        };

        match b {
            b'\r' | b'\n' => {
                // Many serial/bridge backends deliver CRLF. We already consumed the first
                // byte (CR or LF) to submit the prompt; if it was CR, discard a following LF
                // so the cancel-future doesn't immediately see it and abort the request.
                if b == b'\r' {
                    if let Some(next) = io.read_byte() {
                        if next != b'\n' {
                            // Unexpected: we can't un-read. Best-effort: ignore.
                        }
                    }
                }
                let input = line.trim().to_owned();
                line.clear();

                if input.eq_ignore_ascii_case("exit") {
                    ai_status_solid(io, term_cols, term_rows, slot, "exit", 0);
                    active_request = None;
                    break;
                }
                if !input.is_empty() {
                    out.echo_user_text(input.as_str());
                    // Keep row 2 clean while the request is in-flight.
                    draw_ai_prompt_row(io, "");
                    // Replace in-flight request with the new one.
                    if active_request.is_some() {
                        ai_status_solid(io, term_cols, term_rows, slot, "replaced", 3);
                    }
                    active_request = None;
                    active_request = Some(alloc::boxed::Box::pin(process_input(
                        &out,
                        io,
                        term_cols,
                        term_rows,
                        slot,
                        spawner,
                        input,
                        URL,
                        API_KEY,
                        &mut previous_response_id,
                        &mut pending_shell_outputs,
                    )));
                }
            }
            0x7F | 0x08 => {
                // Backspace
                if !line.is_empty() {
                    line.pop();
                }
            }
            c => {
                line.push(c as char);
            }
        }
    }

    if slot != 255 {
        let _ = statusbar::set_active_slot(u8::MAX);
        let _ = crate::matrix::free_slot(slot);
        statusbar::refresh(io, term_cols, term_rows);
    }
}

async fn process_input(
    out: &ReverseOutput<'_>,
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    slot_id: u8,
    spawner: &Spawner,
    input: String,
    url: &str,
    key: &str,
    previous_response_id: &mut Option<String>,
    pending_shell_outputs: &mut Vec<ShellCallOutput>,
) {
    let force_tools = input.trim_start().starts_with('!');
    // Streaming is for normal chat text only; tool turns need the full JSON response.
    let stream = !force_tools
        && pending_shell_outputs.is_empty()
        && !looks_like_execute_request(input.as_str());

    let body = build_openai_request_body(
        input.as_str(),
        previous_response_id.as_deref(),
        pending_shell_outputs,
        stream,
    );

    crate::log!("ai: request_json_begin len={}\n", body.len());
    log_utf8_chunks("ai: request_json: ", body.as_str());
    crate::log!("ai: request_json_end\n");

    statusbar::set_right_active(if stream { "streaming" } else { "thinking" });
    ai_status_wave(
        io,
        term_cols,
        term_rows,
        slot_id,
        if stream { "sse.open" } else { "json.req" },
        0,
        3,
        0,
    );
    statusbar::refresh(io, term_cols, term_rows);

    // Increase timeout to 120s since web search can be slow.
    if stream {
        struct AiStream<'a, 'b> {
            out: &'a ReverseOutput<'b>,
            io: &'static dyn ShellBackend,
            term_cols: usize,
            term_rows: usize,
            slot_id: u8,
            started: bool,
            last_full: String,
            saw_delta: bool,
            pulse: usize,
        }

        impl<'a, 'b> SseHandler for AiStream<'a, 'b> {
            fn on_data(&mut self, data: &str) {
                let et = sse_event_type(data);
                if should_log_sse_data(et.as_deref()) {
                    crate::log!("ai: sse_data_begin len={}\n", data.len());
                    log_utf8_chunks("ai: sse_data: ", data);
                    crate::log!("ai: sse_data_end\n");
                }

                // Update conversation id as early as possible.
                if let Some(id) = extract_response_id(data) {
                    ai_prev_set(Some(id));
                }

                if let Some(t) = et.as_deref() {
                    match t {
                        "response.created" => {
                            self.pulse = self.pulse.wrapping_add(1);
                            ai_status_wave(
                                self.io,
                                self.term_cols,
                                self.term_rows,
                                self.slot_id,
                                "evt.create",
                                self.pulse,
                                4,
                                0,
                            );
                        }
                        "response.output_text.delta" => {
                            self.pulse = self.pulse.wrapping_add(1);
                            ai_status_wave(
                                self.io,
                                self.term_cols,
                                self.term_rows,
                                self.slot_id,
                                "evt.delta",
                                self.pulse,
                                5,
                                0,
                            );
                        }
                        "response.completed" => {
                            ai_status_solid(
                                self.io,
                                self.term_cols,
                                self.term_rows,
                                self.slot_id,
                                "evt.done",
                                2,
                            );
                        }
                        "response.error" => {
                            ai_status_solid(
                                self.io,
                                self.term_cols,
                                self.term_rows,
                                self.slot_id,
                                "evt.error",
                                1,
                            );
                        }
                        _ => {}
                    }
                }

                // Prefer true deltas.
                let delta = extract_json_string_field(data, "\"delta\":");
                // Fallback: some frames carry cumulative output.
                let full = extract_json_string_field(data, "\"output_text\":")
                    .or_else(|| extract_json_string_field(data, "\"text\":"));

                if let Some(d) = delta {
                    if !self.started {
                        self.out.write_live_fragment_wrapped("ai: ");
                        self.started = true;
                    }
                    self.saw_delta = true;
                    let clean = sanitize_term_text(d.as_str());
                    self.out.write_live_fragment_wrapped(clean.as_str());
                    return;
                }

                // If we already printed deltas, ignore done/full frames to avoid duplicating output.
                // The API commonly sends both `...delta` and `...done` with the full text.
                if self.saw_delta {
                    return;
                }

                if let Some(f) = full {
                    // Some providers emit repeated cumulative "full" frames.
                    // Drop exact repeats to avoid visual duplicates.
                    if f == self.last_full {
                        return;
                    }

                    // Print only the new suffix versus last_full to avoid repeats.
                    if f.len() >= self.last_full.len() && f.starts_with(self.last_full.as_str()) {
                        let suffix = &f[self.last_full.len()..];
                        if !suffix.is_empty() {
                            if !self.started {
                                self.out.write_live_fragment_wrapped("ai: ");
                                self.started = true;
                            }
                            let clean = sanitize_term_text(suffix);
                            self.out.write_live_fragment_wrapped(clean.as_str());
                        }
                    } else {
                        // If it doesn't look like an append, reset and print full.
                        if !self.started {
                            self.out.write_live_fragment_wrapped("ai: ");
                            self.started = true;
                        }
                        let clean = sanitize_term_text(f.as_str());
                        self.out.write_live_fragment_wrapped(clean.as_str());
                    }
                    self.last_full = f;
                }
            }
        }

        let mut sink = AiStream {
            out,
            io,
            term_cols,
            term_rows,
            slot_id,
            started: false,
            last_full: String::new(),
            saw_delta: false,
            pulse: 0,
        };

        let result = post_sse_async(url, body, Some(key), 120_000, 256 * 1024, &mut sink).await;

        // Clear status
        statusbar::set_right_active(if result.is_ok() { "done" } else { "" });
        statusbar::refresh(io, term_cols, term_rows);

        match result {
            Ok(()) => {
                // Sync local state from the persisted id.
                *previous_response_id = ai_prev_get();
                if sink.started {
                    out.write_str("\n");
                }
                ai_status_solid(io, term_cols, term_rows, slot_id, "ready", 2);
            }
            Err(e) => {
                ai_status_solid(io, term_cols, term_rows, slot_id, "net.err", 1);
                out.write_fmt(format_args!("ai: network error {:?}\n", e));
            }
        }
        return;
    }

    let result = post_json_async(url, body, Some(key), 120_000, 256 * 1024).await;

    // Clear status
    statusbar::set_right_active(if result.is_ok() { "done" } else { "" });
    statusbar::refresh(io, term_cols, term_rows);

    match result {
        Ok(bytes) => {
            if let Ok(s) = core::str::from_utf8(&bytes) {
                crate::log!("ai: response_json_begin len={}\n", s.len());
                log_utf8_chunks("ai: response_json: ", s);
                crate::log!("ai: response_json_end\n");

                // Track response id for multi-turn and for tool follow-ups.
                if let Some(id) = extract_response_id(s) {
                    *previous_response_id = Some(id);
                    ai_prev_set(previous_response_id.clone());
                }

                // If the model requested shell execution, run it locally and submit outputs.
                let shell_calls = extract_shell_calls(s);
                if !shell_calls.is_empty() {
                    pending_shell_outputs.clear();
                    for call in shell_calls {
                        out.write_str("ai: shell_call\n");
                        for cmd in call.commands.iter() {
                            out.write_str("  $ ");
                            out.write_str(cmd);
                            out.write_str("\n");
                        }
                        let out_item =
                            run_shell_call(spawner, io, term_cols, term_rows, call).await;

                        // Echo a clipped summary so the user can see that something happened.
                        for entry in out_item.output.iter() {
                            if !entry.stdout.trim().is_empty() {
                                out.write_str("ai: [stdout] ");
                                out.write_str(&clip_visible(entry.stdout.as_str(), 1024));
                                out.write_str("\n");
                            }
                            if !entry.stderr.trim().is_empty() {
                                out.write_str("ai: [stderr] ");
                                out.write_str(&clip_visible(entry.stderr.as_str(), 512));
                                out.write_str("\n");
                            }
                        }
                        pending_shell_outputs.push(out_item);
                    }

                    // Try to immediately continue by sending shell_call_output back.
                    let followup_body = build_openai_request_body(
                        "",
                        previous_response_id.as_deref(),
                        pending_shell_outputs,
                        false,
                    );

                    crate::log!(
                        "ai: followup_request_json_begin len={}\n",
                        followup_body.len()
                    );
                    log_utf8_chunks("ai: followup_request_json: ", followup_body.as_str());
                    crate::log!("ai: followup_request_json_end\n");

                    let follow =
                        post_json_async(url, followup_body, Some(key), 120_000, 256 * 1024).await;

                    match follow {
                        Ok(bytes) => {
                            if let Ok(s2) = core::str::from_utf8(&bytes) {
                                crate::log!("ai: followup_response_json_begin len={}\n", s2.len());
                                log_utf8_chunks("ai: followup_response_json: ", s2);
                                crate::log!("ai: followup_response_json_end\n");

                                if let Some(id2) = extract_response_id(s2) {
                                    *previous_response_id = Some(id2);
                                    ai_prev_set(previous_response_id.clone());
                                }
                                pending_shell_outputs.clear();
                                write_openai_text(out, s2);
                                return;
                            }
                        }
                        Err(e) => {
                            out.write_fmt(format_args!(
                                        "ai: tool follow-up network error {:?} (will retry on next prompt)\n",
                                        e
                                    ));
                        }
                    }

                    // If we couldn't complete the follow-up, keep outputs stashed.
                    out.write_str(
                        "ai: shell outputs captured (will be sent with your next prompt)\n",
                    );
                    return;
                }

                // Debug aid: if the user likely wanted command execution but we got no tool call.
                if looks_like_execute_request(input.as_str()) {
                    out.write_str("ai: note: model did not issue shell_call for this prompt\n");
                    out.write_str(
                        "ai: tip: prefix with '!' to force tool usage (example: !tlb.pci)\n",
                    );
                }

                // Normal text response.
                pending_shell_outputs.clear();
                write_openai_text(out, s);
                ai_status_solid(io, term_cols, term_rows, slot_id, "ready", 2);
            } else {
                ai_status_solid(io, term_cols, term_rows, slot_id, "utf8.err", 1);
                out.write_str("ai: error decoding utf8\n");
            }
        }
        Err(e) => {
            ai_status_solid(io, term_cols, term_rows, slot_id, "net.err", 1);
            out.write_fmt(format_args!("ai: network error {:?}\n", e));
        }
    }
}

fn write_openai_text(out: &ReverseOutput<'_>, json: &str) {
    // Prefer Responses API convenience field.
    if let Some(content) = extract_json_string_field(json, "\"output_text\":") {
        out.write_str("ai: ");
        let clean = sanitize_term_text(content.as_str());
        out.write_str(clean.as_str());
        out.write_str("\n");
        return;
    }

    // Fallback: some responses embed a "text" field in output items.
    if let Some(content) = extract_json_string_field(json, "\"text\":") {
        out.write_str("ai: ");
        let clean = sanitize_term_text(content.as_str());
        out.write_str(clean.as_str());
        out.write_str("\n");
        return;
    }

    out.write_str("ai: [raw] ");
    out.write_str(json);
    out.write_str("\n");
}

fn sanitize_term_text(s: &str) -> String {
    // Some backends are not UTF-8 clean and will display mojibake like "â" for
    // punctuation (e.g. em dashes). Keep output readable by mapping common
    // punctuation to ASCII.
    let mut out = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i] == 'â' {
            // Common UTF-8 mojibake triplets can show up as:
            // - â€X (where second char is U+20AC)
            // - â\u{0080}X (where second char is C1 control U+0080)
            let c2 = chars[i + 1];
            if c2 == '€' || c2 == '\u{0080}' {
                match chars[i + 2] {
                    // em/en dash
                    '”' | '“' | '\u{0094}' | '\u{0093}' => {
                        out.push('-');
                        i += 3;
                        continue;
                    }
                    // right single quote / apostrophe
                    '™' | '\u{0099}' => {
                        out.push('\'');
                        i += 3;
                        continue;
                    }
                    // left/right double quote
                    'œ' | '' | '\u{009C}' | '\u{009D}' | '�' => {
                        out.push('"');
                        i += 3;
                        continue;
                    }
                    // ellipsis
                    '¦' | '\u{00A6}' => {
                        out.push_str("...");
                        i += 3;
                        continue;
                    }
                    _ => {}
                }
            }
        }

        let ch = chars[i];
        match ch {
            '−' => out.push('-'),
            '—' | '–' => out.push('-'),
            '…' => out.push_str("..."),
            '’' => out.push('\''),
            '“' | '”' => out.push('"'),
            '\u{00a0}' => out.push(' '),
            c if c.is_ascii() => out.push(c),
            _ => out.push('?'),
        }
        i += 1;
    }
    out
}

#[derive(Clone, Debug)]
struct ShellCall {
    call_id: String,
    commands: Vec<String>,
    timeout_ms: u64,
    max_output_length: usize,
}

#[derive(Clone, Debug)]
struct ShellCallOutputEntry {
    stdout: String,
    stderr: String,
    outcome_type: &'static str,
    exit_code: Option<i32>,
}

#[derive(Clone, Debug)]
struct ShellCallOutput {
    call_id: String,
    max_output_length: usize,
    output: Vec<ShellCallOutputEntry>,
}

fn build_openai_request_body(
    user_text: &str,
    previous_response_id: Option<&str>,
    pending_shell_outputs: &[ShellCallOutput],
    stream: bool,
) -> String {
    // Tools: enable local shell execution and keep web_search available.
    // If `!` is used (force_tools), expose ONLY shell so tool_choice=required can't pick web_search.
    let tools_auto =
        "[{\"type\":\"shell\",\"environment\":{\"type\":\"local\"}},{\"type\":\"web_search\"}]";
    let tools_shell_only = "[{\"type\":\"shell\",\"environment\":{\"type\":\"local\"}}]";

    // Input items may include pending tool outputs first (to complete the tool turn),
    // then the user message (if provided).
    let mut input_items: Vec<String> = Vec::new();

    for item in pending_shell_outputs {
        input_items.push(shell_call_output_item_json(item));
    }

    let force_tools = user_text.trim_start().starts_with('!');
    let user_text = if force_tools {
        user_text.trim_start().trim_start_matches('!').trim_start()
    } else {
        user_text
    };

    if !user_text.trim().is_empty() {
        let mut esc = String::new();
        json_escape_into(&mut esc, user_text);
        input_items.push(alloc::format!(
            "{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}",
            esc
        ));
    }

    let input_json = alloc::format!("[{}]", join_json_array(&input_items));

    let instructions = "You are running inside TRUEOS (not Linux). Use the shell tool to execute TRUEOS shell commands when asked to run commands or to verify facts. Commands must be non-interactive, except `tetris` when the user explicitly asks to launch it. Prefer read-only inspection commands. Avoid destructive commands as you are close to ring0. Avoid GNU/Linux-specific commands and flags (e.g. man, head, which, redirections like 2>/dev/null, and complex pipelines) unless you have verified they exist in TRUEOS. For streamed text replies, emit small incremental chunks frequently instead of buffering long paragraphs.";

    let mut esc_instructions = String::new();
    json_escape_into(&mut esc_instructions, instructions);

    let mut body = String::new();
    body.push_str("{\"model\":\"gpt-5.2-pro-2025-12-11\",");
    if stream {
        body.push_str("\"stream\":true,");
        // Reduce SSE payload overhead on trusted links.
        body.push_str("\"stream_options\":{\"include_obfuscation\":false},");
    }
    body.push_str("\"truncation\":\"auto\",");
    if force_tools {
        body.push_str("\"tool_choice\":\"required\",");
    } else {
        body.push_str("\"tool_choice\":\"auto\",");
    }
    body.push_str("\"text\":{\"verbosity\":\"medium\"},");
    body.push_str("\"instructions\":\"");
    body.push_str(&esc_instructions);
    body.push_str("\",");
    body.push_str("\"tools\":");
    if force_tools {
        body.push_str(tools_shell_only);
    } else {
        body.push_str(tools_auto);
    }
    body.push_str(",\"input\":");
    body.push_str(&input_json);

    if let Some(prev) = previous_response_id {
        let mut esc_prev = String::new();
        json_escape_into(&mut esc_prev, prev);
        body.push_str(",\"previous_response_id\":\"");
        body.push_str(&esc_prev);
        body.push_str("\"");
    }

    body.push_str("}");
    body
}

fn extract_response_id(json: &str) -> Option<String> {
    // Responses API returns top-level: "id": "resp_..." (note the spaces).
    // We also see nested "id" fields (e.g. msg_), so only accept resp_ ids.
    let bytes = json.as_bytes();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        // Match "id"
        if bytes[i] == b'"'
            && bytes.get(i + 1) == Some(&b'i')
            && bytes.get(i + 2) == Some(&b'd')
            && bytes.get(i + 3) == Some(&b'"')
        {
            let mut j = i + 4;
            // Skip whitespace
            while j < bytes.len()
                && (bytes[j] == b' ' || bytes[j] == b'\n' || bytes[j] == b'\r' || bytes[j] == b'\t')
            {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b':' {
                i += 1;
                continue;
            }
            j += 1;
            while j < bytes.len()
                && (bytes[j] == b' ' || bytes[j] == b'\n' || bytes[j] == b'\r' || bytes[j] == b'\t')
            {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b'"' {
                i += 1;
                continue;
            }
            j += 1;
            let start = j;
            while j < bytes.len() && bytes[j] != b'"' {
                j += 1;
            }
            if j >= bytes.len() {
                return None;
            }
            let id_bytes = &bytes[start..j];
            if id_bytes.starts_with(b"resp_") {
                let id_str = core::str::from_utf8(id_bytes).ok()?;
                return Some(String::from(id_str));
            }
        }
        i += 1;
    }
    None
}

fn looks_like_execute_request(s: &str) -> bool {
    let t = s.trim_start();
    if t.starts_with('!') {
        return true;
    }
    let lower = t.as_bytes();
    // Minimal keyword scan without allocating.
    // (We don't need full Unicode case-folding here.)
    fn contains_ascii(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle))
    }
    contains_ascii(lower, b"execute")
        || contains_ascii(lower, b"run ")
        || contains_ascii(lower, b"shell")
}

fn clip_visible(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut n = 0usize;
    for ch in s.chars() {
        if n >= max_chars {
            out.push_str("…");
            break;
        }
        out.push(ch);
        n += 1;
    }
    out
}

fn join_json_array(items: &[String]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        out.push_str(item);
    }
    out
}

fn shell_call_output_item_json(item: &ShellCallOutput) -> String {
    let mut esc_call_id = String::new();
    json_escape_into(&mut esc_call_id, item.call_id.as_str());

    // Responses API expects `output` to be an array of objects (content parts).
    // Keep it compact but informative.
    let mut combined = String::new();
    for (idx, entry) in item.output.iter().enumerate() {
        if idx != 0 {
            combined.push_str("\n");
        }
        if !entry.stdout.is_empty() {
            combined.push_str("[stdout]\n");
            combined.push_str(entry.stdout.as_str());
            if !entry.stdout.ends_with('\n') {
                combined.push('\n');
            }
        }
        if !entry.stderr.is_empty() {
            combined.push_str("[stderr]\n");
            combined.push_str(entry.stderr.as_str());
            if !entry.stderr.ends_with('\n') {
                combined.push('\n');
            }
        }
        match (entry.outcome_type, entry.exit_code) {
            ("exit", Some(code)) => {
                combined.push_str("[exit_code] ");
                combined.push_str(&alloc::format!("{}\n", code));
            }
            ("timeout", _) => combined.push_str("[timeout]\n"),
            _ => {}
        }
    }
    if combined.len() > item.max_output_length {
        combined.truncate(item.max_output_length);
    }

    let mut esc_output = String::new();
    json_escape_into(&mut esc_output, combined.as_str());

    alloc::format!(
        "{{\"type\":\"shell_call_output\",\"call_id\":\"{}\",\"output\":[{{\"type\":\"output_text\",\"text\":\"{}\"}}]}}",
        esc_call_id,
        esc_output
    )
}

fn extract_shell_calls(json: &str) -> Vec<ShellCall> {
    let mut out: Vec<ShellCall> = Vec::new();
    let mut idx: usize = 0;

    loop {
        let hay = &json[idx..];
        let rel = find_any(
            hay,
            &["\"type\":\"shell_call\"", "\"type\": \"shell_call\""],
        );
        let Some(rel_pos) = rel else { break };
        let pos = idx + rel_pos;

        let call_id = match extract_json_string_field_from(json, pos, "\"call_id\":") {
            Some(v) => v,
            None => {
                idx = pos + 10;
                continue;
            }
        };

        let commands =
            extract_json_string_array_field_from(json, pos, "\"commands\":").unwrap_or_default();
        let timeout_ms =
            extract_json_u64_field_from(json, pos, "\"timeout_ms\":").unwrap_or(60_000);
        let max_output_length = extract_json_usize_field_from(json, pos, "\"max_output_length\":")
            .unwrap_or(4096)
            .min(64 * 1024);

        out.push(ShellCall {
            call_id,
            commands,
            timeout_ms,
            max_output_length,
        });

        idx = pos + 10;
    }

    out
}

fn find_any(hay: &str, needles: &[&str]) -> Option<usize> {
    let mut best: Option<usize> = None;
    for n in needles {
        if let Some(p) = hay.find(n) {
            best = match best {
                None => Some(p),
                Some(b) => Some(b.min(p)),
            };
        }
    }
    best
}

fn extract_json_string_field_from(json: &str, start: usize, needle: &str) -> Option<String> {
    let sub = &json[start..];
    let rel = sub.find(needle)?;
    extract_json_string_field(&sub[rel..], needle)
}

fn extract_json_u64_field_from(json: &str, start: usize, needle: &str) -> Option<u64> {
    let sub = &json[start..];
    let rel = sub.find(needle)?;
    let mut i = rel + needle.len();

    while i < sub.len() {
        let b = sub.as_bytes()[i];
        if (b'0'..=b'9').contains(&b) {
            break;
        }
        i += 1;
    }
    let mut j = i;
    while j < sub.len() {
        let b = sub.as_bytes()[j];
        if !(b'0'..=b'9').contains(&b) {
            break;
        }
        j += 1;
    }
    sub[i..j].parse::<u64>().ok()
}

fn extract_json_usize_field_from(json: &str, start: usize, needle: &str) -> Option<usize> {
    extract_json_u64_field_from(json, start, needle).and_then(|v| usize::try_from(v).ok())
}

fn extract_json_string_array_field_from(
    json: &str,
    start: usize,
    needle: &str,
) -> Option<Vec<String>> {
    let sub = &json[start..];
    let rel = sub.find(needle)?;
    let mut i = rel + needle.len();
    while i < sub.len() {
        let b = sub.as_bytes()[i];
        if b == b'[' {
            break;
        }
        i += 1;
    }
    if i >= sub.len() || sub.as_bytes()[i] != b'[' {
        return None;
    }
    i += 1;

    let mut out: Vec<String> = Vec::new();
    let bytes = sub.as_bytes();
    while i < bytes.len() {
        while i < bytes.len()
            && (bytes[i] == b' '
                || bytes[i] == b'\n'
                || bytes[i] == b'\r'
                || bytes[i] == b'\t'
                || bytes[i] == b',')
        {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        if bytes[i] == b']' {
            return Some(out);
        }
        if bytes[i] != b'"' {
            // Unexpected token, bail.
            return Some(out);
        }
        i += 1;

        let mut s = String::new();
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' {
                i += 1;
                break;
            }
            if b == b'\\' {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
                match bytes[i] {
                    b'"' => s.push('"'),
                    b'\\' => s.push('\\'),
                    b'/' => s.push('/'),
                    b'n' => s.push('\n'),
                    b'r' => s.push('\r'),
                    b't' => s.push('\t'),
                    _ => {}
                }
                i += 1;
                continue;
            }

            s.push(b as char);
            i += 1;
        }

        out.push(s);
    }

    Some(out)
}

struct CaptureBackend {
    buf: spin::Mutex<String>,
}

impl CaptureBackend {
    const fn new() -> Self {
        Self {
            buf: spin::Mutex::new(String::new()),
        }
    }

    fn clear(&self) {
        self.buf.lock().clear();
    }

    fn take(&self) -> String {
        core::mem::take(&mut *self.buf.lock())
    }
}

impl ShellIo for CaptureBackend {
    fn write_str(&self, s: &str) {
        self.buf.lock().push_str(s);
    }

    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        use core::fmt::Write;
        let _ = self.buf.lock().write_fmt(args);
    }

    fn write_char(&self, ch: char) {
        self.buf.lock().push(ch);
    }

    fn write_byte(&self, b: u8) {
        self.buf.lock().push(b as char);
    }
}

impl ShellBackend for CaptureBackend {
    fn read_byte(&self) -> Option<u8> {
        None
    }
}

static CAPTURE_BACKEND: CaptureBackend = CaptureBackend::new();

async fn run_shell_call(
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    call: ShellCall,
) -> ShellCallOutput {
    let mut outputs: Vec<ShellCallOutputEntry> = Vec::new();

    let timeout_ms = call.timeout_ms.max(1).min(300_000);
    let max_len = call.max_output_length.max(256).min(64 * 1024);

    for cmd in call.commands.iter() {
        if is_live_tetris_command(cmd.as_str()) {
            let (stdout, stderr, exit_code) =
                run_live_tetris_command(spawner, io, term_cols, term_rows).await;
            outputs.push(ShellCallOutputEntry {
                stdout,
                stderr,
                outcome_type: "exit",
                exit_code,
            });
            continue;
        }

        let (stdout, stderr, exit_code, timed_out) =
            run_shell_command_line(spawner, cmd.as_str(), timeout_ms, max_len).await;

        outputs.push(ShellCallOutputEntry {
            stdout,
            stderr,
            outcome_type: if timed_out { "timeout" } else { "exit" },
            exit_code,
        });
    }

    ShellCallOutput {
        call_id: call.call_id,
        max_output_length: max_len,
        output: outputs,
    }
}

fn is_live_tetris_command(line: &str) -> bool {
    let verb = line.trim().split_whitespace().next().unwrap_or("");
    matches_ignore_ascii(verb, "tetris")
}

async fn run_live_tetris_command(
    spawner: &Spawner,
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
) -> (String, String, Option<i32>) {
    let mut mode: ShellMode = ShellMode::Idle;
    let mut cube_mode: bool = false;
    let mut cube = crate::shell::cube::CubeState::new();
    let mut cols = term_cols.max(1);
    let mut rows = term_rows.max(1);
    let mut history: Vec<String> = Vec::new();

    crate::shell::handle_command_action_for_tools(
        CommandAction::EnterTetris,
        &mut mode,
        &mut cube_mode,
        &mut cube,
        io,
        &mut cols,
        &mut rows,
        spawner,
        &mut history,
    )
    .await;

    (
        String::from("launched interactive tetris; user exited"),
        String::new(),
        Some(0),
    )
}

async fn run_shell_command_line(
    spawner: &Spawner,
    line: &str,
    timeout_ms: u64,
    max_len: usize,
) -> (String, String, Option<i32>, bool) {
    let line = line.trim();
    if line.is_empty() {
        return (String::new(), String::new(), Some(0), false);
    }

    if is_disallowed_shell_command(line) {
        let mut err = String::new();
        err.push_str("command blocked by policy\n");
        return (String::new(), err, Some(126), false);
    }

    CAPTURE_BACKEND.clear();

    let mut cols: usize = 200;
    let mut rows: usize = 60;
    let mut mode: ShellMode = ShellMode::Idle;
    let mut cube_mode: bool = false;
    let mut cube = crate::shell::cube::CubeState::new();
    let mut history: Vec<String> = Vec::new();

    let mut ctx = crate::shell::cmd::registry::ShellCommandCtx {
        line,
        spawner,
        io: &CAPTURE_BACKEND,
        term_cols: &mut cols,
        term_rows: &mut rows,
        mode: &mut mode,
    };

    let action = crate::shell::cmd::registry::dispatch_line(&mut ctx);
    let action = match action {
        Some(a) => a,
        None => {
            CAPTURE_BACKEND.write_str("unknown: ");
            CAPTURE_BACKEND.write_str(line);
            CAPTURE_BACKEND.write_str("\n");
            let stdout = limit_and_strip(CAPTURE_BACKEND.take(), max_len);
            return (stdout, String::new(), Some(127), false);
        }
    };

    // Run action (tables, etc.) but do not allow anything that could recursively re-enter the AI wizard.
    match action {
        CommandAction::OpenAiChat { .. } => {
            CAPTURE_BACKEND.write_str("ai: recursion blocked\n");
        }
        other => {
            // Best-effort timeout: we can't preempt arbitrary async work, but we can at least stop waiting.
            let fut = crate::shell::handle_command_action_for_tools(
                other,
                &mut mode,
                &mut cube_mode,
                &mut cube,
                &CAPTURE_BACKEND,
                &mut cols,
                &mut rows,
                spawner,
                &mut history,
            );

            let timed = embassy_futures_timeout(timeout_ms, fut).await;
            if !timed {
                let stdout = limit_and_strip(CAPTURE_BACKEND.take(), max_len);
                return (stdout, String::new(), None, true);
            }
        }
    }

    let stdout = limit_and_strip(CAPTURE_BACKEND.take(), max_len);
    (stdout, String::new(), Some(0), false)
}

fn is_disallowed_shell_command(line: &str) -> bool {
    // Minimal safety gate: prevent obvious destructive actions.
    let verb = line.split_whitespace().next().unwrap_or("");
    matches_ignore_ascii(verb, "acpi")
        || matches_ignore_ascii(verb, "install")
        || matches_ignore_ascii(verb, "format")
        || matches_ignore_ascii(verb, "update")
        || matches_ignore_ascii(verb, "ai")
}

fn matches_ignore_ascii(a: &str, b: &str) -> bool {
    a.as_bytes().eq_ignore_ascii_case(b.as_bytes())
}

async fn embassy_futures_timeout<F: core::future::Future<Output = ()>>(
    timeout_ms: u64,
    fut: F,
) -> bool {
    let t = async {
        Timer::after(Duration::from_millis(timeout_ms)).await;
    };
    match select2(fut, t).await {
        Either::First(_) => true,
        Either::Second(_) => false,
    }
}

fn limit_and_strip(mut s: String, max_len: usize) -> String {
    s = strip_ansi(&s);
    if s.len() > max_len {
        s.truncate(max_len);
    }
    s
}

fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC ... command
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                i += 1;
                while i < bytes.len() {
                    let b = bytes[i];
                    i += 1;
                    if (b'@'..=b'~').contains(&b) {
                        break;
                    }
                }
                continue;
            }
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
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
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn extract_json_string_field(json: &str, needle: &str) -> Option<String> {
    let i = json.find(needle)?;
    let mut j = i + needle.len();
    while j < json.len() {
        let b = json.as_bytes()[j];
        if b == b'"' {
            break;
        }
        if b != b' ' && b != b':' && b != b'\n' && b != b'\r' {
            return None;
        }
        j += 1;
    }
    if j >= json.len() {
        return None;
    }

    j += 1;
    let bytes = json.as_bytes();
    let mut out = String::new();
    while j < bytes.len() {
        let b = bytes[j];
        if b == b'"' {
            return Some(out);
        }
        if b == b'\\' {
            j += 1;
            if j >= bytes.len() {
                break;
            }
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
                        if let Ok(hex_str) = core::str::from_utf8(&bytes[j + 1..j + 5]) {
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
