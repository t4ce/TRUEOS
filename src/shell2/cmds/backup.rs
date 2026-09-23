//! Shell-owned whole-disk backup. The only data path is the read-only lease.
use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use chacha20poly1305::{XChaCha20Poly1305, XNonce, KeyInit, aead::AeadInPlace};
use trueos_executor::Spawner;
use trueos_time::{Duration, Instant, Timer};
use crate::disc::block::{BackupLease, DeviceHandle};
use crate::net::adapter::{NetCommand, NetEvent, NetHandle, NetQueue, register_app_queues};
use super::super::{MatrixTarget, ShellBackend2, matrix_target_for_backend,
    matrix_target_interrupted, matrix_targets_same_slot_lifetime,
    print_matrix_target_line, print_shell_line, set_matrix_target_active};
use super::super::shell2_cmd::ParseOutcome;

const PORT: u16 = 4246;
const WINDOW_MS: u64 = 5_000;
const CLIENT_TIMEOUT_MS: u64 = 60_000;
static OWNER: spin::Mutex<Option<MatrixTarget>> = spin::Mutex::new(None);
static STOP: AtomicBool = AtomicBool::new(false);
type Queues = (&'static NetQueue<NetCommand>, &'static NetQueue<NetEvent>);
static QUEUES: spin::Once<Queues> = spin::Once::new();

pub(crate) fn try_parse(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    match rest.trim() {
        "" => super::os::try_parse(spawner, io, "backup"),
        "stop" => {
            let target = matrix_target_for_backend(io);
            if OWNER.lock().as_ref().is_some_and(|owner| matrix_targets_same_slot_lifetime(owner, &target)) {
                STOP.store(true, Ordering::Release);
                print_shell_line(io, "backup: stopping service and restoring disk access");
            } else { print_shell_line(io, "backup: no backup belongs to this shell slot"); }
            ParseOutcome::Handled
        }
        _ => {
            print_shell_line(io, "backup: select a whole disk; `backup stop` ends its session and restores access");
            ParseOutcome::Handled
        }
    }
}

pub(crate) fn submit(spawner: &Spawner, target: MatrixTarget, disk: DeviceHandle) {
    let mut owner = OWNER.lock();
    if owner.is_some() {
        print_matrix_target_line(&target, "backup: another backup session is already open");
        return;
    }
    STOP.store(false, Ordering::Release);
    match backup_task(target.clone(), disk) {
        Ok(token) => {
            *owner = Some(target.clone());
            set_matrix_target_active(&target, true);
            spawner.spawn(token);
        }
        Err(_) => print_matrix_target_line(&target, "backup: task unavailable"),
    }
}
fn stopped(target: &MatrixTarget) -> bool {
    STOP.load(Ordering::Acquire) || matrix_target_interrupted(target)
}
fn now() -> u64 { Instant::now().as_millis() }
fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

struct SessionGuard { target: MatrixTarget, cmds: &'static NetQueue<NetCommand> }
impl Drop for SessionGuard {
    fn drop(&mut self) {
        // CloseAll follows any queued Open, so a cancelled pending listen cannot
        // emerge later. This owner has at most one response in flight.
        while self.cmds.pop().is_some() {}
        let _ = self.cmds.push(NetCommand::CloseAll);
        set_matrix_target_active(&self.target, false);
        *OWNER.lock() = None;
    }
}

#[trueos_executor::task(pool_size = 1)]
async fn backup_task(target: MatrixTarget, disk: DeviceHandle) {
    let &(cmds, events) = QUEUES.call_once(|| {
        let cmds = NetQueue::new_leaked("backup-cmd", 32);
        let events = NetQueue::new_leaked("backup-events", 128);
        register_app_queues("disk-backup", cmds, events);
        (cmds, events)
    });
    let guard = SessionGuard { target: target.clone(), cmds };
    // Discard old event notifications; CloseAll is ordered before the next Open.
    while events.pop().is_some() {}
    let outcome = run(&target, disk, cmds, events).await;
    drop(guard);
    print_matrix_target_line(&target, match outcome {
        Ok(true) => "backup: success — full image saved by the client; disk access restored; service stopped",
        Ok(false) => "backup: stopped — disk access restored; service stopped",
        Err(error) => error,
    });
}

async fn run(target: &MatrixTarget, disk: DeviceHandle, cmds: &'static NetQueue<NetCommand>, events: &'static NetQueue<NetEvent>) -> Result<bool, &'static str> {
    let total = disk.block_count().checked_mul(disk.block_size() as u64).filter(|n| *n != 0)
        .ok_or("backup: invalid disk geometry")?;
    let bs = disk.block_size() as u64;
    if bs == 0 || bs > 256 * 1024 { return Err("backup: unsupported block size"); }
    let chunk = (disk.max_transfer_bytes().min(256 * 1024) / bs * bs) as u32;
    if chunk == 0 { return Err("backup: invalid transfer size"); }
    let mut key = [0u8; 32];
    let mut session = [0u8; 16];
    if !crate::tyche::fill_bytes(&mut key) || !crate::tyche::fill_bytes(&mut session) {
        return Err("backup: secure randomness unavailable; disk left mounted");
    }
    print_matrix_target_line(target, &format!("backup: {} — waiting up to 5 seconds for idle I/O and completed writes", disk.id()));
    let deadline = now() + WINDOW_MS;
    let lease = loop {
        if stopped(target) { return Ok(false); }
        match BackupLease::try_acquire(disk) {
            Ok(lease) => break lease,
            Err(_) if now() < deadline => Timer::after(Duration::from_millis(50)).await,
            Err(_) => return Err("backup: disk stayed busy for 5 seconds; left mounted and unchanged"),
        }
    };
    // Drop order restores the saved mount before reopening ordinary I/O.
    let _mount = crate::r::fs::trueosfs::suspend_backup_mount(disk.id());
    lease.flush().await.map_err(|_| "backup: flush failed; disk access restored; no service started")?;
    let cipher = XChaCha20Poly1305::new((&key).into());
    let ip = crate::net::adapter::ipv4_at(crate::net::primary_device_index());
    let host = ip.map(|a| format!("{}.{}.{}.{}", a[0],a[1],a[2],a[3])).unwrap_or_else(|| String::from("<TRUEOS-IP>"));
    print_matrix_target_line(target, &format!("backup: {} detached and protected; {} bytes; TCP {PORT}; one client", disk.id(), total));
    print_matrix_target_line(target, &format!("backup: client: python3 tools/backup-client.py {host} disk.img --session {}", hex(&session)));
    print_matrix_target_line(target, &format!("backup: temporary key (enter at client prompt): {}", hex(&key)));
    print_matrix_target_line(target, "backup: reconnects resume this session; `backup stop` or Ctrl-C restores access");
    let mut connection: Option<Connection> = None;
    let mut handle: Option<NetHandle> = None;
    let mut opening = false;
    let mut retry_at = 0;
    let mut frontier = 0u64;
    let mut saved = 0u64;
    let mut heartbeat = 0;
    loop {
        if stopped(target) { return Ok(false); }
        if handle.is_none() && !opening && now() >= retry_at {
            cmds.push(NetCommand::OpenTcpListen { port: PORT }).map_err(|_| "backup: network queue full; disk access restored")?;
            opening = true;
        }
        for event in events.drain(32) {
            match event {
                NetEvent::Opened { handle: h, .. } => { handle = Some(h); opening = false; }
                NetEvent::TcpEstablished { handle: h, .. } if handle == Some(h) => {
                    let mut challenge = [0u8; 16];
                    if !crate::tyche::fill_bytes(&mut challenge) { return Err("backup: randomness failed; disk access restored"); }
                    let mut hello = Vec::from(&b"TBKP0001"[..]);
                    hello.extend_from_slice(&challenge);
                    hello.extend_from_slice(&session);
                    hello.extend_from_slice(&total.to_le_bytes());
                    hello.extend_from_slice(&(bs as u32).to_le_bytes());
                    hello.extend_from_slice(&chunk.to_le_bytes());
                    cmds.push(NetCommand::SendTcp { handle: h, data: hello.clone() }).map_err(|_| "backup: network queue full; disk access restored")?;
                    connection = Some(Connection { hello, input: Vec::new(), seq: 0, last: now(), final_sent: false });
                }
                NetEvent::TcpData { handle: h, data } if handle == Some(h) => {
                    let Some(c) = connection.as_mut() else { continue; };
                    if c.input.len() + data.len() > 24 {
                        let _ = cmds.push(NetCommand::Close { handle: h });
                        connection = None;
                        continue;
                    }
                    c.input.extend_from_slice(&data);
                }
                NetEvent::Closed { handle: h } if handle == Some(h) => {
                    handle = None; connection = None; opening = false; retry_at = now() + 250;
                }
                NetEvent::Error { .. } => {
                    // A failed NIC/listen/send is recoverable. Keep the same
                    // lease, image identity and durable progress through outages.
                    if let Some(h) = handle.take() { let _ = cmds.push(NetCommand::Close { handle: h }); }
                    connection = None; opening = false; retry_at = now() + 1_000;
                }
                _ => {}
            }
        }
        if let (Some(h), Some(c)) = (handle, connection.as_mut()) {
            let mut reject = now().saturating_sub(c.last) > CLIENT_TIMEOUT_MS;
            if c.input.len() == 24 {
                let mut request = core::mem::take(&mut c.input);
                let nonce = frame_nonce(&c.hello, c.seq, false);
                if cipher.decrypt_in_place(XNonce::from_slice(&nonce), &c.hello, &mut request).is_err() || request.len() != 8 {
                    reject = true;
                } else {
                    let offset = u64::from_le_bytes(request[..8].try_into().unwrap());
                    if offset == u64::MAX && c.final_sent { return Ok(true); }
                    if offset > frontier || offset > total || offset % bs != 0 || c.final_sent {
                        reject = true;
                    } else {
                        saved = saved.max(offset);
                        let len = (total - offset).min(chunk as u64);
                        let mut data = if len == 0 { Vec::new() } else {
                            let data = lease.read(offset / bs, (len / bs) as usize).await
                                .map_err(|_| "backup: disk read failed; incomplete image; disk access restored")?;
                            if data.len() as u64 != len { return Err("backup: short disk read; incomplete image; disk access restored"); }
                            data
                        };
                        frontier = frontier.max(offset + len);
                        let nonce = frame_nonce(&c.hello, c.seq, true);
                        cipher.encrypt_in_place(XNonce::from_slice(&nonce), &c.hello, &mut data)
                            .map_err(|_| "backup: encryption failed; disk access restored")?;
                        let mut frame = Vec::with_capacity(4 + data.len());
                        frame.extend_from_slice(&(data.len() as u32).to_le_bytes());
                        frame.extend_from_slice(&data);
                        cmds.push(NetCommand::SendTcp { handle: h, data: frame }).map_err(|_| "backup: network queue full; disk access restored")?;
                        c.seq += 1;
                        c.last = now();
                        c.final_sent = len == 0;
                    }
                }
            }
            if reject {
                let _ = cmds.push(NetCommand::Close { handle: h });
                connection = None;
            }
        }
        if now() >= heartbeat {
            let percent = (saved as u128 * 100 / total as u128) as u64;
            print_matrix_target_line(target, &format!("backup: {percent}% saved — {} bytes left — {}", total - saved,
                if connection.is_some() { "client connected" } else { "waiting for connection / reconnect" }));
            heartbeat = now() + 2_000;
        }
        Timer::after(Duration::from_millis(10)).await;
    }
}
struct Connection { hello: Vec<u8>, input: Vec<u8>, seq: u64, last: u64, final_sent: bool }
fn frame_nonce(hello: &[u8], seq: u64, response: bool) -> [u8; 24] {
    let mut nonce = [0u8; 24];
    nonce[..16].copy_from_slice(&hello[8..24]);
    nonce[16..].copy_from_slice(&(seq | if response { 1 << 63 } else { 0 }).to_le_bytes());
    nonce
}
