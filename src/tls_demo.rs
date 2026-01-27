extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use alloc::string::ToString;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;

use crate::net::adapter::{NetEndpoint, NetHandle, NetQueue};
use crate::net::tls_socket::{register_tls_app_queues, TlsCommand, TlsEvent};
use crate::tls::{TlsClientConfig, TlsRoots};

// A known stable TLS endpoint.
// We intentionally use a hard-coded IPv4 to avoid DNS requirements in the demo.
const DEMO_HOST: &str = "example.com";
const DEMO_IP: [u8; 4] = [93, 184, 216, 34];
const DEMO_PORT: u16 = 443;

static TLS_DEMO_JOB_SEQ: AtomicU32 = AtomicU32::new(1);

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
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
    let owner = leak_str(alloc::format!("tlsdemo-{}-{}", slot_id + 1, seq));
    let cmds_name = leak_str(alloc::format!("{}-tls-cmd", owner));
    let evts_name = leak_str(alloc::format!("{}-tls-evt", owner));
    let cmds = NetQueue::new_leaked(cmds_name, 128);
    let events = NetQueue::new_leaked(evts_name, 128);
    register_tls_app_queues(owner, cmds, events);

    let mut tls_handle: Option<NetHandle> = None;
    let mut sent_connect = false;
    let mut http_sent = false;

    let mut plaintext: Vec<u8> = Vec::new();
    let mut truncated = false;

    let roots = TlsRoots::mozilla();
    let cfg = TlsClientConfig::new().with_alpn_protocols(&[b"http/1.1"]);

    crate::matrix::push_line(slot_id, "https: opening tls/tcp");

    let deadline = Instant::now() + EmbassyDuration::from_secs(15);

    // Cap plaintext body capture.
    const MAX_PLAINTEXT: usize = 256 * 1024;

    loop {
        for ev in events.drain(32) {
            match ev {
                TlsEvent::Opened { handle } => {
                    tls_handle = Some(handle);
                    crate::matrix::push_line(slot_id, "https: tcp opened");
                }
                TlsEvent::Connected { handle } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }
                    crate::matrix::push_line(slot_id, "https: tls connected");

                    if !http_sent {
                        let req = alloc::format!(
                            "GET / HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS rustls demo\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            host
                        );
                        let _ = cmds.push(TlsCommand::Send {
                            handle,
                            data: req.into_bytes(),
                        });
                        http_sent = true;
                        crate::matrix::push_line(slot_id, "https: sent https request");
                    }
                }
                TlsEvent::Data { handle, data } => {
                    if tls_handle != Some(handle) {
                        continue;
                    }

                    if !data.is_empty() {
                        if plaintext.len() < MAX_PLAINTEXT {
                            let room = MAX_PLAINTEXT - plaintext.len();
                            let take = data.len().min(room);
                            plaintext.extend_from_slice(&data[..take]);
                            if take < data.len() {
                                truncated = true;
                            }
                        } else {
                            truncated = true;
                        }
                    }

                }
                TlsEvent::Closed { handle } => {
                    if tls_handle == Some(handle) {
                        tls_handle = None;

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
                TlsEvent::Error { msg, .. } => {
                    crate::log!("tls_demo: net error: {}\n", msg);
                }
                TlsEvent::TlsError { err, .. } => {
                    crate::matrix::push_line(slot_id, "https: tls error");
                    crate::log!("tls_demo: tls error: {:?}\n", err);
                    crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                    if let Some(h) = tls_handle {
                        let _ = cmds.push(TlsCommand::Close { handle: h });
                    }
                    return;
                }
            }
        }

        if !sent_connect {
            let _ = cmds.push(TlsCommand::OpenTcpConnect {
                remote: NetEndpoint { addr: ip, port },
                server_name: host,
                cfg: cfg.clone(),
                roots: roots.clone(),
            });
            sent_connect = true;
        }

        if Instant::now() >= deadline {
            crate::matrix::push_line(slot_id, "https: timed out");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            if let Some(h) = tls_handle {
                let _ = cmds.push(TlsCommand::Close { handle: h });
            }
            return;
        }

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
