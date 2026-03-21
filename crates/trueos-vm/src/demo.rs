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

fn report(ntp: u64) {
    vpanic::set_stage(0x1010);
    net_line("vm1 guest boot ok");
    net_line_num("unix time: ", ntp);
    net_line("abi probe: ntp + svg ok");
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
