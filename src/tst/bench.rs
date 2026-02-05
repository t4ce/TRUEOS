/// Raw network benchmark: push bulk TCP data and measure throughput with yielding analysis.
#[embassy_executor::task]
pub async fn raw_network_bench_task() {
    crate::log!("bench: raw-network starting\n");
    
    // Wait for network to be ready
    let vnet = loop {
        if let Some(v) = crate::v::net::VNet::open_primary() {
            break v;
        }
        embassy_time::Timer::after(embassy_time::Duration::from_millis(50)).await;
    };

    // Open a listener on port 9999 for benchmark data
    if vnet.submit(trueos_v::vnet::Command::OpenTcpListen {
        port: 9999,
    }).is_err() {
        crate::log!("bench: raw-network listen failed\n");
        return;
    }

    crate::log!("bench: raw-network listening on tcp 9999\n");

    let mut listener_handle: Option<trueos_v::vnet::NetHandle> = None;
    let mut active_handle: Option<trueos_v::vnet::NetHandle> = None;
    let mut bytes_sent: u64 = 0;
    let mut bytes_submitted: u64 = 0;
    let mut submit_errs: u64 = 0;
    let mut sends_since_yield: u64 = 0;
    let mut start_tsc: u64 = 0;
    let mut yield_count: u64 = 0;
    
    // Configurable batch size: test different yield frequencies
    const YIELD_EVERY_N_SENDS: u64 = 100;

    loop {
        while let Some(ev) = vnet.pop_event() {
            match ev {
                trueos_v::vnet::Event::Opened { handle, kind } => {
                    if kind == trueos_v::vnet::SocketKind::Tcp {
                        listener_handle = Some(handle);
                        crate::log!("bench: raw-network listener opened handle={}\n", handle.0);
                    }
                }
                trueos_v::vnet::Event::TcpEstablished { handle } => {
                    active_handle = Some(handle);
                    bytes_sent = 0;
                    bytes_submitted = 0;
                    submit_errs = 0;
                    sends_since_yield = 0;
                    yield_count = 0;
                    start_tsc = tsc_now();
                    crate::log!("bench: raw-network client connected handle={}\n", handle.0);
                }
                trueos_v::vnet::Event::TcpSent { handle, len } => {
                    if active_handle == Some(handle) {
                        bytes_sent += len as u64;
                        let elapsed_tsc = tsc_now().wrapping_sub(start_tsc);
                        if bytes_sent % (1024 * 1024) == 0 {
                            let elapsed_ms = (elapsed_tsc / 2_400_000).max(1);
                            let kb_per_sec = (bytes_sent * 1000) / (1024 * elapsed_ms);
                            let in_flight = bytes_submitted.saturating_sub(bytes_sent);
                            crate::log!(
                                "bench: raw-network bytes_sent={} bytes_submitted={} in_flight={} submit_errs={} yields={} batch_size={} elapsed_ms={} kb_per_sec={}\n",
                                bytes_sent,
                                bytes_submitted,
                                in_flight,
                                submit_errs,
                                yield_count,
                                YIELD_EVERY_N_SENDS,
                                elapsed_ms,
                                kb_per_sec
                            );
                        }
                    }
                }
                trueos_v::vnet::Event::Closed { handle } => {
                    if active_handle == Some(handle) {
                        let elapsed_tsc = tsc_now().wrapping_sub(start_tsc);
                        let elapsed_ms = (elapsed_tsc / 2_400_000).max(1);
                        let kb_per_sec = (bytes_sent * 1000) / (1024 * elapsed_ms);
                        let in_flight = bytes_submitted.saturating_sub(bytes_sent);
                        crate::log!(
                            "bench: raw-network complete bytes_sent={} bytes_submitted={} in_flight={} submit_errs={} yields={} batch_size={} elapsed_ms={} kb_per_sec={}\n",
                            bytes_sent,
                            bytes_submitted,
                            in_flight,
                            submit_errs,
                            yield_count,
                            YIELD_EVERY_N_SENDS,
                            elapsed_ms,
                            kb_per_sec
                        );
                        active_handle = None;
                    }
                }
                _ => {}
            }
        }

        // If no active connection, generate test data and send
        if active_handle.is_none() {
            embassy_time::Timer::after(embassy_time::Duration::from_millis(100)).await;
        } else if active_handle.is_some() {
            // Send 64 KB chunks in a loop
            let test_data = [0x42u8; 65536];
            for chunk in test_data.chunks(trueos_v::vnet::MAX_MSG) {
                let res = vnet.submit(trueos_v::vnet::Command::SendTcp {
                    handle: active_handle.unwrap(),
                    data: trueos_v::vnet::ByteBuf::from_slice_trunc(chunk),
                });
                if res.is_ok() {
                    bytes_submitted += chunk.len() as u64;
                    sends_since_yield += 1;
                } else {
                    submit_errs += 1;
                }
                
                // Yield periodically based on batch size
                if sends_since_yield >= YIELD_EVERY_N_SENDS {
                    embassy_time::Timer::after(embassy_time::Duration::from_micros(0)).await;
                    yield_count += 1;
                    sends_since_yield = 0;
                }
            }
            embassy_time::Timer::after(embassy_time::Duration::from_millis(10)).await;
        }
    }
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
use trueos_v::vnet;
