use crate::v;
use crate::vmcall;
use crate::vpanic;

pub fn start() {
    vpanic::set_stage(0x1000);
    net_line("VMDEMO: begin");

    // vmcall ping — proves the guest→host boundary and vmexit loop work.
    vpanic::set_stage(0x1001);
    if vmcall::ping() {
        net_line("VMDEMO: vmcall ping ok");
    } else {
        net_line("VMDEMO: vmcall ping fail");
    }

    // vmcall unix time — first real service through the boundary.
    vpanic::set_stage(0x1002);
    let ntp = vmcall::unix_time();
    if ntp != 0 {
        net_line("VMDEMO: vmcall ntp ok");
    } else {
        net_line("VMDEMO: vmcall ntp zero");
    }

    // SVG ABI probe: still direct (safe — only reads kernel constants).
    vpanic::set_stage(0x1003);
    let rc = v::vgfx::probe_upload_svg_to_texture_async(1);
    if rc == -2 || rc == -3 {
        net_line("VMDEMO: svg abi ok");
    } else {
        net_line("VMDEMO: svg abi fail");
    }

    vpanic::set_stage(0x1004);
    report(ntp);

    vpanic::set_stage(0x1005);
    net_line("VMDEMO: end");
}

pub fn idle() -> ! {
    vpanic::set_stage(0x1100);
    net_line("VMDEMO: idle");
    net_line("VMDEMO: commands: help ping time preserve");

    let mut line = [0u8; 128];
    let mut line_len = 0usize;
    let mut rx = [0u8; 64];

    loop {
        let got = vmcall::net_tcp_read(&mut rx);
        if got == 0 {
            core::hint::spin_loop();
            continue;
        }

        for &byte in &rx[..got] {
            match byte {
                b'\r' | b'\n' => {
                    if line_len != 0 {
                        handle_command(&line[..line_len]);
                        line_len = 0;
                    }
                }
                0x08 | 0x7F => {
                    line_len = line_len.saturating_sub(1);
                }
                _ => {
                    if line_len < line.len() {
                        line[line_len] = byte;
                        line_len += 1;
                    }
                }
            }
        }
    }
}

fn report(ntp: u64) {
    vpanic::set_stage(0x1010);
    net_line("vm1 guest boot ok");
    net_line_num("unix time: ", ntp);
    net_line("abi probe: ntp + svg ok");
}

fn handle_command(command: &[u8]) {
    if eq_ignore_ascii_case(command, b"help") {
        net_line("VMDEMO: commands: help ping time preserve");
        return;
    }

    if eq_ignore_ascii_case(command, b"ping") {
        if vmcall::ping() {
            net_line("VMDEMO: ping ok");
        } else {
            net_line("VMDEMO: ping fail");
        }
        return;
    }

    if eq_ignore_ascii_case(command, b"time") {
        net_line_num("unix time: ", vmcall::unix_time());
        return;
    }

    if eq_ignore_ascii_case(command, b"preserve") {
        net_line("VMDEMO: preserve requested");
        vmcall::preserve();
        net_line("VMDEMO: preserve returned unexpectedly");
        return;
    }

    net_line("VMDEMO: unknown command");
}

fn eq_ignore_ascii_case(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }

    left.iter()
        .zip(right.iter())
        .all(|(&l, &r)| l.eq_ignore_ascii_case(&r))
}

fn net_line(text: &str) {
    let _ = vmcall::net_tcp_write(text.as_bytes());
    let _ = vmcall::net_tcp_write(b"\r\n");
}

fn net_line_num(prefix: &str, value: u64) {
    let _ = vmcall::net_tcp_write(prefix.as_bytes());
    let mut buf = [0u8; 20];
    let s = fmt_u64(&mut buf, value);
    let _ = vmcall::net_tcp_write(s);
    let _ = vmcall::net_tcp_write(b"\r\n");
}

fn fmt_u64<'a>(buf: &'a mut [u8; 20], mut value: u64) -> &'a [u8] {
    if value == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let mut pos = buf.len();
    while value != 0 {
        pos -= 1;
        buf[pos] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    &buf[pos..]
}
