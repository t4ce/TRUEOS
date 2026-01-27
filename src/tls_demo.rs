extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use alloc::string::ToString;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use crate::net::adapter::{
    register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind,
};

use crate::tls::{TlsClient, TlsClientConfig, TlsError, TlsRoots, TlsRng, TlsTime};

// A known stable TLS endpoint.
// We intentionally use a hard-coded IPv4 to avoid DNS requirements in the demo.
const DEMO_HOST: &str = "example.com";
const DEMO_IP: [u8; 4] = [93, 184, 216, 34];
const DEMO_PORT: u16 = 443;

static TLS_DEMO_JOB_SEQ: AtomicU32 = AtomicU32::new(1);

#[derive(Debug)]
struct FixedTime;

impl TlsTime for FixedTime {
    fn unix_time_seconds(&self) -> Option<u64> {
        // 2026-01-27-ish. Used for certificate validation.
        // If the demo endpoint's certificate validity drifts, adjust this constant.
        Some(1_769_000_000)
    }
}

static FIXED_TIME: FixedTime = FixedTime;

#[derive(Debug)]
struct DemoRng;

impl TlsRng for DemoRng {
    fn fill(&mut self, out: &mut [u8]) -> Result<(), TlsError> {
        getrandom::getrandom(out).map_err(|_| TlsError::Io)
    }
}

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn send_tcp(cmds: &NetQueue<NetCommand>, handle: NetHandle, data: Vec<u8>) {
    let _ = cmds.push(NetCommand::SendTcp { handle, data });
}

#[task]
pub async fn tls_demo_matrix_job(slot_id: u8, host_arg: HString<96>) {
    crate::matrix::push_line(slot_id, "https: rustls demo starting");

    if crate::net::mac_address().is_none() {
        crate::matrix::push_line(slot_id, "https: disabled (no NIC)");
        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
        return;
    }

    let host: &'static str = if host_arg.is_empty() {
        DEMO_HOST
    } else {
        leak_str(host_arg.as_str().to_string())
    };

    // Hardcoded IP (demo endpoint is known-good). If user overrides host, we still
    // connect to DEMO_IP; the demo is a proof of TLS plumbing, not DNS.
    let ip = DEMO_IP;
    let port = DEMO_PORT;

    let seq = TLS_DEMO_JOB_SEQ.fetch_add(1, Ordering::Relaxed);
    let owner = leak_str(alloc::format!("net-tlsdemo-{}-{}", slot_id + 1, seq));
    let cmds_name = leak_str(alloc::format!("{}-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-evt", owner));
    let cmds = NetQueue::new_leaked(cmds_name, 128);
    let events = NetQueue::new_leaked(evts_name, 128);
    register_app_queues(owner, cmds, events);

    let mut tcp_handle: Option<NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;

    let mut plaintext: Vec<u8> = Vec::new();
    let mut truncated = false;

    // Build TLS config + connection via `crate::tls`.
    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);

    let mut rng = DemoRng;
    let mut tls = match TlsClient::new(&cfg, &roots, host, &mut rng, &FIXED_TIME, None) {
        Ok(c) => c,
        Err(e) => {
            crate::matrix::push_line(slot_id, "https: tls client init failed");
            crate::log!("tls_demo: init failed: {:?}\n", e);
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    crate::matrix::push_line(slot_id, "https: opening tcp");

    let deadline = Instant::now() + EmbassyDuration::from_secs(15);

    // Cap plaintext body capture.
    const MAX_PLAINTEXT: usize = 256 * 1024;

    loop {
        for ev in events.drain(32) {
            match ev {
                NetEvent::Opened { handle, kind } => {
                    if kind == SocketKind::Tcp {
                        tcp_handle = Some(handle);
                        crate::matrix::push_line(slot_id, "https: tcp opened");
                    }
                }
                NetEvent::TcpEstablished { handle } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }
                    crate::matrix::push_line(slot_id, "https: tcp established");

                    // Send initial ClientHello.
                    match tls.take_ciphertext_to_send() {
                        Ok(data) => {
                            if !data.is_empty() {
                                send_tcp(cmds, handle, data);
                            }
                        }
                        Err(e) => {
                            crate::log!("tls_demo: take_ciphertext_to_send failed: {:?}\n", e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            let _ = cmds.push(NetCommand::Close { handle });
                            return;
                        }
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }

                    let produced = match tls.ingest_encrypted(&data) {
                        Ok(p) => p,
                        Err(e) => {
                            crate::matrix::push_line(slot_id, "https: tls error");
                            crate::log!("tls_demo: ingest_encrypted failed: {:?}\n", e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            let _ = cmds.push(NetCommand::Close { handle });
                            return;
                        }
                    };

                    // Capture plaintext.
                    if !produced.is_empty() {
                        if plaintext.len() < MAX_PLAINTEXT {
                            let room = MAX_PLAINTEXT - plaintext.len();
                            let take = produced.len().min(room);
                            plaintext.extend_from_slice(&produced[..take]);
                            if take < produced.len() {
                                truncated = true;
                            }
                        } else {
                            truncated = true;
                        }
                    }

                    // Send any TLS flight generated by processing this data.
                    match tls.take_ciphertext_to_send() {
                        Ok(out) => {
                            if !out.is_empty() {
                                send_tcp(cmds, handle, out);
                            }
                        }
                        Err(e) => {
                            crate::log!("tls_demo: take_ciphertext_to_send failed: {:?}\n", e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            let _ = cmds.push(NetCommand::Close { handle });
                            return;
                        }
                    }

                    // Once connected, send the HTTPS request exactly once.
                    if tls.is_connected() && !http_sent {
                        let req = alloc::format!(
                            "GET / HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS rustls demo\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            host
                        );
                        if let Err(e) = tls.write_plaintext(req.as_bytes()) {
                            crate::log!("tls_demo: write_plaintext failed: {:?}\n", e);
                            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                            let _ = cmds.push(NetCommand::Close { handle });
                            return;
                        }
                        match tls.take_ciphertext_to_send() {
                            Ok(out) => {
                                if !out.is_empty() {
                                    send_tcp(cmds, handle, out);
                                }
                            }
                            Err(e) => {
                                crate::log!("tls_demo: take_ciphertext_to_send failed: {:?}\n", e);
                                crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                                let _ = cmds.push(NetCommand::Close { handle });
                                return;
                            }
                        }
                        http_sent = true;
                        crate::matrix::push_line(slot_id, "https: sent https request");
                    }
                }
                NetEvent::Closed { handle } => {
                    if tcp_handle == Some(handle) {
                        tcp_handle = None;

                        let line = alloc::format!(
                            "https: plaintext bytes={}{}",
                            plaintext.len(),
                            if truncated { " (truncated)" } else { "" }
                        );
                        crate::matrix::push_line(slot_id, line.as_str());

                        // Store whatever plaintext we captured for inspection.
                        let _ = crate::matrix::set_blob_owned_with_preview(slot_id, plaintext);
                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Done);
                        return;
                    }
                }
                NetEvent::Error { msg } => {
                    let _ = msg;
                }
                NetEvent::TcpSent { .. } => {}
                NetEvent::UdpPacket { .. } => {}
            }
        }

        if !sent_connect {
            let _ = cmds.push(NetCommand::OpenTcpConnect {
                remote: NetEndpoint { addr: ip, port },
            });
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "https: timed out");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            if let Some(h) = tcp_handle {
                let _ = cmds.push(NetCommand::Close { handle: h });
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
