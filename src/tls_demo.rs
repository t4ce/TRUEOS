extern crate alloc;

use alloc::{boxed::Box, vec::Vec};
use alloc::vec;
use alloc::sync::Arc;
use alloc::string::ToString;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};
use heapless::String as HString;
use spin::Once;

use core::time::Duration;

use crate::net::adapter::{
    register_app_queues, NetCommand, NetEndpoint, NetEvent, NetHandle, NetQueue, SocketKind,
};

// A known stable TLS endpoint.
// We intentionally use a hard-coded IPv4 to avoid DNS requirements in the demo.
const DEMO_HOST: &str = "example.com";
const DEMO_IP: [u8; 4] = [93, 184, 216, 34];
const DEMO_PORT: u16 = 443;

static TLS_DEMO_JOB_SEQ: AtomicU32 = AtomicU32::new(1);
static TLS_PROVIDER_ONCE: Once<()> = Once::new();

#[derive(Debug)]
struct FixedTimeProvider;

impl rustls::time_provider::TimeProvider for FixedTimeProvider {
    fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
        // 2026-01-27-ish. Used for certificate validation.
        // If the demo endpoint's certificate validity drifts, adjust this constant.
        Some(rustls::pki_types::UnixTime::since_unix_epoch(Duration::from_secs(
            1_769_000_000,
        )))
    }
}

fn leak_str(s: alloc::string::String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

enum DriveOutcome {
    NeedMoreData,
    Closed,
}

fn send_tcp(cmds: &NetQueue<NetCommand>, handle: NetHandle, data: Vec<u8>) {
    let _ = cmds.push(NetCommand::SendTcp { handle, data });
}

fn drive_rustls_unbuffered(
    slot_id: u8,
    conn: &mut rustls::client::UnbufferedClientConnection,
    incoming_tls: &mut Vec<u8>,
    pending_tls_to_send: &mut Option<Vec<u8>>,
    cmds: &NetQueue<NetCommand>,
    tcp_handle: NetHandle,
    host: &str,
    http_sent: &mut bool,
    plaintext: &mut Vec<u8>,
    truncated: &mut bool,
) -> Result<DriveOutcome, ()> {
    use rustls::unbuffered::{ConnectionState, EncodeError, EncryptError};

    // Cap plaintext body capture.
    const MAX_PLAINTEXT: usize = 256 * 1024;

    loop {
        let status = conn.process_tls_records(incoming_tls.as_mut_slice());

        let mut discard = status.discard;
        let mut outcome: Option<DriveOutcome> = None;

        {
            let state = match status.state {
                Ok(s) => s,
                Err(_) => {
                    crate::matrix::push_line(slot_id, "https: tls state machine error");
                    return Err(());
                }
            };

            match state {
                ConnectionState::EncodeTlsData(mut enc) => {
                    let mut out = [0u8; 4096];
                    match enc.encode(&mut out) {
                        Ok(n) => {
                            *pending_tls_to_send = Some(out[..n].to_vec());
                        }
                        Err(EncodeError::InsufficientSize(e)) => {
                            let mut v = vec![0u8; e.required_size];
                            let n = enc.encode(&mut v).map_err(|_| ())?;
                            v.truncate(n);
                            *pending_tls_to_send = Some(v);
                        }
                        Err(_) => return Err(()),
                    }
                }
                ConnectionState::TransmitTlsData(tx) => {
                    if let Some(data) = pending_tls_to_send.take() {
                        if !data.is_empty() {
                            send_tcp(cmds, tcp_handle, data);
                        }
                    }
                    tx.done();
                }
                ConnectionState::ReadTraffic(mut rt) => {
                    while let Some(rec) = rt.next_record() {
                        let rec = rec.map_err(|_| ())?;
                        discard = discard.saturating_add(rec.discard);

                        if plaintext.len() < MAX_PLAINTEXT {
                            let room = MAX_PLAINTEXT - plaintext.len();
                            let take = rec.payload.len().min(room);
                            plaintext.extend_from_slice(&rec.payload[..take]);
                            if take < rec.payload.len() {
                                *truncated = true;
                            }
                        } else {
                            *truncated = true;
                        }
                    }
                }
                ConnectionState::ReadEarlyData(mut rt) => {
                    // Client-side connections should not yield early-data reads
                    // (we don't enable 0-RTT in this demo). Keep the state machine
                    // progressing by requesting more data.
                    let _ = &mut rt;
                    outcome = Some(DriveOutcome::NeedMoreData);
                }
                ConnectionState::WriteTraffic(mut wt) => {
                    if !*http_sent {
                        let req = alloc::format!(
                            "GET / HTTP/1.1\r\nHost: {}\r\nUser-Agent: TRUEOS rustls demo\r\nAccept: */*\r\nConnection: close\r\n\r\n",
                            host
                        );

                        let mut out = [0u8; 4096];
                        let encrypted = match wt.encrypt(req.as_bytes(), &mut out) {
                            Ok(n) => out[..n].to_vec(),
                            Err(EncryptError::InsufficientSize(e)) => {
                                let mut v = vec![0u8; e.required_size];
                                let n = wt.encrypt(req.as_bytes(), &mut v).map_err(|_| ())?;
                                v.truncate(n);
                                v
                            }
                            Err(_) => return Err(()),
                        };

                        if !encrypted.is_empty() {
                            send_tcp(cmds, tcp_handle, encrypted);
                        }

                        *http_sent = true;
                        crate::matrix::push_line(slot_id, "https: sent https request");
                    }

                    // This is a stable "ready" state: we now need either more network data
                    // (to get ReadTraffic) or the peer closing the connection.
                    outcome = Some(DriveOutcome::NeedMoreData);
                }
                ConnectionState::BlockedHandshake => {
                    outcome = Some(DriveOutcome::NeedMoreData);
                }
                ConnectionState::PeerClosed | ConnectionState::Closed => {
                    outcome = Some(DriveOutcome::Closed);
                }

                // `ConnectionState` is `#[non_exhaustive]`.
                _ => {
                    outcome = Some(DriveOutcome::NeedMoreData);
                }
            }
        }

        if discard > 0 {
            let discard = discard.min(incoming_tls.len());
            incoming_tls.drain(0..discard);
        }

        if let Some(outcome) = outcome {
            return Ok(outcome);
        }
    }
}

#[task]
pub async fn tls_demo_matrix_job(slot_id: u8, host_arg: HString<96>) {
    crate::matrix::push_line(slot_id, "https: rustls demo starting");

    TLS_PROVIDER_ONCE.call_once(|| {
        // Provide rustls with a crypto backend that works without `std`.
        // (This demo is a proof of wiring; not intended for production use.)
        let _ = rustls::crypto::CryptoProvider::install_default(rustls_rustcrypto::provider());
    });

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
    let mut incoming_tls: Vec<u8> = Vec::new();
    let mut pending_tls_to_send: Option<Vec<u8>> = None;

    // Build rustls config + connection.
    // Note: crypto provider selection is handled by rustls features in Cargo.toml.
    let server_name = match rustls::pki_types::ServerName::try_from(host) {
        Ok(s) => s,
        Err(_) => {
            crate::matrix::push_line(slot_id, "https: invalid server name");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    let mut roots = rustls::RootCertStore::empty();
    // webpki-roots provides the Mozilla root set.
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    let provider = Arc::new(rustls_rustcrypto::provider());
    let time_provider = Arc::new(FixedTimeProvider);
    let roots = Arc::new(roots);

    let config = match rustls::client::ClientConfig::builder_with_details(provider, time_provider)
        .with_safe_default_protocol_versions()
    {
        Ok(b) => b.with_root_certificates(roots).with_no_client_auth(),
        Err(_) => {
            crate::matrix::push_line(slot_id, "https: rustls config builder failed");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    let config = alloc::sync::Arc::new(config);

    let mut conn = match rustls::client::UnbufferedClientConnection::new(config, server_name) {
        Ok(c) => c,
        Err(_) => {
            crate::matrix::push_line(slot_id, "https: rustls UnbufferedClientConnection failed");
            crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
            return;
        }
    };

    crate::matrix::push_line(slot_id, "https: opening tcp");

    let deadline = Instant::now() + EmbassyDuration::from_secs(15);

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

                    // Kick off the handshake by letting rustls emit its initial ClientHello.
                    if drive_rustls_unbuffered(
                        slot_id,
                        &mut conn,
                        &mut incoming_tls,
                        &mut pending_tls_to_send,
                        cmds,
                        handle,
                        host,
                        &mut http_sent,
                        &mut plaintext,
                        &mut truncated,
                    )
                    .is_err()
                    {
                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                        let _ = cmds.push(NetCommand::Close { handle });
                        return;
                    }
                }
                NetEvent::TcpData { handle, data } => {
                    if tcp_handle != Some(handle) {
                        continue;
                    }

                    incoming_tls.extend_from_slice(&data);

                    if drive_rustls_unbuffered(
                        slot_id,
                        &mut conn,
                        &mut incoming_tls,
                        &mut pending_tls_to_send,
                        cmds,
                        handle,
                        host,
                        &mut http_sent,
                        &mut plaintext,
                        &mut truncated,
                    )
                    .is_err()
                    {
                        crate::matrix::push_line(slot_id, "https: tls error");
                        crate::matrix::set_state(slot_id, crate::matrix::SlotState::Failed);
                        let _ = cmds.push(NetCommand::Close { handle });
                        return;
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
