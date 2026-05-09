use alloc::format;
use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::net::{Ipv4Addr, Ipv6Addr};
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use v::vnet as api;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::ParseOutcome;

const NETBENCH_URL: &str = "http://ipv4.download.thinkbroadband.com/5GB.zip";
const INTERNAL_NETBENCH_DEFAULT_FLOWS: usize = 2;
const INTERNAL_NETBENCH_MAX_FLOWS: usize = 4;
const CPUBENCH_PHASE_MS: u64 = 10_000;
const CPUBENCH_STOP_GRACE_MS: u64 = 2_000;
const PROGRESS_LOG_MS: u64 = 3000;
const UAS_RAMP_READ_PROBE_BYTES: u64 = 128 * 1024 * 1024;
const UAS_RAMP_READ_FINAL_BYTES: u64 = 512 * 1024 * 1024;
const UAS_RAMP_WRITE_PROBE_BYTES: u64 = 16 * 1024 * 1024;
const UAS_RAMP_WRITE_FINAL_BYTES: u64 = 64 * 1024 * 1024;
const UAS_RAMP_WRITE_PROGRESS_MS: u64 = 1000;
const UAS_RAMP_READ_CANDIDATES: [(usize, usize); 4] = [
    (1024 * 1024, 2),
    (1024 * 1024, 4),
    (1024 * 1024, 1),
    (512 * 1024, 2),
];
const UAS_RAMP_WRITE_CANDIDATES: [(usize, usize); 5] = [
    (1024 * 1024, 1),
    (1024 * 1024, 2),
    (1024 * 1024, 4),
    (1024 * 1024, 8),
    (512 * 1024, 4),
];
const BENCH_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const BENCH_MENU_ROWS: [[&str; 2]; 4] = [
    ["cpu", "Run CPU-only compute benchmark"],
    ["net", "Run network throughput benchmark"],
    [
        "netk",
        "Run internal netbench (literal URL, default 2 flows)",
    ],
    ["uas", "Auto-ramp SK hynix UAS read/write benchmark"],
];

#[derive(Clone)]
struct BenchSessionState {
    id: u64,
    cancel_requested: bool,
}

static BENCH_SESSIONS: spin::Mutex<Vec<BenchSessionState>> = spin::Mutex::new(Vec::new());
static NEXT_BENCH_SESSION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
struct CpuBenchPhase {
    name: &'static str,
    inner_iters: u64,
    est_ops_per_iter: u64,
    progress_every_rounds: u64,
    rounds_per_yield: u64,
    init_js: &'static str,
    probe_js: &'static str,
    kernel_js: &'static str,
    marker_js: &'static str,
}

const CPUBENCH_PHASES: [CpuBenchPhase; 3] = [
    CpuBenchPhase {
        name: "int32x64",
        inner_iters: 128,
        est_ops_per_iter: 64,
        progress_every_rounds: 65_536,
        rounds_per_yield: 16_384,
        init_js: "let a = 0x12345678 | 0; let b = 0x9e3779b9 | 0; let c = 0x7f4a7c15 | 0; let d = 0x6a09e667 | 0;",
        probe_js: "",
        kernel_js: "for (let i = 0; i < INNER; i++) {\
  a = (a ^ (a << 13)) | 0; b = Math.imul(b ^ c, 1664525) | 0;\
  c = (c + d) | 0; d = Math.imul(d ^ a, 22695477) | 0;\
  a = (a + b) | 0; b = (b ^ (b >>> 7)) | 0;\
  c = Math.imul(c ^ d, 1103515245) | 0; d = (d + 12345) | 0;\
  a = (a ^ (a << 5)) | 0; b = Math.imul(b + c, 214013) | 0;\
  c = (c ^ (c >>> 11)) | 0; d = Math.imul(d + a, 16807) | 0;\
  a = (a + d) | 0; b = (b ^ (b << 9)) | 0;\
  c = Math.imul(c + a, 48271) | 0; d = (d ^ (d >>> 3)) | 0;\
}",
        marker_js: "'0x' + (((a ^ b ^ c ^ d) >>> 0).toString(16))",
    },
    CpuBenchPhase {
        name: "f32x128",
        inner_iters: 128,
        est_ops_per_iter: 4,
        progress_every_rounds: 65_536,
        rounds_per_yield: 16_384,
        init_js: "let a = Math.fround(1.0); let b = Math.fround(2.0); let c = Math.fround(3.0); let d = Math.fround(4.0);",
        probe_js: "parentPort.postMessage('probe-enter phase=f32x128 thread=' + threadId + ' case=quad-add'); a = Math.fround(a + 1.0); b = Math.fround(b + 1.0); c = Math.fround(c + 1.0); d = Math.fround(d + 1.0); parentPort.postMessage('probe-after-add phase=f32x128 thread=' + threadId + ' case=quad-add a=' + String(a) + ' b=' + String(b) + ' c=' + String(c) + ' d=' + String(d)); parentPort.postMessage('probe-ok phase=f32x128 thread=' + threadId + ' case=quad-add');",
        kernel_js: "for (let i = 0; i < INNER; i++) { a = Math.fround(a + 1.0); b = Math.fround(b + 1.0); c = Math.fround(c + 1.0); d = Math.fround(d + 1.0); }",
        marker_js: "String(Math.fround(a + b + c + d))",
    },
    CpuBenchPhase {
        name: "f64x128",
        inner_iters: 128,
        est_ops_per_iter: 4,
        progress_every_rounds: 65_536,
        rounds_per_yield: 16_384,
        init_js: "let a = 1.0; let b = 2.0; let c = 3.0; let d = 4.0;",
        probe_js: "parentPort.postMessage('probe-enter phase=f64x128 thread=' + threadId + ' case=quad-add'); a = a + 1.0; b = b + 1.0; c = c + 1.0; d = d + 1.0; parentPort.postMessage('probe-after-add phase=f64x128 thread=' + threadId + ' case=quad-add a=' + String(a) + ' b=' + String(b) + ' c=' + String(c) + ' d=' + String(d)); parentPort.postMessage('probe-ok phase=f64x128 thread=' + threadId + ' case=quad-add');",
        kernel_js: "for (let i = 0; i < INNER; i++) { a = a + 1.0; b = b + 1.0; c = c + 1.0; d = d + 1.0; }",
        marker_js: "String(a + b + c + d)",
    },
];

pub(crate) fn format_speed(bps: u64) -> AllocString {
    if bps < 100 {
        return format!("{} B/s", bps);
    }
    let kb = bps as f64 / 1024.0;
    if kb < 100.0 {
        return format!("{:.1} KB/s", kb);
    }
    let mb = kb / 1024.0;
    if mb < 100.0 {
        return format!("{:.1} MB/s", mb);
    }
    let gb = mb / 1024.0;
    if gb < 100.0 {
        return format!("{:.1} GB/s", gb);
    }
    let tb = gb / 1024.0;
    format!("{:.1} TB/s", tb)
}

pub(crate) fn format_bytes(bytes: u64) -> AllocString {
    if bytes < 1024 {
        return format!("{} B", bytes);
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{:.1} KB", kb);
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{:.1} MB", mb);
    }
    let gb = mb / 1024.0;
    if gb < 1024.0 {
        return format!("{:.1} GB", gb);
    }
    format!("{:.1} TB", gb / 1024.0)
}

pub(crate) fn format_metric_units(value: u64, unit: &str) -> AllocString {
    if value < 1_000 {
        return format!("{} {}", value, unit);
    }
    let k = value as f64 / 1_000.0;
    if k < 1_000.0 {
        return format!("{:.1} K{}", k, unit);
    }
    let m = k / 1_000.0;
    if m < 1_000.0 {
        return format!("{:.1} M{}", m, unit);
    }
    let g = m / 1_000.0;
    if g < 1_000.0 {
        return format!("{:.1} G{}", g, unit);
    }
    format!("{:.1} T{}", g / 1_000.0, unit)
}

pub(crate) fn elapsed_ms_since(start_tick: u64) -> u64 {
    let now_tick = embassy_time_driver::now();
    let elapsed_ticks = now_tick.saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed_ticks.saturating_mul(1000) / hz
    }
}

pub(crate) fn bps_from_progress(bytes: u64, elapsed_ms: u64) -> u64 {
    if elapsed_ms == 0 {
        0
    } else {
        bytes.saturating_mul(1000) / elapsed_ms
    }
}

#[derive(Clone, Copy)]
struct CpuBenchWorker {
    slot: u32,
    worker_id: u32,
    est_ops: u64,
    saw_message: bool,
    saw_rust_task_start: bool,
}

fn cpubench_worker_script(phase: CpuBenchPhase) -> AllocString {
    format!(
        "import {{ parentPort, threadId }} from 'worker_threads';\
let rounds = 0;\
let iters = 0;\
let firstRoundPosted = false;\
let stopRequested = false;\
const INNER = {inner_iters};\
const OPS_PER_ITER = {est_ops_per_iter};\
const PROGRESS_EVERY_ROUNDS = {progress_every_rounds};\
const ROUNDS_PER_YIELD = {rounds_per_yield};\
{init_js}\
parentPort.onMessage((msg) => {{\
  if (msg === 'stop') {{\
    stopRequested = true;\
  }}\
}});\
parentPort.postMessage('stage phase={phase_name} thread=' + threadId + ' step=boot');\
try {{\
  parentPort.postMessage('stage phase={phase_name} thread=' + threadId + ' step=init-ok');\
}} catch (err) {{\
  let detail = '';\
  try {{\
    detail = (err && typeof err.stack === 'string') ? err.stack : String(err);\
  }} catch (_fmtErr) {{\
    detail = '<unprintable-error>';\
  }}\
  parentPort.postMessage('init-error phase={phase_name} thread=' + threadId + ' detail=' + detail);\
  throw err;\
}}\
parentPort.postMessage('ready phase={phase_name} thread=' + threadId + ' inner=' + INNER + ' ops_per_iter=' + OPS_PER_ITER);\
try {{\
  parentPort.postMessage('stage phase={phase_name} thread=' + threadId + ' step=probe-begin');\
  {probe_js}\
  parentPort.postMessage('stage phase={phase_name} thread=' + threadId + ' step=probe-ok');\
}} catch (err) {{\
  let detail = '';\
  try {{\
    detail = (err && typeof err.stack === 'string') ? err.stack : String(err);\
  }} catch (_fmtErr) {{\
    detail = '<unprintable-error>';\
  }}\
  parentPort.postMessage('probe-error phase={phase_name} thread=' + threadId + ' detail=' + detail);\
  throw err;\
}}\
function tick() {{\
  if (stopRequested) {{\
    parentPort.postMessage('done phase={phase_name} thread=' + threadId + ' rounds=' + rounds + ' iters=' + iters + ' est_ops=' + (iters * OPS_PER_ITER));\
    return;\
  }}\
  try {{\
    for (let batch = 0; batch < ROUNDS_PER_YIELD && !stopRequested; batch++) {{\
      {kernel_js}\
      rounds++;\
      iters += INNER;\
      if (!firstRoundPosted) {{\
        firstRoundPosted = true;\
        parentPort.postMessage('first-round phase={phase_name} thread=' + threadId + ' rounds=' + rounds + ' iters=' + iters);\
      }}\
      if ((rounds % PROGRESS_EVERY_ROUNDS) === 0) {{\
        parentPort.postMessage('progress phase={phase_name} thread=' + threadId + ' rounds=' + rounds + ' iters=' + iters + ' est_ops=' + (iters * OPS_PER_ITER) + ' marker=' + ({marker_js}));\
      }}\
    }}\
  }} catch (err) {{\
    let detail = '';\
    try {{\
      detail = (err && typeof err.stack === 'string') ? err.stack : String(err);\
    }} catch (_fmtErr) {{\
      detail = '<unprintable-error>';\
    }}\
    parentPort.postMessage('error phase={phase_name} thread=' + threadId + ' rounds=' + rounds + ' detail=' + detail);\
    throw err;\
  }}\
  if (stopRequested) {{\
    parentPort.postMessage('done phase={phase_name} thread=' + threadId + ' rounds=' + rounds + ' iters=' + iters + ' est_ops=' + (iters * OPS_PER_ITER));\
    return;\
  }}\
  setTimeout(tick, 0);\
}}\
setTimeout(tick, 0);",
        inner_iters = phase.inner_iters,
        est_ops_per_iter = phase.est_ops_per_iter,
        progress_every_rounds = phase.progress_every_rounds,
        rounds_per_yield = phase.rounds_per_yield,
        init_js = phase.init_js,
        phase_name = phase.name,
        probe_js = phase.probe_js,
        kernel_js = phase.kernel_js,
        marker_js = phase.marker_js,
    )
}

fn parse_kv_u64(text: &str, key: &str) -> Option<u64> {
    let start = text.find(key)?;
    let digits = &text[start + key.len()..];
    let end = digits
        .find(|ch: char| !ch.is_ascii_digit())
        .unwrap_or(digits.len());
    digits[..end].parse::<u64>().ok()
}

pub(crate) fn online_background_worker_slots() -> Vec<u32> {
    let mut slots: Vec<u32> = crate::workers::background_worker_slots()
        .into_iter()
        .filter(|slot| {
            crate::smp::read(*slot as usize)
                .map(|r| r.online)
                .unwrap_or(false)
        })
        .collect();
    slots.sort_unstable();
    slots
}

fn cpubench_slot_health_line(slots: &[u32]) -> AllocString {
    let mut parts: Vec<AllocString> = Vec::new();
    for slot in slots.iter().copied() {
        let part = if let Some(r) = crate::smp::read(slot as usize) {
            format!(
                "slot={} online={} state={} seq={}",
                slot,
                if r.online { 1 } else { 0 },
                r.state,
                r.seq
            )
        } else {
            format!("slot={} online=? state=? seq=?", slot)
        };
        parts.push(part);
    }
    parts.join(" | ")
}

fn drain_cpubench_worker_messages(
    target: &MatrixTarget,
    phase: CpuBenchPhase,
    workers: &mut [CpuBenchWorker],
) {
    for worker in workers.iter_mut() {
        while let Some(msg) = trueos_qjs::workers::take_parent_message(worker.worker_id) {
            let Ok(text) = core::str::from_utf8(msg.as_slice()) else {
                continue;
            };
            worker.saw_message = true;
            if let Some(est_ops) = parse_kv_u64(text, "est_ops=") {
                worker.est_ops = worker.est_ops.max(est_ops);
            }
            if text.contains("\"dbg\":\"worker-rust-task-start\"") {
                worker.saw_rust_task_start = true;
            }
            if text.starts_with("progress ") {
                continue;
            }
            if text.starts_with("ready ") || text.starts_with("done ") || !text.is_empty() {
                print_matrix_target_line(
                    target,
                    format!("bench cpu [{} slot={}]: {}", phase.name, worker.slot, text).as_str(),
                );
            }
        }
    }
}

async fn stop_cpubench_workers(
    target: &MatrixTarget,
    phase: CpuBenchPhase,
    workers: &mut [CpuBenchWorker],
    reason: &str,
) {
    for worker in workers.iter() {
        let _ = trueos_qjs::workers::post_to_worker(worker.worker_id, b"stop");
    }
    print_matrix_target_line(
        target,
        format!("bench cpu [{}]: stop -> workers ({})", phase.name, reason).as_str(),
    );

    let deadline = Instant::now() + EmbassyDuration::from_millis(CPUBENCH_STOP_GRACE_MS);
    loop {
        drain_cpubench_worker_messages(target, phase, workers);
        if workers
            .iter()
            .all(|worker| trueos_qjs::workers::worker_exited(worker.worker_id))
        {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    }

    let mut killed = 0usize;
    for worker in workers.iter() {
        if !trueos_qjs::workers::worker_exited(worker.worker_id) {
            trueos_qjs::workers::terminate(worker.worker_id);
            killed += 1;
        }
    }
    if killed != 0 {
        print_matrix_target_line(
            target,
            format!("bench cpu [{}]: terminate fallback workers={}", phase.name, killed).as_str(),
        );
    }
    Timer::after(EmbassyDuration::from_millis(50)).await;
    drain_cpubench_worker_messages(target, phase, workers);
}

fn print_usage(io: &'static dyn ShellBackend2) {
    super::tlb_helper::print_table(io, &BENCH_MENU_HEADERS, &BENCH_MENU_ROWS);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(kind) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };
    match kind {
        "net" => {
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }
            if let Some(session_id) = submit_netbench(spawner, io) {
                ParseOutcome::StartSession(
                    crate::shell2::shell2_cmd::CommandSessionKind::BenchRunning(session_id),
                )
            } else {
                ParseOutcome::Handled
            }
        }
        "netk" => {
            let url = args.next();
            let flows = args.next();
            if args.next().is_some() {
                print_shell_line(
                    io,
                    "bench netk: usage `bench netk [http://ip[:port]/path] [flows]`",
                );
                return ParseOutcome::Handled;
            }
            submit_internal_netbench(io, url, flows);
            ParseOutcome::Handled
        }
        "cpu" => {
            if args.next().is_some() {
                print_usage(io);
                return ParseOutcome::Handled;
            }
            if let Some(session_id) = submit_cpubench(spawner, io) {
                ParseOutcome::StartSession(
                    crate::shell2::shell2_cmd::CommandSessionKind::BenchRunning(session_id),
                )
            } else {
                ParseOutcome::Handled
            }
        }
        "uas" => {
            if args.next().is_some() {
                print_shell_line(io, "bench uas: usage `bench uas`");
                return ParseOutcome::Handled;
            }
            if let Some(session_id) = submit_uasbench(spawner, io) {
                ParseOutcome::StartSession(
                    crate::shell2::shell2_cmd::CommandSessionKind::BenchRunning(session_id),
                )
            } else {
                ParseOutcome::Handled
            }
        }
        _ => {
            print_usage(io);
            ParseOutcome::Handled
        }
    }
}

fn submit_cpubench(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Option<u64> {
    let target = matrix_target_for_backend(io);
    let session_id = bench_session_start();

    print_matrix_target_line(&target, "bench cpu: starting pure compute load");
    set_matrix_target_active(&target, true);
    match cpubench_task(target.clone(), session_id) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            bench_session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "bench cpu: spawn failed");
            return None;
        }
    }
    print_matrix_target_line(&target, "bench cpu: send `q` in this slot to stop");
    Some(session_id)
}

fn select_uasbench_disk(
    io: &'static dyn ShellBackend2,
) -> Option<crate::disc::block::DeviceHandle> {
    let disk = super::tlb_helper::collect_top_level_disk_choices()
        .into_iter()
        .map(|choice| choice.handle)
        .find(|handle| crate::usb2::pen::is_uas_skhynix_disk(*handle));
    if disk.is_none() {
        print_shell_line(io, "bench uas: no UAS SK hynix top-level disc found");
    }
    disk
}

fn submit_uasbench(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Option<u64> {
    let disk = select_uasbench_disk(io)?;
    let target = matrix_target_for_backend(io);
    let session_id = bench_session_start();
    let info = disk.info();

    print_matrix_target_line(
        &target,
        format!(
            "bench uas: starting auto ramp label={} read_probe={} read_final={} write_probe={} write_final={}",
            info.label.as_deref().unwrap_or("-"),
            format_bytes(UAS_RAMP_READ_PROBE_BYTES),
            format_bytes(UAS_RAMP_READ_FINAL_BYTES),
            format_bytes(UAS_RAMP_WRITE_PROBE_BYTES),
            format_bytes(UAS_RAMP_WRITE_FINAL_BYTES)
        )
        .as_str(),
    );

    set_matrix_target_active(&target, true);
    match uasbench_task(target.clone(), session_id) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            bench_session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "bench uas: spawn failed");
            return None;
        }
    }
    print_matrix_target_line(&target, "bench uas: send `q` in this slot to stop");
    Some(session_id)
}

pub(crate) fn bench_session_start() -> u64 {
    let id = NEXT_BENCH_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    BENCH_SESSIONS.lock().push(BenchSessionState {
        id,
        cancel_requested: false,
    });
    id
}

pub(crate) fn bench_session_finish(session_id: u64) {
    let mut sessions = BENCH_SESSIONS.lock();
    if let Some(idx) = sessions.iter().position(|s| s.id == session_id) {
        let _ = sessions.remove(idx);
    }
}

pub(crate) fn bench_cancel_requested(session_id: u64) -> bool {
    BENCH_SESSIONS
        .lock()
        .iter()
        .find(|s| s.id == session_id)
        .map(|s| s.cancel_requested)
        .unwrap_or(false)
}

pub(crate) fn session_alive(session_id: u64) -> bool {
    BENCH_SESSIONS.lock().iter().any(|s| s.id == session_id)
}

pub(crate) fn handle_session_input(
    session_id: u64,
    target: &MatrixTarget,
    submitted: &str,
) -> CommandSessionInputResult {
    if !session_alive(session_id) {
        return CommandSessionInputResult::CompleteIdle;
    }

    let cmd = submitted.trim();
    if cmd.is_empty() {
        return CommandSessionInputResult::KeepRunning;
    }

    if cmd.eq_ignore_ascii_case("q") {
        let mut sessions = BENCH_SESSIONS.lock();
        if let Some(state) = sessions.iter_mut().find(|s| s.id == session_id) {
            if !state.cancel_requested {
                state.cancel_requested = true;
                print_matrix_target_line(target, "bench: stop requested");
            } else {
                print_matrix_target_line(target, "bench: stop already requested");
            }
        }
        return CommandSessionInputResult::KeepRunning;
    }

    print_matrix_target_line(target, "bench: running; send `q` to stop");
    CommandSessionInputResult::KeepRunning
}

fn submit_netbench(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Option<u64> {
    if crate::net::device_count() == 0 {
        print_shell_line(io, "bench net: no NIC available");
        return None;
    }

    let nic_index = crate::net::primary_device_index();
    let target = matrix_target_for_backend(io);
    let session_id = bench_session_start();
    print_matrix_target_line(
        &target,
        format!(
            "bench net: starting on nic={} ({}) url={}",
            nic_index,
            crate::net::device_name_at(nic_index).unwrap_or("Unknown"),
            NETBENCH_URL
        )
        .as_str(),
    );

    set_matrix_target_active(&target, true);
    match netbench_task(target.clone(), session_id, nic_index) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            bench_session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "bench net: spawn failed");
            return None;
        }
    }
    print_matrix_target_line(&target, "bench net: send `q` in this slot to stop");
    Some(session_id)
}

fn submit_internal_netbench(
    io: &'static dyn ShellBackend2,
    url_arg: Option<&str>,
    flows_arg: Option<&str>,
) {
    if crate::net::device_count() == 0 {
        print_shell_line(io, "bench netk: no NIC available");
        return;
    }

    let url = url_arg.unwrap_or(NETBENCH_URL);
    let Some(parsed) = parse_http_url(url) else {
        print_shell_line(io, "bench netk: bad url");
        return;
    };

    let flows = match flows_arg {
        Some(raw) => match raw.parse::<usize>() {
            Ok(n) if (1..=INTERNAL_NETBENCH_MAX_FLOWS).contains(&n) => n,
            _ => {
                print_shell_line(io, "bench netk: flows must be 1..=4");
                return;
            }
        },
        None => INTERNAL_NETBENCH_DEFAULT_FLOWS,
    };

    let nic_index = crate::net::primary_device_index();
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        parsed.path.as_str(),
        parsed.host_header.as_str()
    );

    let submitted = match parsed.target {
        HostTarget::V4(ip) => (0..flows)
            .filter(|_| {
                crate::net::adapter::internal_netbench_submit(
                    nic_index,
                    ip,
                    parsed.port,
                    request.as_bytes(),
                )
            })
            .count(),
        HostTarget::V6(ip) => (0..flows)
            .filter(|_| {
                crate::net::adapter::internal_netbench_submit_v6(
                    nic_index,
                    ip,
                    parsed.port,
                    request.as_bytes(),
                )
            })
            .count(),
        HostTarget::Name(_) => {
            print_shell_line(
                io,
                "bench netk: use a literal http://ip[:port]/path URL to avoid DNS/readiness noise",
            );
            return;
        }
    };

    print_shell_line(
        io,
        format!(
            "bench netk: nic={} ({}) url={} flows={} submitted={}",
            nic_index,
            crate::net::device_name_at(nic_index).unwrap_or("Unknown"),
            url,
            flows,
            submitted
        )
        .as_str(),
    );

    if submitted == 0 {
        print_shell_line(io, "bench netk: submit failed (queue full or service not ready)");
    } else {
        print_shell_line(
            io,
            "bench netk: watch for `netbench-internal:` logs for rx/progress/done",
        );
    }
}

#[derive(Clone, Copy)]
struct UasRampReadBest {
    chunk_bytes: usize,
    max_inflight: usize,
    avg_bps: u64,
}

#[derive(Clone, Copy)]
struct UasRampWriteStats {
    bytes: u64,
    chunk_bytes: usize,
    tx_cap_bytes: usize,
    max_inflight: usize,
    elapsed_ms: u64,
}

fn fill_uasbench_write_pattern(buf: &mut [u8], chunk_bytes: usize, absolute_offset: u64) {
    let mut seed = absolute_offset
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(chunk_bytes as u64)
        .wrapping_add(0x5355_4153_4245_4E43);
    for byte in buf.iter_mut() {
        seed ^= seed << 7;
        seed ^= seed >> 9;
        seed = seed.wrapping_mul(0xD6E8_FD93_35A5_6B19);
        *byte = (seed >> 24) as u8;
    }
}

async fn run_uasbench_write_probe(
    target: &MatrixTarget,
    session_id: u64,
    disk: crate::disc::block::DeviceHandle,
    label: &str,
    total_bytes: u64,
    chunk_bytes: usize,
    tx_cap_bytes: usize,
    max_inflight: usize,
) -> Result<Option<UasRampWriteStats>, crate::disc::block::Error> {
    let (tx_cap, write_inflight) =
        crate::usb2::pen::set_uas_skhynix_write_window_for_bench(disk, tx_cap_bytes, max_inflight)
            .await?;
    let path = format!(".trueosfs-bench/uas-{}-tx-{}-w{}.bin", label, tx_cap, write_inflight);
    let begin =
        crate::r::fs::trueosfs::file_write_begin_async(disk, path.as_str(), total_bytes).await?;
    let Some(stream) = begin else {
        print_matrix_target_line(
            target,
            format!(
                "bench uas: write {} no-space path={} bytes={}",
                label,
                path,
                format_bytes(total_bytes)
            )
            .as_str(),
        );
        return Err(crate::disc::block::Error::OutOfBounds);
    };

    let mut chunk = Vec::new();
    chunk.resize(chunk_bytes, 0);
    let start = Instant::now();
    let mut last_report_ms = 0u64;
    let mut last_report_bytes = 0u64;
    let mut written = 0u64;

    while written < total_bytes {
        if bench_cancel_requested(session_id) {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(stream).await;
            print_matrix_target_line(target, "bench uas: write cancelled");
            return Ok(None);
        }

        let take = core::cmp::min(total_bytes.saturating_sub(written), chunk_bytes as u64) as usize;
        fill_uasbench_write_pattern(&mut chunk[..take], chunk_bytes, written);
        if let Err(err) =
            crate::r::fs::trueosfs::file_write_chunk_async(stream, &chunk[..take]).await
        {
            if matches!(err, crate::disc::block::Error::Timeout) {
                print_matrix_target_line(
                    target,
                    format!(
                        "bench uas: write {} timeout offset={} tx_cap={} write_inflight={}",
                        label,
                        format_bytes(written),
                        format_bytes(tx_cap as u64),
                        write_inflight
                    )
                    .as_str(),
                );
            }
            return Err(err);
        }
        written = written.saturating_add(take as u64);

        let elapsed_ms = start.elapsed().as_millis() as u64;
        if elapsed_ms.saturating_sub(last_report_ms) >= UAS_RAMP_WRITE_PROGRESS_MS {
            let interval_ms = elapsed_ms.saturating_sub(last_report_ms);
            let interval_bytes = written.saturating_sub(last_report_bytes);
            last_report_ms = elapsed_ms;
            last_report_bytes = written;
            print_matrix_target_line(
                target,
                format!(
                    "bench uas: write {} progress={}/{} rate={} avg={} tx_cap={} write_inflight={}",
                    label,
                    format_bytes(written),
                    format_bytes(total_bytes),
                    format_speed(bps_from_progress(interval_bytes, interval_ms)),
                    format_speed(bps_from_progress(written, elapsed_ms)),
                    format_bytes(tx_cap as u64),
                    write_inflight
                )
                .as_str(),
            );
        }
    }

    crate::r::fs::trueosfs::file_write_finish_async(stream).await?;
    let elapsed_ms = start.elapsed().as_millis() as u64;
    Ok(Some(UasRampWriteStats {
        bytes: total_bytes,
        chunk_bytes,
        tx_cap_bytes: tx_cap,
        max_inflight: write_inflight,
        elapsed_ms,
    }))
}

#[embassy_executor::task(pool_size = 1)]
async fn uasbench_task(target: MatrixTarget, session_id: u64) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let Some(disk) = super::tlb_helper::collect_top_level_disk_choices()
            .into_iter()
            .map(|choice| choice.handle)
            .find(|handle| crate::usb2::pen::is_uas_skhynix_disk(*handle))
        else {
            log("bench uas: no UAS SK hynix top-level disc found");
            return;
        };

        let mut best_read: Option<UasRampReadBest> = None;

        for (chunk_bytes, max_inflight) in UAS_RAMP_READ_CANDIDATES {
            if bench_cancel_requested(session_id) {
                log("bench uas: cancelled before read probe");
                break;
            }

            log(
                format!(
                    "bench uas: read probe chunk={} max_inflight={} total={}",
                    format_bytes(chunk_bytes as u64),
                    max_inflight,
                    format_bytes(UAS_RAMP_READ_PROBE_BYTES)
                )
                .as_str(),
            );

            let config = crate::usb2::pen::UasBenchConfig {
                total_bytes: UAS_RAMP_READ_PROBE_BYTES,
                chunk_bytes,
                max_inflight,
            };
            let progress_target = task_target.clone();
            let result = crate::usb2::pen::run_uas_skhynix_stream_bench(
                disk,
                config,
                || bench_cancel_requested(session_id),
                |progress| {
                    if progress.phase == "start" {
                        return;
                    }
                    let interval_rate =
                        bps_from_progress(progress.interval_bytes, progress.interval_ms);
                    let avg_rate =
                        bps_from_progress(progress.completed_bytes, progress.elapsed_ms);
                    print_matrix_target_line(
                        &progress_target,
                        format!(
                            "bench uas: read {} {}/{} rate={} avg={} cwnd={} in_flight={} chunk={} timeouts={} dead={}",
                            progress.phase,
                            format_bytes(progress.completed_bytes),
                            format_bytes(progress.target_bytes),
                            format_speed(interval_rate),
                            format_speed(avg_rate),
                            progress.cwnd,
                            progress.in_flight,
                            format_bytes(progress.chunk_bytes as u64),
                            progress.timeouts,
                            progress.dead_streams
                        )
                        .as_str(),
                    );
                },
            )
            .await;

            let stats = match result {
                Ok(stats) => stats,
                Err(err) => {
                    log(
                        format!(
                            "bench uas: read fail chunk={} max_inflight={} err={:?}",
                            format_bytes(chunk_bytes as u64),
                            max_inflight,
                            err
                        )
                        .as_str(),
                    );
                    continue;
                }
            };

            let avg = bps_from_progress(stats.completed_bytes, stats.elapsed_ms);
            log(
                format!(
                    "bench uas: read result chunk={} max_inflight={} avg={} elapsed={}ms reads={} timeouts={} dead={}",
                    format_bytes(stats.chunk_bytes as u64),
                    stats.max_inflight,
                    format_speed(avg),
                    stats.elapsed_ms,
                    stats.reads_completed,
                    stats.timeouts,
                    stats.dead_streams
                )
                .as_str(),
            );

            if stats.timeouts != 0 || stats.dead_streams != 0 {
                log(
                    format!(
                        "bench uas: read unstable chunk={} max_inflight={} keeping-ramp-short",
                        format_bytes(chunk_bytes as u64),
                        max_inflight
                    )
                    .as_str(),
                );
                continue;
            }

            if best_read.is_none_or(|best| avg > best.avg_bps) {
                best_read = Some(UasRampReadBest {
                    chunk_bytes: stats.chunk_bytes,
                    max_inflight: stats.max_inflight,
                    avg_bps: avg,
                });
            }
        }

        let Some(best_read) = best_read else {
            log("bench uas: no stable read point found; root remains deferred");
            return;
        };

        log(
            format!(
                "bench uas: read best chunk={} max_inflight={} avg={}",
                format_bytes(best_read.chunk_bytes as u64),
                best_read.max_inflight,
                format_speed(best_read.avg_bps)
            )
            .as_str(),
        );

        if !bench_cancel_requested(session_id) {
            log(
                format!(
                    "bench uas: read final chunk={} max_inflight={} total={}",
                    format_bytes(best_read.chunk_bytes as u64),
                    best_read.max_inflight,
                    format_bytes(UAS_RAMP_READ_FINAL_BYTES)
                )
                .as_str(),
            );
            let final_config = crate::usb2::pen::UasBenchConfig {
                total_bytes: UAS_RAMP_READ_FINAL_BYTES,
                chunk_bytes: best_read.chunk_bytes,
                max_inflight: best_read.max_inflight,
            };
            match crate::usb2::pen::run_uas_skhynix_stream_bench(
                disk,
                final_config,
                || bench_cancel_requested(session_id),
                |_| {},
            )
            .await
            {
                Ok(stats) => {
                    let avg = bps_from_progress(stats.completed_bytes, stats.elapsed_ms);
                    log(
                        format!(
                            "bench uas: read final done read={} avg={} elapsed={}ms timeouts={} dead={}",
                            format_bytes(stats.completed_bytes),
                            format_speed(avg),
                            stats.elapsed_ms,
                            stats.timeouts,
                            stats.dead_streams
                        )
                        .as_str(),
                    );
                    if stats.timeouts != 0 || stats.dead_streams != 0 {
                        log("bench uas: read final unstable; skipping write ramp");
                        return;
                    }
                }
                Err(err) => {
                    log(format!("bench uas: read final failed err={:?}", err).as_str());
                    return;
                }
            }
        }

        let mut best_write: Option<UasRampWriteStats> = None;
        for (tx_cap_bytes, max_inflight) in UAS_RAMP_WRITE_CANDIDATES {
            if bench_cancel_requested(session_id) {
                log("bench uas: cancelled before write probe");
                break;
            }
            let chunk_bytes = tx_cap_bytes.saturating_mul(max_inflight.max(1));

            log(
                format!(
                    "bench uas: write probe tx_cap={} write_inflight={} app_chunk={} total={}",
                    format_bytes(tx_cap_bytes as u64),
                    max_inflight,
                    format_bytes(chunk_bytes as u64),
                    format_bytes(UAS_RAMP_WRITE_PROBE_BYTES)
                )
                .as_str(),
            );
            let probe = run_uasbench_write_probe(
                &task_target,
                session_id,
                disk,
                "probe",
                UAS_RAMP_WRITE_PROBE_BYTES,
                chunk_bytes,
                tx_cap_bytes,
                max_inflight,
            )
            .await;
            let Some(stats) = (match probe {
                Ok(stats) => stats,
                Err(err) => {
                    log(
                        format!(
                            "bench uas: write fail tx_cap={} write_inflight={} err={:?}",
                            format_bytes(tx_cap_bytes as u64),
                            max_inflight,
                            err
                        )
                        .as_str(),
                    );
                    let _ =
                        crate::usb2::pen::reset_uas_skhynix_transport_for_bench(disk, "bench-write-fail")
                            .await;
                    break;
                }
            }) else {
                break;
            };
            let avg = bps_from_progress(stats.bytes, stats.elapsed_ms);
            log(
                format!(
                    "bench uas: write result tx_cap={} write_inflight={} avg={} elapsed={}ms",
                    format_bytes(stats.tx_cap_bytes as u64),
                    stats.max_inflight,
                    format_speed(avg),
                    stats.elapsed_ms
                )
                .as_str(),
            );
            if best_write.is_none_or(|best| {
                avg > bps_from_progress(best.bytes, best.elapsed_ms)
            }) {
                best_write = Some(stats);
            }
        }

        let Some(best_write) = best_write else {
            log("bench uas: no stable write point found; root remains deferred");
            return;
        };
        log(
            format!(
                "bench uas: write best tx_cap={} write_inflight={} app_chunk={} avg={}",
                format_bytes(best_write.tx_cap_bytes as u64),
                best_write.max_inflight,
                format_bytes(best_write.chunk_bytes as u64),
                format_speed(bps_from_progress(best_write.bytes, best_write.elapsed_ms))
            )
            .as_str(),
        );

        if !bench_cancel_requested(session_id) {
            log(
                format!(
                    "bench uas: write final tx_cap={} write_inflight={} app_chunk={} total={}",
                    format_bytes(best_write.tx_cap_bytes as u64),
                    best_write.max_inflight,
                    format_bytes(best_write.chunk_bytes as u64),
                    format_bytes(UAS_RAMP_WRITE_FINAL_BYTES)
                )
                .as_str(),
            );
            match run_uasbench_write_probe(
                &task_target,
                session_id,
                disk,
                "final",
                UAS_RAMP_WRITE_FINAL_BYTES,
                best_write.chunk_bytes,
                best_write.tx_cap_bytes,
                best_write.max_inflight,
            )
            .await
            {
                Ok(Some(stats)) => {
                    log(
                        format!(
                            "bench uas: write final done wrote={} avg={} elapsed={}ms tx_cap={} write_inflight={}",
                            format_bytes(stats.bytes),
                            format_speed(bps_from_progress(stats.bytes, stats.elapsed_ms)),
                            stats.elapsed_ms,
                            format_bytes(stats.tx_cap_bytes as u64),
                            stats.max_inflight
                        )
                        .as_str(),
                    );
                }
                Ok(None) => {}
                Err(err) => {
                    log(format!("bench uas: write final failed err={:?}", err).as_str());
                    return;
                }
            }
        }

        log(
            format!(
                "bench uas: done read_chunk={} read_inflight={} write_tx_cap={} write_inflight={} public_root=deferred",
                format_bytes(best_read.chunk_bytes as u64),
                best_read.max_inflight,
                format_bytes(best_write.tx_cap_bytes as u64),
                best_write.max_inflight
            )
            .as_str(),
        );
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}

#[embassy_executor::task(pool_size = 2)]
async fn cpubench_task(target: MatrixTarget, session_id: u64) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };
        let all_worker_slots = crate::workers::background_worker_slots();
        let worker_slots = online_background_worker_slots();
        if worker_slots.is_empty() {
            if all_worker_slots.is_empty() {
                log("bench cpu: no disposable qjs worker slots available");
            } else {
                log("bench cpu: no online disposable qjs worker slots available");
                log(
                    format!(
                        "bench cpu: slot health {}",
                        cpubench_slot_health_line(&all_worker_slots)
                    )
                    .as_str(),
                );
            }
            return;
        }

        log(
            format!(
                "bench cpu: running {} phases x {}s across worker slots {:?}",
                CPUBENCH_PHASES.len(),
                CPUBENCH_PHASE_MS / 1000,
                worker_slots
            )
            .as_str(),
        );
        log(
            format!(
                "bench cpu: slot health {}",
                cpubench_slot_health_line(&worker_slots)
            )
            .as_str(),
        );

        let bench_start_tick = embassy_time_driver::now();

        for phase in CPUBENCH_PHASES {
            if bench_cancel_requested(session_id) {
                log("bench cpu: cancelled before next phase");
                break;
            }

            let worker_code = cpubench_worker_script(phase);
            let mut workers: Vec<CpuBenchWorker> = Vec::new();
            for slot in worker_slots.iter().copied() {
                match trueos_qjs::workers::spawn_eval_on_slot(slot, worker_code.as_bytes()) {
                    Ok(worker_id) => workers.push(CpuBenchWorker {
                        slot,
                        worker_id,
                        est_ops: 0,
                        saw_message: false,
                        saw_rust_task_start: false,
                    }),
                    Err(rc) => {
                        log(
                            format!(
                                "bench cpu [{}]: worker spawn failed slot={} rc={}",
                                phase.name, slot, rc
                            )
                            .as_str(),
                        );
                    }
                }
            }

            if workers.is_empty() {
                log(format!("bench cpu [{}]: no workers started", phase.name).as_str());
                continue;
            }

            log(
                format!(
                    "bench cpu [{}]: started workers={} inner_iters={} est_ops/iter={}",
                    phase.name,
                    workers.len(),
                    phase.inner_iters,
                    phase.est_ops_per_iter
                )
                .as_str(),
            );

            let startup_probe_deadline = Instant::now() + EmbassyDuration::from_millis(500);
            while Instant::now() < startup_probe_deadline {
                drain_cpubench_worker_messages(&task_target, phase, workers.as_mut_slice());
                if workers.iter().all(|worker| worker.saw_message) {
                    break;
                }
                Timer::after(EmbassyDuration::from_millis(25)).await;
            }

            for worker in workers.iter() {
                if !worker.saw_message {
                    log(
                        format!(
                            "bench cpu [{}]: startup-stalled slot={} worker_id={} online_state={}",
                            phase.name,
                            worker.slot,
                            worker.worker_id,
                            crate::smp::read(worker.slot as usize)
                                .map(|r| if r.online { r.state } else { 255 })
                                .unwrap_or(254)
                        )
                        .as_str(),
                    );
                } else if !worker.saw_rust_task_start {
                    log(
                        format!(
                            "bench cpu [{}]: startup-partial slot={} worker_id={} no-rust-start-msg",
                            phase.name, worker.slot, worker.worker_id
                        )
                        .as_str(),
                    );
                }
            }

            let phase_start_tick = embassy_time_driver::now();
            let phase_deadline = Instant::now() + EmbassyDuration::from_millis(CPUBENCH_PHASE_MS);
            let mut next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);

            loop {
                drain_cpubench_worker_messages(&task_target, phase, workers.as_mut_slice());

                if bench_cancel_requested(session_id) {
                    stop_cpubench_workers(&task_target, phase, workers.as_mut_slice(), "cancel")
                        .await;
                    break;
                }

                let all_exited = workers
                    .iter()
                    .all(|worker| trueos_qjs::workers::worker_exited(worker.worker_id));
                if all_exited {
                    break;
                }

                if Instant::now() >= next_progress {
                    let total_est_ops: u64 = workers.iter().map(|worker| worker.est_ops).sum();
                    let elapsed_ms = elapsed_ms_since(phase_start_tick);
                    let rate = bps_from_progress(total_est_ops, elapsed_ms);
                    log(
                        format!(
                            "bench cpu [{}]: progress workers={} total_est_ops={} rate={}/s elapsed={}ms",
                            phase.name,
                            workers.len(),
                            format_metric_units(total_est_ops, "ops"),
                            format_metric_units(rate, "ops"),
                            elapsed_ms
                        )
                        .as_str(),
                    );
                    next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);
                }

                if Instant::now() >= phase_deadline {
                    stop_cpubench_workers(&task_target, phase, workers.as_mut_slice(), "phase-complete")
                        .await;
                    break;
                }

                Timer::after(EmbassyDuration::from_millis(50)).await;
            }

            drain_cpubench_worker_messages(&task_target, phase, workers.as_mut_slice());
            let elapsed_ms = elapsed_ms_since(phase_start_tick);
            let total_est_ops: u64 = workers.iter().map(|worker| worker.est_ops).sum();
            let rate = bps_from_progress(total_est_ops, elapsed_ms);
            log(
                format!(
                    "bench cpu [{}]: done workers={} total_est_ops={} rate={}/s elapsed={}ms",
                    phase.name,
                    workers.len(),
                    format_metric_units(total_est_ops, "ops"),
                    format_metric_units(rate, "ops"),
                    elapsed_ms
                )
                .as_str(),
            );

            if bench_cancel_requested(session_id) {
                break;
            }
        }

        let elapsed_ms = elapsed_ms_since(bench_start_tick);
        log(format!("bench cpu: session elapsed={}ms", elapsed_ms).as_str());
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}

enum HostTarget {
    Name(AllocString),
    V4([u8; 4]),
    V6([u8; 16]),
}

struct ParsedHttpUrl {
    host_header: AllocString,
    port: u16,
    path: AllocString,
    target: HostTarget,
}

fn parse_http_url(url: &str) -> Option<ParsedHttpUrl> {
    let mut u = url.trim();
    if let Some(rest) = u.strip_prefix("http://") {
        u = rest;
    } else {
        return None;
    }
    let (hostport, path) = match u.split_once('/') {
        Some((a, b)) => (a, format!("/{}", b)),
        None => (u, AllocString::from("/")),
    };
    if hostport.is_empty() {
        return None;
    }

    if let Some(rest) = hostport.strip_prefix('[') {
        let (inside, after) = rest.split_once(']')?;
        if inside.is_empty() {
            return None;
        }
        let ip6: Ipv6Addr = inside.parse().ok()?;
        let port = if after.is_empty() {
            80
        } else if let Some(p) = after.strip_prefix(':') {
            if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
                p.parse::<u16>().ok()?
            } else {
                return None;
            }
        } else {
            return None;
        };
        let host_header = if port == 80 {
            format!("[{}]", inside)
        } else {
            format!("[{}]:{}", inside, port)
        };
        return Some(ParsedHttpUrl {
            host_header,
            port,
            path,
            target: HostTarget::V6(ip6.octets()),
        });
    }

    let (host, port) = if let Some((h, p)) = hostport.rsplit_once(':') {
        if !p.is_empty() && p.as_bytes().iter().all(|b| b.is_ascii_digit()) {
            (h, p.parse::<u16>().ok()?)
        } else {
            (hostport, 80)
        }
    } else {
        (hostport, 80)
    };
    if host.is_empty() {
        return None;
    }

    if let Ok(ip4) = host.parse::<Ipv4Addr>() {
        return Some(ParsedHttpUrl {
            host_header: if port == 80 {
                AllocString::from(host)
            } else {
                format!("{}:{}", host, port)
            },
            port,
            path,
            target: HostTarget::V4(ip4.octets()),
        });
    }

    Some(ParsedHttpUrl {
        host_header: if port == 80 {
            AllocString::from(host)
        } else {
            format!("{}:{}", host, port)
        },
        port,
        path,
        target: HostTarget::Name(AllocString::from(host)),
    })
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn header_get_value<'a>(headers: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    let mut i = 0usize;
    while i < headers.len() {
        let line_start = i;
        while i < headers.len() && headers[i] != b'\n' {
            i = i.saturating_add(1);
        }
        let mut line = &headers[line_start..i];
        if i < headers.len() && headers[i] == b'\n' {
            i = i.saturating_add(1);
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }
        let Some(colon) = line.iter().position(|b| *b == b':') else {
            continue;
        };
        let (k, mut v) = line.split_at(colon);
        v = v.get(1..).unwrap_or(&[]);
        if k.len() != name.len() {
            continue;
        }
        if !k
            .iter()
            .zip(name.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            continue;
        }
        while !v.is_empty() && (v[0] == b' ' || v[0] == b'\t') {
            v = &v[1..];
        }
        return Some(v);
    }
    None
}

fn parse_content_length(headers: &[u8]) -> Option<usize> {
    let v = header_get_value(headers, b"content-length")?;
    let s = core::str::from_utf8(v).ok()?;
    s.trim().parse::<usize>().ok()
}

#[embassy_executor::task(pool_size = 2)]
async fn netbench_task(target: MatrixTarget, session_id: u64, nic_index: usize) {
    let task_target = target.clone();
    async move {
        const OPEN_TIMEOUT_MS: u64 = 4000;
        const OVERALL_TIMEOUT_MS: u64 = 120000;
        const IDLE_YIELD_US: u64 = 100;

        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        let mut cancelled = false;

        log("bench net: waiting for net");
        crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;
        if bench_cancel_requested(session_id) {
            cancelled = true;
        }
        if cancelled {
            log("bench net: cancelled before start");
            return;
        }

        let Some(parsed) = parse_http_url(NETBENCH_URL) else {
            log("bench net: bad url");
            return;
        };

        let (host_header, port, path) = (parsed.host_header, parsed.port, parsed.path);

        let (ip4, ip6) = match parsed.target {
            HostTarget::V4(ip) => (Ok(ip), None),
            HostTarget::V6(ip) => (Err(crate::t::net::dns::DnsError::NoAnswer), Some(ip)),
            HostTarget::Name(host) => {
                log(format!("bench net: resolving {}", host).as_str());
                let ip4 = crate::t::net::dns::resolve_ipv4_for_device(
                    nic_index,
                    host.as_str(),
                    crate::t::net::dns::DnsConfig::for_device(nic_index),
                )
                .await;

                let ip6 = if ip4.is_err() {
                    crate::t::net::dns::resolve_ipv6_for_device(
                        nic_index,
                        host.as_str(),
                        crate::t::net::dns::DnsConfig::for_device(nic_index),
                    )
                    .await
                    .ok()
                } else {
                    None
                };

                (ip4, ip6)
            }
        };

        if ip4.is_err() && ip6.is_none() {
            log("bench net: resolve failed");
            return;
        }

        if let Ok(ip) = ip4 {
            log(format!("bench net: connecting to {}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], port).as_str());
        } else if let Some(ip) = ip6 {
            log(format!(
                "bench net: connecting to ipv6 {:02x}{:02x}:{:02x}{:02x}:...:{}",
                ip[0], ip[1], ip[2], ip[3], port
            )
            .as_str());
        }

        let Some(vnet) = crate::r::net::VNet::open_with_event_queue_depth(nic_index, 4096) else {
            log("bench net: vnet open failed");
            return;
        };

        let connect_ok = if let Ok(ip) = ip4 {
            vnet.submit(api::Command::OpenTcpConnect {
                remote: api::EndpointV4 { addr: ip, port },
            })
        } else if let Some(ip) = ip6 {
            vnet.submit(api::Command::OpenTcpConnectV6 {
                remote: api::EndpointV6 { addr: ip, port },
            })
        } else {
            Err(())
        };

        if connect_ok.is_err() {
            log("bench net: tcp connect submit failed");
            return;
        }

        let open_deadline = Instant::now() + EmbassyDuration::from_millis(OPEN_TIMEOUT_MS);
        let tcp_handle = loop {
            if bench_cancel_requested(session_id) {
                log("bench net: cancel requested during connect");
                return;
            }
            if Instant::now() >= open_deadline {
                log("bench net: connect timeout");
                return;
            }
            if let Some(ev) = vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => {
                        break handle;
                    }
                    api::Event::Error { msg } => {
                        log(format!("bench net: connect error: {:?}", msg).as_str());
                        return;
                    }
                    _ => {}
                }
            } else {
                Timer::after(EmbassyDuration::from_millis(1)).await;
            }
        };

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
            path.as_str(),
            host_header.as_str()
        );
        if vnet
            .submit(api::Command::SendTcp {
                handle: tcp_handle,
                data: api::ByteBuf::from_slice_trunc(request.as_bytes()),
            })
            .is_err()
        {
            log("bench net: send failed");
            let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
            return;
        }

        log("bench net: receiving data");

        let mut overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
        let mut header_bytes: Vec<u8> = Vec::new();
        let mut header_done = false;
        let mut expected_len: Option<usize> = None;
        let mut received_bytes: usize = 0;
        let mut closed = false;

        let start_tick = embassy_time_driver::now();
        let mut next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);

        loop {
            if bench_cancel_requested(session_id) {
                cancelled = true;
                break;
            }
            if Instant::now() >= overall_deadline {
                log("bench net: transfer timeout");
                break;
            }

            let mut got_event = false;
            while let Some(ev) = vnet.pop_event() {
                got_event = true;
                match ev {
                    api::Event::TcpData { handle, data } if handle == tcp_handle => {
                        let bytes = data.as_slice();
                        if !header_done {
                            if header_bytes.len() + bytes.len() > 16 * 1024 {
                                log("bench net: header too large");
                                closed = true;
                                break;
                            }
                            header_bytes.extend_from_slice(bytes);

                            if let Some(hend) = find_http_header_end(header_bytes.as_slice()) {
                                header_done = true;
                                expected_len = parse_content_length(&header_bytes[..hend]);
                                received_bytes += header_bytes.len() - hend;
                                if let Some(cl) = expected_len
                                    && received_bytes >= cl
                                {
                                    closed = true;
                                    break;
                                }
                            }
                        } else {
                            received_bytes += bytes.len();
                            if let Some(cl) = expected_len
                                && received_bytes >= cl
                            {
                                closed = true;
                                break;
                            }
                        }
                        overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
                    }
                    api::Event::Closed { handle } if handle == tcp_handle => {
                        closed = true;
                        break;
                    }
                    api::Event::Error { msg } => {
                        log(format!("bench net: socket error: {:?}", msg).as_str());
                        closed = true;
                        break;
                    }
                    _ => {}
                }
            }

            if closed {
                break;
            }

            if Instant::now() >= next_progress {
                let elapsed_ms = elapsed_ms_since(start_tick);
                let bps = bps_from_progress(received_bytes as u64, elapsed_ms);
                log(format!(
                    "bench net: progress {} speed={} elapsed={}ms",
                    format_bytes(received_bytes as u64),
                    format_speed(bps),
                    elapsed_ms
                )
                .as_str());
                next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);
            }

            if !got_event {
                Timer::after(EmbassyDuration::from_micros(IDLE_YIELD_US)).await;
            }
        }

        let _ = vnet.submit(api::Command::Close { handle: tcp_handle });

        let elapsed_ms = elapsed_ms_since(start_tick);
        let bps = bps_from_progress(received_bytes as u64, elapsed_ms);
        if cancelled {
            log(format!(
                "bench net: cancelled received={} speed={} elapsed={}ms",
                format_bytes(received_bytes as u64),
                format_speed(bps),
                elapsed_ms
            )
            .as_str());
        } else {
            log(format!(
                "bench net: done received={} speed={} elapsed={}ms",
                format_bytes(received_bytes as u64),
                format_speed(bps),
                elapsed_ms
            )
            .as_str());
        }
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}
