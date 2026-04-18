extern crate alloc;

use alloc::string::String;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::vmcall;
use crate::vpanic;
use v::{vfetch, vfs, vui2};

const HULL_PROBE_PATH: &[u8] = b"/vm/hull_probe.txt";
const HULL_FETCH_URL: &[u8] = b"https://example.com/";
static HULL_WINDOW_ID: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct ProbeSummary {
    fs_ok: bool,
    net_ok: bool,
    ui_ok: bool,
}

pub fn start() {
    vpanic::set_stage(0x1000);
    net_line("VMHULL: begin");
    log_time_sample("VMHULL: unix time: ");
    let summary = run_vlayer_probes();
    log_probe_summary(summary);
    vpanic::set_stage(0x1001);
    net_line("VMHULL: start complete");
}

pub fn hull_bss_anchor() -> u64 {
    core::ptr::addr_of!(HULL_WINDOW_ID) as u64
}

pub fn idle() -> ! {
    vpanic::set_stage(0x1100);
    net_line("VMHULL: idle");
    net_line("VMHULL: minute logger active");
    let mut last_logged_minute = vmcall::unix_time() / 60;
    loop {
        let now = vmcall::unix_time();
        let minute = now / 60;
        if minute != 0 && minute != last_logged_minute {
            vpanic::set_stage(0x1101);
            net_line_num("VMHULL: unix time: ", now);
            refresh_probe_window_title(now);
            last_logged_minute = minute;
        }
        core::hint::spin_loop();
    }
}

fn log_time_sample(prefix: &str) {
    vpanic::set_stage(0x1010);
    let now = vmcall::unix_time();
    if now != 0 {
        net_line_num(prefix, now);
    } else {
        net_line("VMHULL: unix time unavailable");
    }
}

fn run_vlayer_probes() -> ProbeSummary {
    let fs_ok = run_fs_probe();
    let net_ok = run_net_probe();
    let ui_ok = run_ui2_probe(fs_ok, net_ok);
    ProbeSummary { fs_ok, net_ok, ui_ok }
}

fn run_fs_probe() -> bool {
    vpanic::set_stage(0x1020);
    let now = vmcall::unix_time();
    let mut text = String::new();
    let _ = write!(&mut text, "vmhull trueosfs probe unix_time={}\n", now);

    let handle = match vfs::write_begin(HULL_PROBE_PATH, text.len() as u64) {
        Ok(handle) => handle,
        Err(rc) => {
            net_line_num_signed("VMHULL: fs write_begin rc=", rc);
            return false;
        }
    };

    if let Err(rc) = vfs::write_chunk(handle, text.as_bytes()) {
        let _ = vfs::write_abort(handle);
        net_line_num_signed("VMHULL: fs write_chunk rc=", rc);
        return false;
    }

    if let Err(rc) = vfs::write_finish(handle) {
        net_line_num_signed("VMHULL: fs write_finish rc=", rc);
        return false;
    }

    match vfs::read_file_utf8(HULL_PROBE_PATH) {
        Ok(readback) => {
            if readback == text {
                net_line("VMHULL: trueosfs write/read ok");
                true
            } else {
                net_line("VMHULL: trueosfs readback mismatch");
                false
            }
        }
        Err(rc) => {
            net_line_num_signed("VMHULL: fs read rc=", rc);
            false
        }
    }
}

fn run_net_probe() -> bool {
    vpanic::set_stage(0x1030);
    let op_id = match vfetch::fetch_bytes(HULL_FETCH_URL) {
        Ok(op_id) => op_id,
        Err(rc) => {
            net_line_num_signed("VMHULL: fetch start rc=", rc);
            return false;
        }
    };

    let wait_rc = vfetch::fetch_bytes_wait(op_id, 8_000);
    if wait_rc != 0 {
        net_line_num_signed("VMHULL: fetch wait rc=", wait_rc);
        let _ = vfetch::fetch_bytes_discard(op_id);
        return false;
    }

    match vfetch::fetch_bytes_read(op_id) {
        Ok(bytes) => {
            let mut line = String::new();
            let _ = write!(&mut line, "VMHULL: fetch bytes ok len={}", bytes.len());
            net_line(line.as_str());
            true
        }
        Err(rc) => {
            net_line_num_signed("VMHULL: fetch read rc=", rc);
            false
        }
    }
}

fn run_ui2_probe(fs_ok: bool, net_ok: bool) -> bool {
    vpanic::set_stage(0x1040);
    let rect = vui2::Rect {
        x: 72,
        y: 72,
        width: 420,
        height: 140,
    };
    let Some(window) = vui2::OwnedWindow::create("VMHULL", rect) else {
        net_line("VMHULL: ui2 create failed");
        return false;
    };
    let mut title = String::new();
    let _ = write!(
        &mut title,
        "VMHULL fs={} net={} time={}",
        bool_mark(fs_ok),
        bool_mark(net_ok),
        vmcall::unix_time()
    );
    let ok = window.id().set_title(title.as_str())
        && window.id().set_decorations(vui2::WindowDecorationMode::System)
        && window.id().focus();
    if ok {
        let id = window.leak();
        HULL_WINDOW_ID.store(id.raw(), Ordering::Release);
        net_line("VMHULL: ui2 window ok");
    } else {
        net_line("VMHULL: ui2 window metadata update failed");
    }
    ok
}

fn log_probe_summary(summary: ProbeSummary) {
    let mut line = String::new();
    let _ = write!(
        &mut line,
        "VMHULL: probes fs={} net={} ui2={}",
        bool_mark(summary.fs_ok),
        bool_mark(summary.net_ok),
        bool_mark(summary.ui_ok)
    );
    net_line(line.as_str());
}

fn refresh_probe_window_title(now: u64) {
    let path = HULL_PROBE_PATH;
    let fs_ok = vfs::read_file(path).is_ok();
    let net_ok = vfetch::prewarm_url(HULL_FETCH_URL) == 0;
    let mut title = String::new();
    let _ = write!(
        &mut title,
        "VMHULL fs={} net={} t={}",
        bool_mark(fs_ok),
        bool_mark(net_ok),
        now
    );
    if let Some(window) = vui2::WindowId::new(HULL_WINDOW_ID.load(Ordering::Acquire)) {
        let _ = window.set_title(title.as_str());
    }
}

fn bool_mark(ok: bool) -> &'static str {
    if ok { "ok" } else { "no" }
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

fn net_line_num_signed(prefix: &str, value: i32) {
    let _ = vmcall::net_tcp_write(prefix.as_bytes());
    let mut buf = [0u8; 12];
    let s = fmt_i32(&mut buf, value);
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

fn fmt_i32<'a>(buf: &'a mut [u8; 12], value: i32) -> &'a [u8] {
    if value == 0 {
        buf[0] = b'0';
        return &buf[..1];
    }
    let negative = value < 0;
    let mut n = value.unsigned_abs();
    let mut pos = buf.len();
    while n != 0 {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if negative {
        pos -= 1;
        buf[pos] = b'-';
    }
    &buf[pos..]
}
