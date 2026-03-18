use alloc::format;
use alloc::string::String as AllocString;
use alloc::vec::Vec;
use core::net::{Ipv4Addr, Ipv6Addr};
use core::str::SplitWhitespace;
use core::sync::atomic::{AtomicU64, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use trueos_v::vnet as api;

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::CommandSessionInputResult;

const NETBENCH_URL: &str = "http://ipv4.download.thinkbroadband.com/5GB.zip";
const FILEBENCH_PATH: &str = "bench-lorem-100mb.txt";
const FILEBENCH_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
const FILEBENCH_PATTERN: &[u8] = b"10101010";
const PROGRESS_LOG_MS: u64 = 3000;
const BENCH_MENU_HEADERS: [&str; 2] = ["Subcommand", "Description"];
const BENCH_MENU_ROWS: [[&str; 2]; 2] = [
    ["net", "Run network throughput benchmark"],
    ["file", "Run TRUEOSFS streaming write benchmark"],
];

#[derive(Clone)]
struct BenchSessionState {
    id: u64,
    cancel_requested: bool,
}

static BENCH_SESSIONS: spin::Mutex<Vec<BenchSessionState>> = spin::Mutex::new(Vec::new());
static NEXT_BENCH_SESSION_ID: AtomicU64 = AtomicU64::new(1);

fn format_speed(bps: u64) -> AllocString {
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

fn format_bytes(bytes: u64) -> AllocString {
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

fn elapsed_ms_since(start_tick: u64) -> u64 {
    let now_tick = embassy_time_driver::now();
    let elapsed_ticks = now_tick.saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ;
    if hz == 0 {
        0
    } else {
        elapsed_ticks.saturating_mul(1000) / hz
    }
}

fn bps_from_progress(bytes: u64, elapsed_ms: u64) -> u64 {
    if elapsed_ms == 0 {
        0
    } else {
        bytes.saturating_mul(1000) / elapsed_ms
    }
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
    if args.next().is_some() {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    match kind {
        "net" => {
            if let Some(session_id) = submit_netbench(spawner, io) {
                ParseOutcome::StartSession(
                    crate::shell2::shell2_cmd::CommandSessionKind::BenchRunning(session_id),
                )
            } else {
                ParseOutcome::Handled
            }
        }
        "file" => {
            if let Some(session_id) = submit_filebench(spawner, io) {
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

fn bench_session_start() -> u64 {
    let id = NEXT_BENCH_SESSION_ID.fetch_add(1, Ordering::Relaxed);
    BENCH_SESSIONS.lock().push(BenchSessionState {
        id,
        cancel_requested: false,
    });
    id
}

fn bench_session_finish(session_id: u64) {
    let mut sessions = BENCH_SESSIONS.lock();
    if let Some(idx) = sessions.iter().position(|s| s.id == session_id) {
        let _ = sessions.remove(idx);
    }
}

fn bench_cancel_requested(session_id: u64) -> bool {
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

fn submit_filebench(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Option<u64> {
    let Some(disk) = crate::v::fs::trueosfs::primary_root_handle().or_else(super::select_default_disk_target) else {
        print_shell_line(io, "bench file: no disk device found");
        return None;
    };

    let target = matrix_target_for_backend(io);
    let session_id = bench_session_start();
    let info = disk.info();
    print_matrix_target_line(
        &target,
        format!(
            "bench file: starting on disk id={} ({}) label={:?}",
            info.id.raw(),
            info.id,
            info.label
        )
        .as_str(),
    );

    set_matrix_target_active(&target, true);
    if spawner
        .spawn(filebench_task(target.clone(), session_id, disk))
        .is_err()
    {
        bench_session_finish(session_id);
        set_matrix_target_active(&target, false);
        print_shell_line(io, "bench file: spawn failed");
        return None;
    }
    print_matrix_target_line(&target, "bench file: send `q` in this slot to stop");
    Some(session_id)
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
    if spawner
        .spawn(netbench_task(target.clone(), session_id, nic_index))
        .is_err()
    {
        bench_session_finish(session_id);
        set_matrix_target_active(&target, false);
        print_shell_line(io, "bench net: spawn failed");
        return None;
    }
    print_matrix_target_line(&target, "bench net: send `q` in this slot to stop");
    Some(session_id)
}

#[embassy_executor::task(pool_size = 2)]
async fn filebench_task(
    target: MatrixTarget,
    session_id: u64,
    disk: crate::disc::block::DeviceHandle,
) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            print_matrix_target_line(&task_target, line);
        };

        let Some(placement) = crate::v::fs::trueosfs::locate_async(disk)
            .await
            .ok()
            .flatten()
        else {
            log("bench file: selected disk is not TRUEOSFS");
            return;
        };
        if !disk.supports_write() {
            log("bench file: selected disk is read-only");
            return;
        }

        let info = disk.info();
        log(
            format!(
                "bench file: target id={} ({}) blocks={} bs={} writable={} label={:?}",
                info.id.raw(),
                info.id,
                info.block_count,
                info.block_size,
                info.writable,
                info.label
            )
            .as_str(),
        );
        log(
            format!(
                "bench file: super_lba={} data_lba={} file=/{} bytes={}",
                placement.super_lba,
                placement.data_lba,
                FILEBENCH_PATH,
                FILEBENCH_TOTAL_BYTES
            )
            .as_str(),
        );

        let bench_chunk_bytes = if info.max_transfer_bytes > 0 {
            let max_transfer = info.max_transfer_bytes as usize;
            core::cmp::max(4 * 1024, core::cmp::min(max_transfer, 1024 * 1024))
        } else {
            256 * 1024
        };

        let Some(stream_handle) = (match crate::v::fs::trueosfs::file_write_begin_async(
            disk,
            FILEBENCH_PATH,
            FILEBENCH_TOTAL_BYTES,
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                log(format!("bench file: begin failed ({:?})", e).as_str());
                return;
            }
        }) else {
            log("bench file: begin failed (no space / no placement)");
            return;
        };

        let mut chunk: Vec<u8> = alloc::vec![0u8; bench_chunk_bytes];
        if !FILEBENCH_PATTERN.is_empty() {
            let mut off = 0usize;
            while off < chunk.len() {
                let take = core::cmp::min(FILEBENCH_PATTERN.len(), chunk.len() - off);
                chunk[off..off + take].copy_from_slice(&FILEBENCH_PATTERN[..take]);
                off = off.saturating_add(take);
            }
        }

        let mut written: u64 = 0;
        let mut write_err: Option<crate::disc::block::Error> = None;
        let mut finished_ok = false;
        let mut cancelled = false;
        let start_tick = embassy_time_driver::now();
        let mut next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);

        log("bench file: streaming write started");

        while written < FILEBENCH_TOTAL_BYTES {
            if bench_cancel_requested(session_id) {
                cancelled = true;
                break;
            }
            let remaining = (FILEBENCH_TOTAL_BYTES - written) as usize;
            let n = core::cmp::min(remaining, chunk.len());

            if let Err(e) =
                crate::v::fs::trueosfs::file_write_chunk_async(stream_handle, &chunk[..n]).await
            {
                write_err = Some(e);
                break;
            }
            written = written.saturating_add(n as u64);

            if Instant::now() >= next_progress || written >= FILEBENCH_TOTAL_BYTES {
                let elapsed_ms = elapsed_ms_since(start_tick);
                let bps = bps_from_progress(written, elapsed_ms);
                log(
                    format!(
                        "bench file: progress {}/{} speed={}/s elapsed={}ms",
                        format_bytes(written),
                        format_bytes(FILEBENCH_TOTAL_BYTES),
                        format_speed(bps),
                        elapsed_ms
                    )
                    .as_str(),
                );
                next_progress = Instant::now() + EmbassyDuration::from_millis(PROGRESS_LOG_MS);
            }
        }

        if cancelled {
            let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
        } else if write_err.is_none() {
            match crate::v::fs::trueosfs::file_write_finish_async(stream_handle).await {
                Ok(()) => finished_ok = true,
                Err(e) => {
                    let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
                    write_err = Some(e);
                }
            }
        } else {
            let _ = crate::v::fs::trueosfs::file_write_abort_async(stream_handle).await;
        }

        let _ = crate::v::fs::trueosfs::file_delete_async(disk, FILEBENCH_PATH).await;

        let elapsed_ms = elapsed_ms_since(start_tick);
        let bps = bps_from_progress(written, elapsed_ms);
        if cancelled {
            log(
                format!(
                    "bench file: cancelled wrote={} speed={}/s elapsed={}ms",
                    format_bytes(written),
                    format_speed(bps),
                    elapsed_ms
                )
                .as_str(),
            );
        } else if let Some(e) = write_err {
            log(format!("bench file: write failed ({:?})", e).as_str());
        } else if finished_ok {
            log(
                format!(
                    "bench file: done wrote={} speed={}/s elapsed={}ms",
                    format_bytes(written),
                    format_speed(bps),
                    elapsed_ms
                )
                .as_str(),
            );
        }
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
        crate::v::readiness::wait_for(crate::v::readiness::NET_CONFIGURED).await;
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
            HostTarget::V6(ip) => (Err(crate::v::net::dns::DnsError::NoAnswer), Some(ip)),
            HostTarget::Name(host) => {
                log(format!("bench net: resolving {}", host).as_str());
                let ip4 = crate::v::net::dns::resolve_ipv4_for_device(
                    nic_index,
                    host.as_str(),
                    crate::v::net::dns::DnsConfig::for_device(nic_index),
                )
                .await;

                let ip6 = if ip4.is_err() {
                    crate::v::net::dns::resolve_ipv6_for_device(
                        nic_index,
                        host.as_str(),
                        crate::v::net::dns::DnsConfig::for_device(nic_index),
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
            log(
                format!(
                    "bench net: connecting to {}.{}.{}.{}:{}",
                    ip[0], ip[1], ip[2], ip[3], port
                )
                .as_str(),
            );
        } else if let Some(ip) = ip6 {
            log(
                format!(
                    "bench net: connecting to ipv6 {:02x}{:02x}:{:02x}{:02x}:...:{}",
                    ip[0], ip[1], ip[2], ip[3], port
                )
                .as_str(),
            );
        }

        let Some(vnet) = crate::v::net::VNet::open_with_event_queue_depth(nic_index, 4096) else {
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
                        overall_deadline =
                            Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
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
                log(
                    format!(
                        "bench net: progress {} speed={}/s elapsed={}ms",
                        format_bytes(received_bytes as u64),
                        format_speed(bps),
                        elapsed_ms
                    )
                    .as_str(),
                );
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
            log(
                format!(
                    "bench net: cancelled received={} speed={}/s elapsed={}ms",
                    format_bytes(received_bytes as u64),
                    format_speed(bps),
                    elapsed_ms
                )
                .as_str(),
            );
        } else {
            log(
                format!(
                    "bench net: done received={} speed={}/s elapsed={}ms",
                    format_bytes(received_bytes as u64),
                    format_speed(bps),
                    elapsed_ms
                )
                .as_str(),
            );
        }
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}
