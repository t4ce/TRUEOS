use alloc::string::String;
use alloc::vec::Vec;
use alloc::borrow::ToOwned;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};

use crate::wait::{select2, Either};

use crate::shell::{CommandAction, ShellBackend, ShellMode};
use crate::shell::interface::ShellIo;
use crate::shell::output::ReverseOutput;
use crate::shell::statusbar;
use crate::v::net::https::post_https_json_async;

static AI_DUMP_REQUEST_JSON_ONCE: AtomicBool = AtomicBool::new(true);

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
    spawner: &Spawner,
    history: &mut Vec<String>,
    initial_prompt: &str,
) {
    const API_KEY: &str = "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";
    const URL: &str = "https://api.openai.com/v1/responses";
    
    // Ensure correct scroll region (Row 3..Bottom)
    crate::shell::output::apply_shell_scroll_region(io, term_rows);

    let out = ReverseOutput::new(io, term_cols, term_rows, history);

    let mut previous_response_id: Option<String> = None;
    let mut pending_shell_outputs: Vec<ShellCallOutput> = Vec::new();

    if !initial_prompt.trim().is_empty() {
        {
             let mut echoed = String::new();
             echoed.push_str("ai> ");
             echoed.push_str(initial_prompt);
             out.echo_command(echoed.as_str());
        }
        process_input(
            &out,
            io,
            term_cols,
            term_rows,
            spawner,
            initial_prompt,
            URL,
            API_KEY,
            &mut previous_response_id,
            &mut pending_shell_outputs,
        )
        .await;
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
                    break;
                }
                if !input.is_empty() {
                    {
                        let mut echoed = String::new();
                        echoed.push_str("ai> ");
                        echoed.push_str(&input);
                        out.echo_command(echoed.as_str());
                    }
                    process_input(
                        &out,
                        io,
                        term_cols,
                        term_rows,
                        spawner,
                        &input,
                        URL,
                        API_KEY,
                        &mut previous_response_id,
                        &mut pending_shell_outputs,
                    )
                    .await;
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
    spawner: &Spawner,
    input: &str,
    url: &str,
    key: &str,
    previous_response_id: &mut Option<String>,
    pending_shell_outputs: &mut Vec<ShellCallOutput>,
) {
    let body = build_openai_request_body(input, previous_response_id.as_deref(), pending_shell_outputs);

    if AI_DUMP_REQUEST_JSON_ONCE.swap(false, Ordering::AcqRel) {
        let clipped = clip_visible(body.as_str(), 4096);
        crate::log!("ai: request_json(len={}): {}\n", body.len(), clipped);
    }

    statusbar::set_right_active("thinking");
    statusbar::refresh(io, term_cols, term_rows);

    // Increase timeout to 120s since web search can be slow.
    let net_future = post_https_json_async(url, body, Some(key), 120_000, 256 * 1024);
    let cancel_future = wait_for_enter_with_animation(io);

    // Run network vs cancellation
    let result = select2(net_future, cancel_future).await;

    // Clear animations/status
    statusbar::set_right_active("");
    statusbar::refresh(io, term_cols, term_rows);
    io.write_str(crate::ecma48::SHOW_CURSOR); // ensure cursor shown if animation hid it
    // Restore cursor to prompt line just in case animation left it elsewhere
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 5))); // move past ai> 

    match result {
        Either::First(res) => {
            // Network completed
            match res {
                Ok(bytes) => {
                    if let Ok(s) = core::str::from_utf8(&bytes) {
                        // Track response id for multi-turn and for tool follow-ups.
                        if let Some(id) = extract_response_id(s) {
                            *previous_response_id = Some(id);
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
                                let out_item = run_shell_call(spawner, call).await;

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
                            );

                            let net_future = post_https_json_async(
                                url,
                                followup_body,
                                Some(key),
                                120_000,
                                256 * 1024,
                            );
                            let cancel_future = wait_for_enter_with_animation(io);
                            let follow = select2(net_future, cancel_future).await;

                            match follow {
                                Either::First(Ok(bytes)) => {
                                    if let Ok(s2) = core::str::from_utf8(&bytes) {
                                        if let Some(id2) = extract_response_id(s2) {
                                            *previous_response_id = Some(id2);
                                        }
                                        pending_shell_outputs.clear();
                                        write_openai_text(out, s2);
                                        return;
                                    }
                                }
                                Either::First(Err(e)) => {
                                    out.write_fmt(format_args!(
                                        "ai: tool follow-up network error {:?} (will retry on next prompt)\n",
                                        e
                                    ));
                                }
                                Either::Second(_) => {
                                    out.write_str("ai: tool follow-up aborted (will retry on next prompt)\n");
                                }
                            }

                            // If we couldn't complete the follow-up, keep outputs stashed.
                            out.write_str("ai: shell outputs captured (will be sent with your next prompt)\n");
                            return;
                        }

                        // Debug aid: if the user likely wanted command execution but we got no tool call.
                        if looks_like_execute_request(input) {
                            out.write_str("ai: note: model did not issue shell_call for this prompt\n");
                            out.write_str("ai: tip: prefix with '!' to force tool usage (example: !tlb.pci)\n");
                        }

                        // Normal text response.
                        pending_shell_outputs.clear();
                        write_openai_text(out, s);
                    } else {
                        out.write_str("ai: error decoding utf8\n");
                    }
                }
                Err(e) => {
                    out.write_fmt(format_args!("ai: network error {:?}\n", e));
                }
            }
        }
        Either::Second(_) => {
            // User cancelled
            out.write_str("ai: aborted\n");
        }
    }
}

fn write_openai_text(out: &ReverseOutput<'_>, json: &str) {
    // Prefer Responses API convenience field.
    if let Some(content) = extract_json_string_field(json, "\"output_text\":") {
        out.write_str("ai: ");
        out.write_str(&content);
        out.write_str("\n");
        return;
    }

    // Fallback: some responses embed a "text" field in output items.
    if let Some(content) = extract_json_string_field(json, "\"text\":") {
        out.write_str("ai: ");
        out.write_str(&content);
        out.write_str("\n");
        return;
    }

    out.write_str("ai: [raw] ");
    out.write_str(json);
    out.write_str("\n");
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
) -> String {
    // Tools: enable local shell execution and keep web_search available.
    // If `!` is used (force_tools), expose ONLY shell so tool_choice=required can't pick web_search.
    let tools_auto = "[{\"type\":\"shell\",\"environment\":{\"type\":\"local\"}},{\"type\":\"web_search\"}]";
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

    // Keep instructions short; they're repeated each turn.
    let instructions = "You are running inside TRUEOS. Use the shell tool to execute TRUEOS shell commands when asked to run commands or to verify facts. Commands must be non-interactive. Prefer read-only inspection commands. Avoid destructive commands.";
    let mut esc_instructions = String::new();
    json_escape_into(&mut esc_instructions, instructions);

    let mut body = String::new();
    body.push_str("{\"model\":\"gpt-5.2\",");
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
    // Manual scan for "id":"resp_..." (Responses ids start with resp_).
    let marker = "\"id\":\"resp_";
    let start = json.find(marker)? + marker.len();
    let rest = &json[start..];
    let end = rest.find('"')?;
    let mut out = String::new();
    out.push_str("resp_");
    out.push_str(&rest[..end]);
    Some(out)
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
        hay.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
    }
    contains_ascii(lower, b"execute") || contains_ascii(lower, b"run ") || contains_ascii(lower, b"shell")
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

    let mut outputs: Vec<String> = Vec::new();
    for entry in item.output.iter() {
        let mut esc_out = String::new();
        let mut esc_err = String::new();
        json_escape_into(&mut esc_out, entry.stdout.as_str());
        json_escape_into(&mut esc_err, entry.stderr.as_str());

        let outcome = match (entry.outcome_type, entry.exit_code) {
            ("exit", Some(code)) => alloc::format!(
                "{{\"type\":\"exit\",\"exit_code\":{}}}",
                code
            ),
            ("timeout", _) => "{\"type\":\"timeout\"}".to_owned(),
            _ => "{\"type\":\"exit\",\"exit_code\":1}".to_owned(),
        };

        outputs.push(alloc::format!(
            "{{\"stdout\":\"{}\",\"stderr\":\"{}\",\"outcome\":{}}}",
            esc_out,
            esc_err,
            outcome
        ));
    }

    alloc::format!(
        "{{\"type\":\"shell_call_output\",\"call_id\":\"{}\",\"max_output_length\":{},\"output\":[{}]}}",
        esc_call_id,
        item.max_output_length,
        join_json_array(&outputs)
    )
}

fn extract_shell_calls(json: &str) -> Vec<ShellCall> {
    let mut out: Vec<ShellCall> = Vec::new();
    let mut idx: usize = 0;

    loop {
        let hay = &json[idx..];
        let rel = find_any(hay, &["\"type\":\"shell_call\"", "\"type\": \"shell_call\""]);
        let Some(rel_pos) = rel else { break };
        let pos = idx + rel_pos;

        let call_id = match extract_json_string_field_from(json, pos, "\"call_id\":") {
            Some(v) => v,
            None => {
                idx = pos + 10;
                continue;
            }
        };

        let commands = extract_json_string_array_field_from(json, pos, "\"commands\":").unwrap_or_default();
        let timeout_ms = extract_json_u64_field_from(json, pos, "\"timeout_ms\":").unwrap_or(60_000);
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

fn extract_json_string_array_field_from(json: &str, start: usize, needle: &str) -> Option<Vec<String>> {
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
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' || bytes[i] == b',') {
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
        Self { buf: spin::Mutex::new(String::new()) }
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

async fn run_shell_call(spawner: &Spawner, call: ShellCall) -> ShellCallOutput {
    let mut outputs: Vec<ShellCallOutputEntry> = Vec::new();

    let timeout_ms = call.timeout_ms.max(1).min(300_000);
    let max_len = call.max_output_length.max(256).min(64 * 1024);

    for cmd in call.commands.iter() {
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

async fn wait_for_enter_with_animation(io: &dyn ShellBackend) {
    const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    let mut go_idx = 0;
    
    io.write_str(crate::ecma48::HIDE_CURSOR);
    
    loop {
        // check input
        // Since we are racing with network, any input should abort.
        // But specifically looking for Enter? The user said "wait can be canceled".
        // Usually implied by any key or Enter.
        // The read_byte blocks? No, we need to poll or use Timer.
        // wait... io.read_byte() is non-blocking (returns Option<u8> immediately?).
        // If it is blocking, we have a problem because we need to animate.
        // src/shell/cmd/shell_cmds.rs used `io.read_byte()` loop with Timer.
        // So read_byte seems non-blocking or at least returns None quickly.
        
        if let Some(b) = io.read_byte() {
            if b == b'\r' || b == b'\n' {
                break;
            }
        }
        
        // draw spinner at Row 2, col 5 (after "ai> ")
        io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 5)));
        io.write_char(GO_CHARS[go_idx]);
        
        go_idx = (go_idx + 1) % GO_CHARS.len();
        
        Timer::after(Duration::from_millis(160)).await;
    }
    
    io.write_str(crate::ecma48::SHOW_CURSOR);
    // Clear the spinner char
    io.write_fmt(format_args!("{}", crate::ecma48::pos(2, 5)));
    io.write_str(" ");
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

