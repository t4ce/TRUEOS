use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use crate::shell::ShellBackend;

// NETBENCH Statics
pub(crate) const NETBENCH_UPDATE_MS: u64 = 250;
pub(crate) const NETBENCH_URL: &str = "http://ipv4.download.thinkbroadband.com/1GB.zip";

pub(crate) const NETBENCH_IDLE: u8 = 0;
pub(crate) const NETBENCH_RUNNING: u8 = 1;
pub(crate) const NETBENCH_DONE: u8 = 2;
pub(crate) const NETBENCH_ABORTED: u8 = 3;
pub(crate) const NETBENCH_FAILED: u8 = 4;

pub(crate) static NETBENCH_STATE: AtomicU8 = AtomicU8::new(NETBENCH_IDLE);
pub(crate) static NETBENCH_ABORT_REQ: AtomicBool = AtomicBool::new(false);
pub(crate) static NETBENCH_BYTES: AtomicU64 = AtomicU64::new(0);
pub(crate) static NETBENCH_START_TICK: AtomicU64 = AtomicU64::new(0);
pub(crate) static NETBENCH_END_TICK: AtomicU64 = AtomicU64::new(0);
pub(crate) static NETBENCH_FAIL_CODE: AtomicU8 = AtomicU8::new(0);
pub(crate) static NETBENCH_STATUS_SLOT: AtomicU8 = AtomicU8::new(u8::MAX);

#[inline]
pub(crate) fn netbench_fail_text(code: u8) -> &'static str {
    match code {
        1 => "bad url",
        2 => "dns",
        3 => "open vnet",
        4 => "open tcp",
        5 => "tcp open timeout",
        6 => "tcp open failed",
        7 => "tcp send failed",
        8 => "timeout",
        9 => "response too large",
        10 => "io",
        _ => "unknown",
    }
}

pub(crate) async fn run_bench_fs(io: &dyn ShellBackend, disk: crate::disc::block::DeviceHandle) {
    const BENCH_PATH: &str = "bench-lorem-100mb.txt";
    const BENCH_TOTAL_BYTES: u64 = 100 * 1024 * 1024;
    const UPDATE_MS: u64 = 250;
    const PATTERN: &[u8] = b"10101010";
    const CONTROL_PERIOD_CHUNKS: u32 = 8;

    let Some(placement) = crate::v::fs::trueosfs::locate_async(disk).await.ok().flatten() else {
        io.write_str("bench: selected disk is not TRUEOSFS\r\n");
        return;
    };
    if !disk.supports_write() {
        io.write_str("bench: selected disk is read-only\r\n");
        return;
    }

    io.write_fmt(format_args!(
        "bench: target={} super_lba={} data_lba={} file=/{}\r\n",
        disk.id(),
        placement.super_lba,
        placement.data_lba,
        BENCH_PATH
    ));
    io.write_str("bench: writing 100MB fs stream (press any key to abort)\r\n");

    let info = disk.info();
    let bench_chunk_bytes = if info.max_transfer_bytes > 0 {
        let max_transfer = info.max_transfer_bytes as usize;
        core::cmp::max(4 * 1024, core::cmp::min(max_transfer, 1024 * 1024))
    } else {
        256 * 1024
    };

    let Some(stream_handle) = (match crate::v::fs::trueosfs::file_write_begin_async(
        disk,
        BENCH_PATH,
        BENCH_TOTAL_BYTES,
    )
    .await
    {
        Ok(v) => v,
        Err(e) => {
            io.write_fmt(format_args!("bench: begin failed ({:?})\r\n", e));
            return;
        }
    }) else {
        io.write_str("bench: begin failed (no space / no placement)\r\n");
        return;
    };

    let mut chunk: alloc::vec::Vec<u8> = alloc::vec![0u8; bench_chunk_bytes];
    if !PATTERN.is_empty() {
        let mut off = 0usize;
        while off < chunk.len() {
            let take = core::cmp::min(PATTERN.len(), chunk.len() - off);
            chunk[off..off + take].copy_from_slice(&PATTERN[..take]);
            off = off.saturating_add(take);
        }
    }

    let mut written: u64 = 0;
    let mut aborted = false;
    let mut write_err: Option<crate::disc::block::Error> = None;
    let mut finished_ok = false;
    let mut chunk_count: u32 = 0;

    let start_tick = embassy_time_driver::now();
    let mut next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);

    while written < BENCH_TOTAL_BYTES {
        let remaining = (BENCH_TOTAL_BYTES - written) as usize;
        let n = core::cmp::min(remaining, chunk.len());

        if let Err(e) = crate::v::fs::trueosfs::file_write_chunk_async(stream_handle, &chunk[..n]).await {
            write_err = Some(e);
            break;
        }
        written = written.saturating_add(n as u64);
        chunk_count = chunk_count.wrapping_add(1);

        if (chunk_count % CONTROL_PERIOD_CHUNKS) == 0 && io.read_byte().is_some() {
            aborted = true;
            break;
        }

        if Instant::now() >= next_update || written >= BENCH_TOTAL_BYTES {
            let now_tick = embassy_time_driver::now();
            let elapsed_ticks = now_tick.saturating_sub(start_tick);
            let hz = embassy_time_driver::TICK_HZ as u64;
            let elapsed_ms = if hz == 0 {
                0
            } else {
                elapsed_ticks.saturating_mul(1000) / hz
            };
            let bps = if elapsed_ms == 0 {
                0
            } else {
                written.saturating_mul(1000) / elapsed_ms
            };
            let kbps = bps / 1024;
            let total_kb = written / 1024;
            io.write_fmt(format_args!(
                "\rwrite speed: {} kb/sec | {} KB total   ",
                kbps,
                total_kb
            ));
            next_update = Instant::now() + EmbassyDuration::from_millis(UPDATE_MS);
        }
    }

    if write_err.is_none() && !aborted {
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

    io.write_str("\r\n");
    if aborted {
        io.write_str("bench: aborted by key press\r\n");
    }
    if let Some(e) = write_err {
        io.write_fmt(format_args!("bench: write failed ({:?})\r\n", e));
    } else if finished_ok {
        io.write_str("bench: write complete\r\n");
    }

    let expected_absent = aborted || write_err.is_some() || !finished_ok;
    match crate::v::fs::trueosfs::file_delete_async(disk, BENCH_PATH).await {
        Ok(true) => io.write_str("bench: cleanup ok (deleted benchmark file)\r\n"),
        Ok(false) if expected_absent => {
            io.write_str("bench: cleanup: nothing to delete (expected for aborted/failed run)\r\n")
        }
        Ok(false) => io.write_str("bench: cleanup: benchmark file not present\r\n"),
        Err(e) => io.write_fmt(format_args!("bench: cleanup failed ({:?})\r\n", e)),
    }
}

pub(crate) fn netbench_start(spawner: &Spawner, nic_index: usize) -> bool {
    if NETBENCH_STATE.load(Ordering::Relaxed) == NETBENCH_RUNNING {
        return false;
    }
    let old_slot = NETBENCH_STATUS_SLOT.swap(u8::MAX, Ordering::Relaxed);
    if old_slot != u8::MAX {
        let _ = crate::matrix::free_slot(old_slot);
    }
    if let Some(slot) = crate::matrix::alloc_slot("netbench") {
        NETBENCH_STATUS_SLOT.store(slot, Ordering::Relaxed);
        let _ = crate::shell::statusbar::set_active_slot(slot);
        let _ = crate::shell::statusbar::set_left(slot, "netbench");
        let _ = crate::shell::statusbar::set_right(slot, "starting");
        for i in 0..crate::shell::statusbar::INDICATOR_COUNT {
            let _ = crate::shell::statusbar::set_indicator(slot, i, 2);
        }
    }
    NETBENCH_ABORT_REQ.store(false, Ordering::Relaxed);
    NETBENCH_BYTES.store(0, Ordering::Relaxed);
    NETBENCH_FAIL_CODE.store(0, Ordering::Relaxed);
    NETBENCH_START_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
    NETBENCH_END_TICK.store(0, Ordering::Relaxed);
    NETBENCH_STATE.store(NETBENCH_RUNNING, Ordering::Relaxed);
    if spawner.spawn(netbench_worker_task(nic_index)).is_err() {
        NETBENCH_FAIL_CODE.store(10, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return false;
    }
    true
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn netbench_worker_task(nic_index: usize) {
    use alloc::{string::String as AString, vec::Vec};
    use trueos_v::vnet as api;

    const OPEN_TIMEOUT_MS: u64 = 4000;
    const OVERALL_TIMEOUT_MS: u64 = 120000;
    const MAX_CAPTURE_BYTES: usize = 1024 * 1024 * 1024 + 1024 * 1024; // 1GB + 1MB buffer
    const IDLE_YIELD_US: u64 = 100;

    fn parse_http_url(url: &str) -> Option<(AString, u16, AString)> {
        let mut u = url.trim();
        if let Some(rest) = u.strip_prefix("http://") {
            u = rest;
        } else {
            return None;
        }
        let (hostport, path) = match u.split_once('/') {
            Some((a, b)) => (a, alloc::format!("/{}", b)),
            None => (u, alloc::string::String::from("/")),
        };
        if hostport.is_empty() {
            return None;
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
        Some((AString::from(host), port, path))
    }

    fn find_http_header_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| p + 4)
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
                .all(|(a, b)| a.to_ascii_lowercase() == b.to_ascii_lowercase())
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

    let Some((host, port, path)) = parse_http_url(NETBENCH_URL) else {
        NETBENCH_FAIL_CODE.store(1, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    };
    let ip = match crate::v::net::dns::resolve_ipv4_for_device(
        nic_index,
        host.as_str(),
        crate::v::net::dns::DnsConfig::default(),
    )
    .await
    {
        Ok(v) => v,
        Err(_e) => {
            NETBENCH_FAIL_CODE.store(2, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
    };

    let Some(vnet) = crate::v::net::VNet::open_with_event_queue_depth(nic_index, 4096) else {
        NETBENCH_FAIL_CODE.store(3, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    };
    if vnet
        .submit(api::Command::OpenTcpConnect {
            remote: api::EndpointV4 { addr: ip, port },
        })
        .is_err()
    {
        NETBENCH_FAIL_CODE.store(4, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    }

    let open_deadline = Instant::now() + EmbassyDuration::from_millis(OPEN_TIMEOUT_MS);
    let tcp_handle = loop {
        if NETBENCH_ABORT_REQ.load(Ordering::Relaxed) {
            NETBENCH_STATE.store(NETBENCH_ABORTED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
        if Instant::now() >= open_deadline {
            NETBENCH_FAIL_CODE.store(5, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            return;
        }
        if let Some(ev) = vnet.pop_event() {
            match ev {
                api::Event::Opened { handle, kind } if kind == api::SocketKind::Tcp => break handle,
                api::Event::Error { msg: _ } => {
                    NETBENCH_FAIL_CODE.store(6, Ordering::Relaxed);
                    NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
                    NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
                    return;
                }
                _ => {}
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    };

    let request = alloc::format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS netbench\r\nAccept: */*\r\nConnection: close\r\n\r\n",
        path.as_str(),
        host.as_str()
    );
    if vnet
        .submit(api::Command::SendTcp {
            handle: tcp_handle,
            data: api::ByteBuf::from_slice_trunc(request.as_bytes()),
        })
        .is_err()
    {
        let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
        NETBENCH_FAIL_CODE.store(7, Ordering::Relaxed);
        NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
        return;
    }

    let mut overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
    let mut header_bytes: Vec<u8> = Vec::new();
    // We stream data to void to avoid OOM on large files
    let mut header_done = false;
    let mut expected_len: Option<usize> = None;
    let mut received_bytes: usize = 0;
    let mut failed = false;
    let mut fail_code: u8 = 10;
    let mut closed = false;

    loop {
        if NETBENCH_ABORT_REQ.load(Ordering::Relaxed) {
            NETBENCH_STATE.store(NETBENCH_ABORTED, Ordering::Relaxed);
            NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
            break;
        }
        if Instant::now() >= overall_deadline {
            failed = true;
            fail_code = 8;
            break;
        }
        let mut got_event = false;
        while let Some(ev) = vnet.pop_event() {
            got_event = true;
            match ev {
                api::Event::TcpData { handle, data } if handle == tcp_handle => {
                    let bytes = data.as_slice();
                    if !header_done {
                        // Max header size check
                        if header_bytes.len() + bytes.len() > 16 * 1024 {
                             failed = true;
                             fail_code = 9;
                             break;
                        }
                        
                        // Append to header buffer (inefficient but only for headers)
                        header_bytes.extend_from_slice(bytes);
                        
                        if let Some(hend) = find_http_header_end(header_bytes.as_slice()) {
                            header_done = true;
                            expected_len = parse_content_length(&header_bytes[..hend]);
                            
                            // Count body bytes that came with the header
                            let body_len = header_bytes.len() - hend;
                            received_bytes = received_bytes.saturating_add(body_len);
                            NETBENCH_BYTES.store(received_bytes as u64, Ordering::Relaxed);
                            
                            // Free header memory now that we are done
                            header_bytes = Vec::new();
                            
                            if let Some(cl) = expected_len {
                                if received_bytes >= cl {
                                    closed = true;
                                    break;
                                }
                            }
                        }
                    } else {
                        // Streaming mode: simply count the bytes
                        let len = bytes.len();
                        received_bytes = received_bytes.saturating_add(len);
                        NETBENCH_BYTES.store(received_bytes as u64, Ordering::Relaxed);
                        
                        if let Some(cl) = expected_len {
                            if received_bytes >= cl {
                                closed = true;
                                break;
                            }
                        }
                    }
                    overall_deadline = Instant::now() + EmbassyDuration::from_millis(OVERALL_TIMEOUT_MS);
                }
                api::Event::Closed { handle } if handle == tcp_handle => {
                    closed = true;
                    break;
                }
                api::Event::Error { msg: _ } => {
                    failed = true;
                    fail_code = 10;
                    break;
                }
                _ => {}
            }
        }
        if failed {
            break;
        }
        if closed {
            break;
        }

        if !got_event {
            Timer::after(EmbassyDuration::from_micros(IDLE_YIELD_US)).await;
        }
    }

    let _ = vnet.submit(api::Command::Close { handle: tcp_handle });
    if NETBENCH_STATE.load(Ordering::Relaxed) != NETBENCH_ABORTED {
        // Body is already truncated by not storing it :D
        NETBENCH_BYTES.store(received_bytes as u64, Ordering::Relaxed);
        if failed {
            NETBENCH_FAIL_CODE.store(fail_code, Ordering::Relaxed);
            NETBENCH_STATE.store(NETBENCH_FAILED, Ordering::Relaxed);
        } else {
            NETBENCH_STATE.store(NETBENCH_DONE, Ordering::Relaxed);
        }
        NETBENCH_END_TICK.store(embassy_time_driver::now(), Ordering::Relaxed);
    }
}