use alloc::string::String;

use crate::v;
use crate::vpanic;

pub fn start() {
    vpanic::set_stage(0x1000);
    v::vsys::log_info("VMDEMO: begin\n");

    if v::vclock::ntp_current_unix_seconds() != 0 {
        v::vsys::log_info("VMDEMO: ntp ok\n");
    } else {
        v::vsys::log_error("VMDEMO: ntp zero\n");
    }

    // Probe SVG upload ABI entry without allocating: kernel should reject null/empty with -3.
    let rc = v::vgfx::probe_upload_svg_to_texture_async(1);
    if rc == -2 || rc == -3 {
        v::vsys::log_info("VMDEMO: svg abi ok\n");
    } else {
        v::vsys::log_error("VMDEMO: svg abi fail\n");
    }

    vpanic::set_stage(0x1001);
    run_tcp_shell();

    vpanic::set_stage(0x1002);
    v::vsys::log_info("VMDEMO: end\n");
}

fn run_tcp_shell() {
    vpanic::set_stage(0x1100);
    vpanic::note("tcp-shell-enter");
    let port = v::env::var("TRUEOS_VM_TCP_SHELL_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or(4246);

    vpanic::set_stage(0x1101);
    if !v::vshell::vm_tcp_shell_start(port) {
        vpanic::dump("tcp-shell-start-failed");
        v::vsys::log_error("VMSHELL: tcp listen failed\n");
        return;
    }

    vpanic::set_stage(0x1102);
    v::vsys::log_info("VMSHELL: tcp bridge ready\n");
    write_dual_line("guest shell ready");
    write_dual_line("commands: help, echo, time, date, svg, exit");
    write_dual_line_num("listening tcp port ", port as u64);
    write_prompt();

    let mut line = String::new();
    let mut buf = [0u8; 256];
    let started_at = v::vclock::ntp_current_unix_seconds();

    loop {
        vpanic::set_stage(0x1200);
        v::vsys::poll_once();

        vpanic::set_stage(0x1201);
        let status = v::vshell::vm_tcp_shell_status();
        if status & 0x4 == 0 {
            let now = v::vclock::ntp_current_unix_seconds();
            if started_at != 0 && now != 0 && now.saturating_sub(started_at) >= 10 {
                vpanic::set_stage(0x1202);
                write_dual_line("tcp shell timeout waiting for client");
                break;
            }
        }

        vpanic::set_stage(0x1203);
        let got = v::vshell::vm_tcp_shell_read(&mut buf);
        if got == 0 {
            continue;
        }

        for &byte in &buf[..got] {
            match byte {
                b'\r' | b'\n' => {
                    vpanic::set_stage(0x1204);
                    write_dual_raw(b"\r\n");
                    let command = line.trim();
                    if !command.is_empty() && handle_command(command) {
                        vpanic::set_stage(0x1205);
                        return;
                    }
                    line.clear();
                    write_prompt();
                }
                0x08 | 0x7F => {
                    if line.pop().is_some() {
                        write_dual_raw(b"\x08 \x08");
                    }
                }
                byte if byte.is_ascii_graphic() || byte == b' ' => {
                    if line.len() < 160 {
                        line.push(byte as char);
                        write_dual_raw(&[byte]);
                    }
                }
                _ => {}
            }
        }
    }
}

fn handle_command(command: &str) -> bool {
    if command.eq_ignore_ascii_case("help") {
        write_dual_line("help  - show commands");
        write_dual_line("echo  - echo text");
        write_dual_line("time  - show unix seconds");
        write_dual_line("date  - show kernel date");
        write_dual_line("svg   - probe svg abi");
        write_dual_line("exit  - leave vm guest shell");
        return false;
    }

    if let Some(rest) = command.strip_prefix("echo ") {
        write_dual_line(rest);
        return false;
    }

    if command.eq_ignore_ascii_case("time") {
        write_dual_line_num("unix seconds ", v::vclock::ntp_current_unix_seconds());
        return false;
    }

    if command.eq_ignore_ascii_case("date") {
        if let Some(date) = v::vclock::kernel_date_day_month_year() {
            write_dual_line(date.as_str());
        } else {
            write_dual_line("date unavailable");
        }
        return false;
    }

    if command.eq_ignore_ascii_case("svg") {
        let rc = v::vgfx::probe_upload_svg_to_texture_async(1);
        if rc == -2 || rc == -3 {
            write_dual_line("svg abi ok");
        } else {
            write_dual_line_num("svg abi rc ", rc as u64);
        }
        return false;
    }

    if command.eq_ignore_ascii_case("exit") {
        write_dual_line("leaving vm guest shell");
        return true;
    }

    write_dual_line("unknown command");
    false
}

fn write_prompt() {
    write_dual_raw(b"vm> ");
}

fn write_dual_line_num(prefix: &str, value: u64) {
    let mut line = String::from(prefix);
    push_u64_decimal(&mut line, value);
    write_dual_line(line.as_str());
}

fn write_dual_line(text: &str) {
    write_dual_raw(text.as_bytes());
    write_dual_raw(b"\r\n");
}

fn write_dual_raw(bytes: &[u8]) {
    let _ = v::vshell::uart1_shell_write(bytes);
    let _ = v::vshell::vm_tcp_shell_write(bytes);
}

fn push_u64_decimal(out: &mut String, mut value: u64) {
    if value == 0 {
        out.push('0');
        return;
    }

    let mut digits = [0u8; 20];
    let mut len = 0usize;
    while value != 0 {
        digits[len] = (value % 10) as u8;
        value /= 10;
        len += 1;
    }

    while len != 0 {
        len -= 1;
        out.push((b'0' + digits[len]) as char);
    }
}
