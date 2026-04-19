use core::sync::atomic::{AtomicU32, Ordering};
use core::arch::x86_64::__cpuid;

use crate::vfetch_job::{BytesJob, Poll as FetchPoll};
use crate::vmcall;
use crate::vpanic;
use v::{vio, vui2, vsys};

const HULL_PROBE_PATH: &[u8] = b"/vm/hull_probe.txt";
const HULL_FETCH_URL: &[u8] = b"https://example.com/";
static HULL_WINDOW_ID: AtomicU32 = AtomicU32::new(0);
static HULL_PROBE_FLAGS: AtomicU32 = AtomicU32::new(0);

const PROBE_FS_OK: u32 = 1 << 0;
const PROBE_NET_OK: u32 = 1 << 1;
const PROBE_UI_OK: u32 = 1 << 2;

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
    let mut flags = 0u32;
    if fs_ok {
        flags |= PROBE_FS_OK;
    }
    if net_ok {
        flags |= PROBE_NET_OK;
    }
    if ui_ok {
        flags |= PROBE_UI_OK;
    }
    HULL_PROBE_FLAGS.store(flags, Ordering::Release);
    ProbeSummary { fs_ok, net_ok, ui_ok }
}

fn run_fs_probe() -> bool {
    vpanic::set_stage(0x1020);
    let now = vmcall::unix_time();
    let mut text = [0u8; 96];
    let text_len = build_fs_probe_line(&mut text, now);

    let path = core::str::from_utf8(HULL_PROBE_PATH).unwrap_or("/vm/hull_probe.txt");
    let handle = match vio::kfs::write_file_begin(path, text_len as u64) {
        Ok(handle) => handle,
        Err(rc) => {
            net_line_num_signed("VMHULL: fs write_begin rc=", rc);
            return false;
        }
    };

    if let Err(rc) = vio::kfs::write_file_chunk(handle, &text[..text_len]) {
        let _ = vio::kfs::write_file_abort(handle);
        net_line_num_signed("VMHULL: fs write_chunk rc=", rc);
        return false;
    }

    if let Err(rc) = vio::kfs::write_file_finish(handle) {
        net_line_num_signed("VMHULL: fs write_finish rc=", rc);
        return false;
    }

    net_line("VMHULL: trueosfs write ok");
    true
}

fn run_net_probe() -> bool {
    vpanic::set_stage(0x1030);
    let job = match BytesJob::start(HULL_FETCH_URL) {
        Ok(job) => job,
        Err(rc) => {
            net_line_num_signed("VMHULL: fetch start rc=", rc);
            return false;
        }
    };

    let start_secs = vmcall::unix_time();
    let deadline_secs = start_secs.saturating_add(8);
    let mut spins = 0u32;

    loop {
        match job.poll_len() {
            Ok(FetchPoll::Pending) => {
                spins = spins.saturating_add(1);
                if start_secs != 0 && vmcall::unix_time() >= deadline_secs {
                    net_line("VMHULL: fetch poll timeout");
                    let _ = job.discard();
                    return false;
                }
                if start_secs == 0 && spins >= 50_000 {
                    net_line("VMHULL: fetch poll budget exhausted");
                    let _ = job.discard();
                    return false;
                }
                vsys::poll_once();
                core::hint::spin_loop();
            }
            Ok(FetchPoll::Ready(len)) => {
                net_line_num("VMHULL: fetch bytes ok len=", len as u64);
                let _ = job.discard();
                return true;
            }
            Err(rc) => {
                net_line_num_signed("VMHULL: fetch poll rc=", rc);
                let _ = job.discard();
                return false;
            }
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
    let mut title = [0u8; 64];
    let title_len = build_probe_title(&mut title, fs_ok, net_ok);
    let ok = window.id().set_title(as_str(&title[..title_len]))
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
    let mut line = [0u8; 64];
    let len = build_probe_summary(&mut line, summary);
    net_line(as_str(&line[..len]));
}

fn refresh_probe_window_title(_now: u64) {
    let flags = HULL_PROBE_FLAGS.load(Ordering::Acquire);
    let fs_ok = (flags & PROBE_FS_OK) != 0;
    let net_ok = (flags & PROBE_NET_OK) != 0;
    let mut title = [0u8; 64];
    let title_len = build_probe_title(&mut title, fs_ok, net_ok);
    if let Some(window) = vui2::WindowId::new(HULL_WINDOW_ID.load(Ordering::Acquire)) {
        let _ = window.set_title(as_str(&title[..title_len]));
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

fn build_fs_probe_line(buf: &mut [u8; 96], unix_time: u64) -> usize {
    let mut n = 0usize;
    n = push_bytes(buf, n, b"vmhull trueosfs probe unix_time=");
    let mut num = [0u8; 20];
    n = push_bytes(buf, n, fmt_u64(&mut num, unix_time));
    push_bytes(buf, n, b"\n")
}

fn build_probe_summary(buf: &mut [u8; 64], summary: ProbeSummary) -> usize {
    let mut n = 0usize;
    n = push_bytes(buf, n, b"VMHULL: probes fs=");
    n = push_bytes(buf, n, bool_mark(summary.fs_ok).as_bytes());
    n = push_bytes(buf, n, b" net=");
    n = push_bytes(buf, n, bool_mark(summary.net_ok).as_bytes());
    n = push_bytes(buf, n, b" ui2=");
    push_bytes(buf, n, bool_mark(summary.ui_ok).as_bytes())
}

fn build_probe_title(buf: &mut [u8; 64], fs_ok: bool, net_ok: bool) -> usize {
    let mut n = 0usize;
    n = push_bytes(buf, n, b"VMHULL fs=");
    n = push_bytes(buf, n, bool_mark(fs_ok).as_bytes());
    n = push_bytes(buf, n, b" net=");
    n = push_bytes(buf, n, bool_mark(net_ok).as_bytes());
    n = push_bytes(buf, n, b" ap=");
    let mut num = [0u8; 20];
    push_bytes(buf, n, fmt_u64(&mut num, guest_apic_id() as u64))
}

fn guest_apic_id() -> u32 {
    unsafe { (__cpuid(1).ebx >> 24) & 0xff }
}

fn push_bytes(dst: &mut [u8], at: usize, src: &[u8]) -> usize {
    let mut i = at;
    for &b in src {
        if i >= dst.len() {
            break;
        }
        dst[i] = b;
        i += 1;
    }
    i
}

fn as_str(bytes: &[u8]) -> &str {
    unsafe { core::str::from_utf8_unchecked(bytes) }
}
