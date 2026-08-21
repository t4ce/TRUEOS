//! Deferred, paired TCP throughput probe for the kernel network path.
//!
//! Two independent Embassy tasks connect to the Linux peer. One consumes an
//! unpaced byte stream while the other keeps a bounded window of outgoing
//! `SendTcp` commands in flight. The peer's received-byte counter is the
//! authoritative upload measurement because `TcpSent` means admitted to
//! smoltcp, not acknowledged on the wire.

use trueos_executor::task;
use trueos_time::{Duration, Instant, Timer};
use v::vnet as api;

use crate::r::net::VNet;

const PEER_IPV4: [u8; 4] = crate::allports::local_assets::HOST_IPV4;
const PEER_PORT: u16 = crate::allports::services::NET_THROUGHPUT_BENCH_TCP_PORT;
const START_DELAY_MS: u64 = crate::allcaps::net::THROUGHPUT_BENCH_START_DELAY_MS;
const DURATION_MS: u64 = crate::allcaps::net::THROUGHPUT_BENCH_DURATION_MS;
const CONNECT_TIMEOUT_MS: u64 = crate::allcaps::net::THROUGHPUT_BENCH_CONNECT_TIMEOUT_MS;
const TX_INFLIGHT_BYTES: usize = crate::allcaps::net::THROUGHPUT_BENCH_TX_INFLIGHT_BYTES;
const REPORT_INTERVAL_MS: u64 = 1_000;

struct RateMeter {
    started: Instant,
    last: Instant,
    total: u64,
    last_total: u64,
}

impl RateMeter {
    fn new() -> Self {
        let now = Instant::now();
        Self {
            started: now,
            last: now,
            total: 0,
            last_total: 0,
        }
    }

    fn add(&mut self, bytes: usize) {
        self.total = self.total.saturating_add(bytes as u64);
    }

    fn report_if_due(&mut self, direction: &str) {
        let now = Instant::now();
        let elapsed_ms = now.saturating_duration_since(self.last).as_millis();
        if elapsed_ms < REPORT_INTERVAL_MS {
            return;
        }

        let delta = self.total.saturating_sub(self.last_total);
        let instant_bps = delta.saturating_mul(8).saturating_mul(1_000) / elapsed_ms.max(1);
        let total_ms = now
            .saturating_duration_since(self.started)
            .as_millis()
            .max(1);
        let average_bps = self.total.saturating_mul(8).saturating_mul(1_000) / total_ms;
        crate::log_info!(target: "net";
            "netbench-pair: {} bytes={} instant={}.{:02}Mbit/s average={}.{:02}Mbit/s\n",
            direction,
            self.total,
            instant_bps / 1_000_000,
            (instant_bps % 1_000_000) / 10_000,
            average_bps / 1_000_000,
            (average_bps % 1_000_000) / 10_000
        );
        self.last = now;
        self.last_total = self.total;
    }

    fn report_final(&self, direction: &str) {
        let elapsed_ms = Instant::now()
            .saturating_duration_since(self.started)
            .as_millis()
            .max(1);
        let average_bps = self.total.saturating_mul(8).saturating_mul(1_000) / elapsed_ms;
        crate::log_info!(target: "net";
            "netbench-pair: {} complete bytes={} elapsed_ms={} average={}.{:02}Mbit/s\n",
            direction,
            self.total,
            elapsed_ms,
            average_bps / 1_000_000,
            (average_bps % 1_000_000) / 10_000
        );
    }
}

async fn connect(net: &VNet, direction: &str) -> Option<api::NetHandle> {
    let remote = api::EndpointV4::new(PEER_IPV4, PEER_PORT);
    if net.submit(api::Command::OpenTcpConnect { remote }).is_err() {
        crate::log_warn!(target: "net";
            "netbench-pair: {} connect submit failed\n",
            direction
        );
        return None;
    }

    let deadline = Instant::now() + Duration::from_millis(CONNECT_TIMEOUT_MS);
    let mut opened = None;
    loop {
        while let Some(event) = net.pop_event() {
            match event {
                api::Event::Opened {
                    handle,
                    kind: api::SocketKind::Tcp,
                } => opened = Some(handle),
                api::Event::TcpEstablished { handle, .. }
                    if opened.is_none() || opened == Some(handle) =>
                {
                    crate::log_info!(target: "net";
                        "netbench-pair: {} connected handle={} peer={}.{}.{}.{}:{}\n",
                        direction,
                        handle.0,
                        PEER_IPV4[0],
                        PEER_IPV4[1],
                        PEER_IPV4[2],
                        PEER_IPV4[3],
                        PEER_PORT
                    );
                    return Some(handle);
                }
                api::Event::Error { .. } | api::Event::Closed { .. } => {
                    crate::log_warn!(target: "net";
                        "netbench-pair: {} connect failed\n",
                        direction
                    );
                    return None;
                }
                _ => {}
            }
        }

        if Instant::now() >= deadline {
            crate::log_warn!(target: "net";
                "netbench-pair: {} connect timeout peer={}.{}.{}.{}:{}\n",
                direction,
                PEER_IPV4[0],
                PEER_IPV4[1],
                PEER_IPV4[2],
                PEER_IPV4[3],
                PEER_PORT
            );
            return None;
        }
        Timer::after(Duration::from_micros(0)).await;
    }
}

fn close(net: &VNet, handle: api::NetHandle) {
    let _ = net.submit(api::Command::Close { handle });
}

#[task]
pub async fn throughput_rx_task() {
    Timer::after(Duration::from_millis(START_DELAY_MS)).await;
    let Some(net) = VNet::open_primary() else {
        crate::log_warn!(target: "net"; "netbench-pair: rx no primary NIC\n");
        return;
    };
    let Some(handle) = connect(&net, "rx").await else {
        return;
    };
    let hello = format!("TRUEOS-BENCH/1 RX {}\n", DURATION_MS);
    if net.send_tcp_all(handle, hello.as_bytes()).is_err() {
        crate::log_warn!(target: "net"; "netbench-pair: rx hello failed\n");
        close(&net, handle);
        return;
    }

    let mut meter = RateMeter::new();
    let deadline = Instant::now() + Duration::from_millis(DURATION_MS + 5_000);
    let mut closed = false;
    while Instant::now() < deadline && !closed {
        let mut did_work = false;
        while let Some(event) = net.pop_event() {
            did_work = true;
            match event {
                api::Event::TcpData {
                    handle: event_handle,
                    data,
                } if event_handle == handle => {
                    meter.add(data.len());
                }
                api::Event::Closed {
                    handle: event_handle,
                } if event_handle == handle => {
                    closed = true;
                    break;
                }
                api::Event::Error { .. } => {
                    crate::log_warn!(target: "net"; "netbench-pair: rx network error\n");
                    closed = true;
                    break;
                }
                _ => {}
            }
        }
        meter.report_if_due("rx-wire");
        Timer::after(Duration::from_micros(if did_work { 0 } else { 100 })).await;
    }

    close(&net, handle);
    meter.report_final("rx-wire");
}

#[task]
pub async fn throughput_tx_task() {
    Timer::after(Duration::from_millis(START_DELAY_MS)).await;
    let Some(net) = VNet::open_primary() else {
        crate::log_warn!(target: "net"; "netbench-pair: tx no primary NIC\n");
        return;
    };
    let Some(handle) = connect(&net, "tx").await else {
        return;
    };
    let hello = format!("TRUEOS-BENCH/1 TX {}\n", DURATION_MS);
    if net.send_tcp_all(handle, hello.as_bytes()).is_err() {
        crate::log_warn!(target: "net"; "netbench-pair: tx hello failed\n");
        close(&net, handle);
        return;
    }

    let mut pattern = [0u8; api::MAX_MSG];
    for (index, byte) in pattern.iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(31).wrapping_add(17);
    }
    let payload = api::ByteBuf::from_slice_trunc(&pattern);

    let mut outstanding = hello.len();
    let mut hello_pending = hello.len();
    let mut meter = RateMeter::new();
    let send_deadline = Instant::now() + Duration::from_millis(DURATION_MS);
    let close_deadline = send_deadline + Duration::from_millis(5_000);
    let mut closed = false;

    while Instant::now() < close_deadline && !closed {
        let mut did_work = false;
        while let Some(event) = net.pop_event() {
            did_work = true;
            match event {
                api::Event::TcpSent {
                    handle: event_handle,
                    len,
                } if event_handle == handle => {
                    let len = usize::from(len);
                    outstanding = outstanding.saturating_sub(len);
                    let hello_bytes = hello_pending.min(len);
                    hello_pending -= hello_bytes;
                    meter.add(len.saturating_sub(hello_bytes));
                }
                api::Event::TcpData { .. } => {}
                api::Event::Closed {
                    handle: event_handle,
                } if event_handle == handle => {
                    closed = true;
                    break;
                }
                api::Event::Error { .. } => {
                    crate::log_warn!(target: "net"; "netbench-pair: tx network error\n");
                    closed = true;
                    break;
                }
                _ => {}
            }
        }

        if Instant::now() < send_deadline {
            while outstanding.saturating_add(api::MAX_MSG) <= TX_INFLIGHT_BYTES {
                if net
                    .submit(api::Command::SendTcp {
                        handle,
                        data: payload,
                    })
                    .is_err()
                {
                    break;
                }
                outstanding = outstanding.saturating_add(api::MAX_MSG);
                did_work = true;
            }
        } else if outstanding == 0 {
            break;
        }

        meter.report_if_due("tx-stack-accepted");
        Timer::after(Duration::from_micros(if did_work { 0 } else { 100 })).await;
    }

    close(&net, handle);
    meter.report_final("tx-stack-accepted");
}
