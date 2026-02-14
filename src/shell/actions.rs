use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use super::cube::{CubeState, WireShape};
use super::{CommandAction, PendingAction, ShellBackend, ShellIo, ShellMode};

use crate::v::net::wss::WssConnection;
use crate::shell::output::ReverseOutput;

pub(super) async fn handle_command_action(
    action: CommandAction,
    mode: &mut ShellMode,
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
) {
    match action {
        CommandAction::Pending(pending) => handle_pending(mode, pending),
        CommandAction::ShowInstallDiskTable => super::print_install_disk_table(io).await,
        CommandAction::ShowFormatDiskTable => super::print_format_disk_table(io).await,
        CommandAction::ShowUpdateDiskTable => super::print_update_disk_table(io).await,
        CommandAction::ShowFileMountTable => {
            super::print_trueosfs_mount_table(io).await;
            io.write_str("file: enter mount index or disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowBenchDiskTable => {
            super::print_bench_disk_table(io).await;
            io.write_str("bench: enter TRUEOSFS disk id (blank/q cancels)\r\n");
        }
        CommandAction::ShowNetbenchNicTable => {
            super::print_netbench_nic_table(io).await;
            io.write_str("netbench: enter nic id (blank/q cancels)\r\n");
        }
        CommandAction::Qjs { src } => handle_qjs(io, term_cols, term_rows, spawner, history, src).await,
        CommandAction::EnterCube => handle_enter_cube(cube_mode, cube, io, term_cols, term_rows),
        CommandAction::EnterIco => handle_enter_ico(cube_mode, cube, io, term_cols, term_rows),
        CommandAction::EnterGo => handle_enter_go(io).await,
        CommandAction::EnterGoTwo => handle_enter_go_two(io).await,
        CommandAction::EnterTxtEdt { filename, slot_id } => {
            handle_enter_txt(cube_mode, io, term_cols, term_rows, filename, slot_id).await;
        }
        CommandAction::EnterTetris => {
            handle_enter_tetris(cube_mode, io, term_cols, term_rows).await;
        }
        CommandAction::DoFormat { disc_id } => {
            handle_do_format(mode, io, disc_id).await;
        }
        CommandAction::DoInstall { disc_id } => {
            handle_do_install(mode, io, term_cols, spawner, disc_id).await;
        }
        CommandAction::DoUpdate { disc_id } => {
            handle_do_update(mode, io, term_cols, spawner, disc_id).await;
        }
        CommandAction::RunNetbench { nic_index } => {
            super::bench::run_netbench(io, nic_index, *term_cols, *term_rows, history).await;
            clear_statusbar(io, *term_cols, *term_rows);
            *mode = ShellMode::Idle;
        }
        CommandAction::RunBenchFs { disk_id } => {
            let target = crate::disc::block::device_handles()
                .into_iter()
                .find(|h| h.parent().is_none() && h.id().raw() == disk_id);
            if let Some(handle) = target {
                super::bench::run_bench_fs(io, handle, *term_cols, *term_rows, history).await;
            } else {
                io.write_str("\r\nbench: disk disappeared\r\n");
            }
            clear_statusbar(io, *term_cols, *term_rows);
            *mode = ShellMode::Idle;
        }
        CommandAction::OpenAiChat { token, first } => {
            handle_ai_realtime_chat(io, *term_cols, *term_rows, history, token.as_str(), first.as_str()).await;
        }
        CommandAction::None => {}
    }
}

fn json_escape_into(out: &mut alloc::string::String, s: &str) {
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

fn extract_json_string_field(json: &str, needle: &str) -> Option<alloc::string::String> {
    // Tiny extractor for patterns like: "delta":"...".
    // Not a general JSON parser.
    let i = json.find(needle)?;
    let mut j = i + needle.len();
    if !json.as_bytes().get(j).copied().is_some_and(|b| b == b'"') {
        return None;
    }
    j += 1; // opening quote

    let bytes = json.as_bytes();
    let mut out = alloc::string::String::new();
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

fn log_prefixed_lines(out: &ReverseOutput<'_>, prefix: &str, msg: &str) {
    if msg.is_empty() {
        out.write_str(prefix);
        out.write_str("\n");
        return;
    }

    let mut first_line = true;
    for line in msg.split('\n') {
        if first_line {
            out.write_str(prefix);
            first_line = false;
        } else {
            out.write_str(prefix);
        }
        out.write_str(line);
        out.write_str("\n");
    }
}

async fn handle_ai_realtime_chat(
    io: &'static dyn ShellBackend,
    term_cols: usize,
    term_rows: usize,
    history: &mut alloc::vec::Vec<alloc::string::String>,
    token: &str,
    first: &str,
) {
    // Model string is intentionally fixed per user request.
    const MODEL: &str = "gpt-5.3-codex";
    const BETA_HDR: &str = "OpenAI-Beta: realtime=v1";

    let out = ReverseOutput::new(io, term_cols, term_rows, history);

    out.write_str("ai: connecting... (type .exit or Ctrl-C to leave)\n");

    let url = alloc::format!("wss://api.openai.com/v1/realtime?model={}", MODEL);
    let auth = alloc::format!("Authorization: Bearer {}", token);
    let headers: [&str; 2] = [auth.as_str(), BETA_HDR];

    let mut wss = match WssConnection::connect_with_headers(url.as_str(), &headers).await {
        Ok(c) => c,
        Err(e) => {
            out.write_fmt(format_args!("ai: connect failed {:?}\n", e));
            return;
        }
    };

    // Best-effort session init.
    let _ = wss.send(
        "{\"type\":\"session.update\",\"session\":{\"modalities\":[\"text\"],\"instructions\":\"You are a concise shell assistant running inside TRUEOS.\"}}",
    );

    if !first.trim().is_empty() {
        log_prefixed_lines(&out, "you: ", first.trim());
        let mut esc = alloc::string::String::new();
        json_escape_into(&mut esc, first.trim());
        let msg = alloc::format!(
            "{{\"type\":\"conversation.item.create\",\"item\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}}}",
            esc
        );
        let _ = wss.send(&msg);
        let _ = wss.send("{\"type\":\"response.create\"}");
    }

    out.write_str("ai: connected\n");
    io.write_str("ai> ");

    let mut line = alloc::string::String::new();
    let mut saw_cr = false;
    let mut started_output = false;
    let mut response_buf = alloc::string::String::new();
    let mut response_active = false;

    loop {
        while let Some(frame) = wss.recv() {
            if frame.contains("\"type\":\"response.create\"") {
                response_active = true;
                response_buf.clear();
            }

            if let Some(delta) = extract_json_string_field(&frame, "\"delta\":")
                .or_else(|| extract_json_string_field(&frame, "\"text\":"))
            {
                if !started_output {
                    io.write_str("\r\n");
                    started_output = true;
                }
                io.write_str(delta.as_str());
                if response_active {
                    response_buf.push_str(delta.as_str());
                }
            }

            // Heuristic end markers.
            if frame.contains("\"type\":\"response.output_text.done\"")
                || frame.contains("\"type\":\"response.done\"")
                || frame.contains("\"type\":\"response.completed\"")
            {
                response_active = false;
                if !response_buf.trim().is_empty() {
                    log_prefixed_lines(&out, "ai: ", response_buf.trim_end());
                    response_buf.clear();
                }
                // Restore prompt after logging into reverse buffer.
                io.write_str("\r\nai> ");
                started_output = false;
            }
        }

        if let Some(b) = io.read_byte() {
            if saw_cr && b == b'\n' {
                saw_cr = false;
                continue;
            }
            saw_cr = b == b'\r';

            match b {
                0x03 => {
                    io.write_str("\r\nopenai: exit\r\n");
                    return;
                }
                b'\r' | b'\n' => {
                    let input = alloc::string::String::from(line.trim());
                    line.clear();

                    io.write_str("\r\n");
                    started_output = false;

                    if input.eq_ignore_ascii_case(".exit") || input.eq_ignore_ascii_case(".quit") {
                        if !response_buf.trim().is_empty() {
                            response_active = false;
                            log_prefixed_lines(&out, "ai: ", response_buf.trim_end());
                            response_buf.clear();
                        }
                        out.write_str("ai: bye\n");
                        return;
                    }

                    if input.is_empty() {
                        if !response_buf.trim().is_empty() {
                            response_active = false;
                            log_prefixed_lines(&out, "ai: ", response_buf.trim_end());
                            response_buf.clear();
                        }
                        io.write_str("ai> ");
                        continue;
                    }

                    if !response_buf.trim().is_empty() {
                        response_active = false;
                        log_prefixed_lines(&out, "ai: ", response_buf.trim_end());
                        response_buf.clear();
                    }

                    log_prefixed_lines(&out, "you: ", input.as_str());

                    let mut esc = alloc::string::String::new();
                    json_escape_into(&mut esc, input.as_str());
                    let msg = alloc::format!(
                        "{{\"type\":\"conversation.item.create\",\"item\":{{\"type\":\"message\",\"role\":\"user\",\"content\":[{{\"type\":\"input_text\",\"text\":\"{}\"}}]}}}}",
                        esc
                    );
                    if wss.send(&msg).is_err() || wss.send("{\"type\":\"response.create\"}").is_err() {
                        out.write_str("ai: send failed\n");
                        return;
                    }

                    response_active = true;
                    response_buf.clear();
                    io.write_str("ai> ");
                }
                0x08 | 0x7F => {
                    if !line.is_empty() {
                        line.pop();
                        io.write_str("\x08 \x08");
                    }
                }
                _ => {
                    // Minimal line editing: accept bytes as chars.
                    line.push(b as char);
                    io.write_byte(b);
                }
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(20)).await;
        }
    }
}

fn handle_pending(mode: &mut ShellMode, pending: PendingAction) {
    match pending {
        PendingAction::AcpiReset | PendingAction::AcpiState(_) => {
            *mode = ShellMode::Wait {
                action: pending,
                deadline: Instant::now() + EmbassyDuration::from_secs(5),
            };
        }
        _ => {
            *mode = ShellMode::Confirm(pending);
        }
    }
}

async fn handle_qjs(
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    spawner: &Spawner,
    history: &mut alloc::vec::Vec<alloc::string::String>,
    src: heapless::String<192>,
) {
    if trueos_qjs::async_fs::ensure_service_started(spawner) {
        if src.trim().is_empty() {
            super::shellqjs::repl_shell(io, *term_cols, *term_rows, history).await;
        } else {
            super::shellqjs::run(io, src.as_str()).await;
        }
    } else {
        io.write_str("qjs: async fs service unavailable\r\n");
    }
}

fn handle_enter_cube(
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = true;
    cube.set_shape(WireShape::Cube);
    cube.reset();
    super::enter_cube_mode(io, term_cols, term_rows);
}

fn handle_enter_ico(
    cube_mode: &mut bool,
    cube: &mut CubeState,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = true;
    cube.set_shape(WireShape::Icosidodecahedron);
    cube.reset();
    super::enter_cube_mode(io, term_cols, term_rows);
}

async fn handle_enter_go(io: &'static dyn ShellBackend) {
    const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
    run_go_animation(io, &GO_CHARS).await;
}

async fn handle_enter_go_two(io: &'static dyn ShellBackend) {
    const GO_TWO_CHARS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
    run_go_animation(io, &GO_TWO_CHARS).await;
}

async fn run_go_animation(io: &'static dyn ShellBackend, chars: &[char]) {
    if chars.is_empty() {
        return;
    }
    let mut go_idx = 0;
    io.write_str(crate::ecma48::HIDE_CURSOR);
    loop {
        if io.read_byte().is_some() {
            break;
        }
        let ch = chars[go_idx];
        go_idx = (go_idx + 1) % chars.len();
        io.write_str("\r");
        super::write_prompt(io);
        io.write_char(ch);
        Timer::after(EmbassyDuration::from_millis(160)).await;
    }
    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str("\r\n");
}

async fn handle_enter_txt(
    cube_mode: &mut bool,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
    filename: heapless::String<48>,
    slot_id: u8,
) {
    *cube_mode = false;
    let cols = *term_cols;
    let rows = *term_rows;

    if let Some(buf) = crate::matrix::take_blob(slot_id) {
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Running);
        let out_buf = super::txt::run(io, cols, rows, filename.as_str(), buf).await;
        let _ = crate::matrix::set_blob_owned_with_preview(slot_id, out_buf);
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
        io.write_fmt(format_args!("\r\ntxt: updated §{}\r\n", slot_id + 1));
        super::refresh_title_bar(io, cols);
    } else {
        io.write_str("\r\ntxt: invalid slot\r\n");
    }

    reset_shell_display(io, *term_cols, *term_rows);
}

async fn handle_enter_tetris(
    cube_mode: &mut bool,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    term_rows: &mut usize,
) {
    *cube_mode = false;
    let cols = *term_cols;
    let rows = *term_rows;
    super::shelltetris::run(io, cols, rows).await;
    reset_shell_display(io, *term_cols, *term_rows);
}

async fn handle_do_format(mode: &mut ShellMode, io: &'static dyn ShellBackend, disc_id: u32) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        io.write_str("\r\nformat: creating 1 partition + TRUEOSFS...\r\n");
        let parts = [crate::disc::install::gpt::GptPartitionSpec {
            type_guid: crate::v::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
            name: "TRUEOS",
            size: crate::disc::install::gpt::PartitionSize::Remaining,
            attributes: 0,
        }];
        let mut log = |msg: &str| {
            io.write_str(msg);
            io.write_str("\r\n");
        };

        match crate::disc::install::gpt::write_gpt_layout_with_log(handle, &parts, &mut log).await {
            Ok(_) => {
                if let Ok(reg) = crate::v::disc::partition::register_gpt_partitions(handle).await {
                    if let Some(first) = reg.first() {
                        if let Some(part_handle) = crate::disc::block::device_handle(first.id) {
                            match crate::v::fs::trueosfs::format_blank_partition_async(part_handle).await {
                                Ok(()) => {
                                    let (status, err) = crate::v::disc::detect::detect_physical_disk_detail(handle).await;
                                    io.write_fmt(format_args!(
                                        "format: ok (status now: {}{})\r\n",
                                        status.short(),
                                        match (&status, err) {
                                            (crate::v::disc::detect::DiscStatus::Unknown, Some(e)) => alloc::format!("; err={:?}", e),
                                            _ => alloc::string::String::new(),
                                        }
                                    ));
                                }
                                Err(e) => io.write_fmt(format_args!("format: TRUEOSFS failed ({:?})\r\n", e)),
                            }
                        }
                    }
                }
            }
            Err(e) => io.write_fmt(format_args!("format: GPT write failed ({:?})\r\n", e)),
        }
    } else {
        io.write_str("\r\nformat: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

async fn handle_do_install(
    mode: &mut ShellMode,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    spawner: &Spawner,
    disc_id: u32,
) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        if let (Some(kernel), Some(bootx64)) = (crate::limine::install_kernel_bytes(), crate::limine::install_bootx64_bytes()) {
            io.write_str("\r\ninstall: starting...\r\n");
            match crate::matrix::alloc_slot(alloc::format!("install disc{:03}", disc_id).as_str()) {
                Some(slot) => {
                    let _ = spawner.spawn(crate::matrix::install_matrix_job(slot, handle, bootx64, kernel));
                    io.write_fmt(format_args!("install: started §{} (dump logs with §{})\r\n", slot + 1, slot + 1));
                    super::refresh_title_bar(io, *term_cols);
                }
                None => io.write_str("install: matrix full\r\n"),
            }
        } else {
            io.write_str("\r\ninstall: kernel or BOOTX64.EFI missing\r\n");
        }
    } else {
        io.write_str("\r\ninstall: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

async fn handle_do_update(
    mode: &mut ShellMode,
    io: &'static dyn ShellBackend,
    term_cols: &mut usize,
    spawner: &Spawner,
    disc_id: u32,
) {
    let target = crate::disc::block::device_handles()
        .into_iter()
        .find(|h| h.parent().is_none() && h.id().raw() == disc_id);
    if let Some(handle) = target {
        io.write_str("\r\nupdate: starting...\r\n");
        match crate::matrix::alloc_slot(alloc::format!("update disc{:03}", disc_id).as_str()) {
            Some(slot) => {
                let _ = spawner.spawn(crate::matrix::update_matrix_job(slot, handle));
                io.write_fmt(format_args!("update: started §{} (dump logs with §{})\r\n", slot + 1, slot + 1));
                super::refresh_title_bar(io, *term_cols);
            }
            None => io.write_str("update: matrix full\r\n"),
        }
    } else {
        io.write_str("\r\nupdate: no such disk\r\n");
    }
    *mode = ShellMode::Idle;
}

fn clear_statusbar(io: &dyn super::ShellIo, cols: usize, rows: usize) {
    let _ = super::statusbar::set_left_active("");
    let _ = super::statusbar::set_right_active("");
    for i in 0..super::statusbar::INDICATOR_COUNT {
        let _ = super::statusbar::set_indicator_active(i, 0);
    }
    super::statusbar::refresh(io, cols, rows);
}

fn reset_shell_display(io: &'static dyn ShellBackend, term_cols: usize, term_rows: usize) {
    io.write_str(crate::ecma48::CLEAR_SCREEN);
    io.write_str(crate::ecma48::HOME);
    super::write_banner(io, term_cols);
    super::apply_shell_scroll_region(io, term_rows);
}
