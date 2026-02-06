/// UDP TX benchmark: send-only, no client/server required.
///
/// Notes:
/// - We use the directed broadcast for the default slirp /24 (10.0.2.255) so smoltcp
///   will emit an L2 broadcast frame without needing ARP/gateway reachability.
/// - UDP has no completion event in vnet today, so we estimate "sent" as:
///     submitted_ok - udp_send_fail_events
#[embassy_executor::task]
pub async fn raw_network_bench_task() {
    crate::log!("bench: udp-bcast starting
");

    // Wait for a NIC to be present
    let vnet = loop {
        if let Some(v) = crate::v::net::VNet::open_primary() {
            break v;
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;
    };

    const LOCAL_PORT: u16 = 40000;
    const REMOTE_PORT: u16 = 9999;
    const REMOTE_ADDR: [u8; 4] = [10, 0, 2, 255];
    const RUN_SECS: u64 = 10;
    const PAYLOAD_BYTES: usize = 1472;
    const YIELD_EVERY_SENDS: u64 = 256;

    let hz: u64 = embassy_time_driver::TICK_HZ as u64;
    if hz == 0 {
        crate::log!("bench: udp-bcast invalid TICK_HZ=0
");
        return;
    }

    if vnet
        .submit(trueos_v::vnet::Command::OpenUdp { port: LOCAL_PORT })
        .is_err()
    {
        crate::log!("bench: udp-bcast open udp failed
");
        return;
    }

    // Wait for Opened(Udp)
    let mut udp_handle: Option<trueos_v::vnet::NetHandle> = None;
    let open_deadline = embassy_time_driver::now().saturating_add(hz.saturating_mul(2));
    while udp_handle.is_none() {
        while let Some(ev) = vnet.pop_event() {
            if let trueos_v::vnet::Event::Opened { handle, kind } = ev {
                if kind == trueos_v::vnet::SocketKind::Udp {
                    udp_handle = Some(handle);
                    crate::log!(
                        "bench: udp-bcast opened handle={} local_port={}
",
                        handle.0,
                        LOCAL_PORT
                    );
                    break;
                }
            }
        }

        if udp_handle.is_some() {
            break;
        }
        if embassy_time_driver::now() >= open_deadline {
            crate::log!("bench: udp-bcast open timeout
");
            return;
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(5)).await;
    }

    let handle = udp_handle.unwrap();
    let remote = trueos_v::vnet::EndpointV4::new(REMOTE_ADDR, REMOTE_PORT);

    let payload = [0x42u8; PAYLOAD_BYTES];
    let payload_buf =
        trueos_v::vnet::ByteBuf::<{ trueos_v::vnet::MAX_MSG }>::from_slice_trunc(&payload);

    let start_ticks = embassy_time_driver::now();
    let end_ticks = start_ticks.saturating_add(hz.saturating_mul(RUN_SECS));
    let mut last_report_ticks = start_ticks;
    let mut next_report_ticks = start_ticks.saturating_add(hz);

    let mut pkts_submitted_ok: u64 = 0;
    let mut submit_errs: u64 = 0;
    let mut udp_send_fails: u64 = 0;

    let mut pkts_sent_est_last: u64 = 0;
    let mut sends_since_yield: u64 = 0;

    crate::log!(
        "bench: udp-bcast sending to {}.{}.{}.{}:{} payload_bytes={} duration_s={}
",
        remote.addr[0],
        remote.addr[1],
        remote.addr[2],
        remote.addr[3],
        remote.port,
        PAYLOAD_BYTES,
        RUN_SECS
    );

    loop {
        let now = embassy_time_driver::now();
        if now >= end_ticks {
            break;
        }

        // Drain events; count only UDP send failures.
        while let Some(ev) = vnet.pop_event() {
            if let trueos_v::vnet::Event::Error { msg } = ev {
                if msg == "udp send fail" {
                    udp_send_fails = udp_send_fails.saturating_add(1);
                }
            }
        }

        match vnet.submit(trueos_v::vnet::Command::SendUdp {
            handle,
            remote,
            data: payload_buf,
        }) {
            Ok(()) => {
                pkts_submitted_ok = pkts_submitted_ok.saturating_add(1);
                sends_since_yield = sends_since_yield.saturating_add(1);
            }
            Err(_) => {
                submit_errs = submit_errs.saturating_add(1);
                embassy_time::Timer::after(embassy_time::Duration::from_micros(0)).await;
            }
        }

        if sends_since_yield >= YIELD_EVERY_SENDS {
            embassy_time::Timer::after(embassy_time::Duration::from_micros(0)).await;
            sends_since_yield = 0;
        }

        let now2 = embassy_time_driver::now();
        if now2 >= next_report_ticks {
            let dt = now2.saturating_sub(last_report_ticks).max(1);

            let pkts_sent_est = pkts_submitted_ok.saturating_sub(udp_send_fails);
            let pkts_delta = pkts_sent_est.saturating_sub(pkts_sent_est_last);

            let pps = ((pkts_delta as u128) * (hz as u128) / (dt as u128)) as u64;
            let bytes_per_sec = pps.saturating_mul(PAYLOAD_BYTES as u64);
            let mbps = (bytes_per_sec.saturating_mul(8)) / 1_000_000;

            crate::log!(
                "bench: udp-bcast submitted_ok={} submit_errs={} udp_send_fails={} sent_est={} bytes_per_sec={} pps={} mbps={}
",
                pkts_submitted_ok,
                submit_errs,
                udp_send_fails,
                pkts_sent_est,
                bytes_per_sec,
                pps,
                mbps
            );

            last_report_ticks = now2;
            next_report_ticks = next_report_ticks.saturating_add(hz);
            pkts_sent_est_last = pkts_sent_est;
        }
    }

    // Final drain before summary
    while let Some(ev) = vnet.pop_event() {
        if let trueos_v::vnet::Event::Error { msg } = ev {
            if msg == "udp send fail" {
                udp_send_fails = udp_send_fails.saturating_add(1);
            }
        }
    }

    let done_ticks = embassy_time_driver::now();
    let dt_total = done_ticks.saturating_sub(start_ticks).max(1);
    let pkts_sent_est = pkts_submitted_ok.saturating_sub(udp_send_fails);

    let pps = ((pkts_sent_est as u128) * (hz as u128) / (dt_total as u128)) as u64;
    let bytes_per_sec = pps.saturating_mul(PAYLOAD_BYTES as u64);
    let mbps = (bytes_per_sec.saturating_mul(8)) / 1_000_000;

    crate::log!(
        "bench: udp-bcast complete submitted_ok={} submit_errs={} udp_send_fails={} sent_est={} bytes_per_sec={} pps={} mbps={}
",
        pkts_submitted_ok,
        submit_errs,
        udp_send_fails,
        pkts_sent_est,
        bytes_per_sec,
        pps,
        mbps
    );
}

/// Raw filesystem benchmark: write a 1 MB file and measure throughput.
#[embassy_executor::task]
pub async fn raw_filesystem_bench_task() {
    crate::log!("bench: raw-filesystem starting\n");

    let disk = match crate::v::fs::trueosfs::primary_root_handle() {
        Some(d) => d,
        None => {
            crate::log!("bench: raw-filesystem no mounted root\n");
            return;
        }
    };

    let test_path = "benchdata";
    let buf = vec![0x42u8; 100 * 1024 * 1024];
    let start_tsc = tsc_now();

    match crate::v::fs::trueosfs::file_in_async(disk, test_path, &buf).await {
        Ok(true) | Ok(false) => {
            let elapsed_tsc = tsc_now().wrapping_sub(start_tsc);
            let elapsed_ms = (elapsed_tsc / 2_400_000).max(1);
            let bytes_written = buf.len() as u64;
            let kb_per_sec = (bytes_written * 1000) / (1024 * elapsed_ms);
            crate::log!(
                "bench: raw-filesystem write bytes_written={} elapsed_ms={} kb_per_sec={}\n",
                bytes_written,
                elapsed_ms,
                kb_per_sec
            );
        }
        Err(rc) => {
            crate::log!("bench: raw-filesystem write error rc={:?}\n", rc);
        }
    }
}

#[inline]
fn tsc_now() -> u64 {
    #[cfg(target_arch = "x86_64")]
    {
        unsafe { core::arch::x86_64::_rdtsc() as u64 }
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        0
    }
}

use alloc::vec;
