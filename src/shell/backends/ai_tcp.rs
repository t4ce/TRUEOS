use alloc::{collections::VecDeque, vec::Vec};
use core::fmt::Write;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};

use crate::net::adapter::{
    NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
};
use crate::shell::{ShellBackend, ShellIo};

const AI_TCP_PORT: u16 = 4246;

pub(crate) struct AiTcpBackend;

pub(crate) static AI_TCP_BACKEND: AiTcpBackend = AiTcpBackend;

static AI_TCP_LAST_WAS_CR: AtomicBool = AtomicBool::new(false);
static AI_TCP_BRIDGE_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TCP_REPL_STARTED: AtomicBool = AtomicBool::new(false);
static AI_TCP_CLOSE_REQUEST: AtomicBool = AtomicBool::new(false);

struct AiState {
    handle: Option<NetHandle>,
    rx: VecDeque<u8>,
    tx: VecDeque<u8>,
    eof_pending: bool,
}

static AI_STATE: spin::Mutex<AiState> = spin::Mutex::new(AiState {
    handle: None,
    rx: VecDeque::new(),
    tx: VecDeque::new(),
    eof_pending: false,
});

#[inline]
fn ai_is_connected() -> bool {
    AI_STATE.lock().handle.is_some()
}

fn ai_read_byte() -> Option<u8> {
    let mut st = AI_STATE.lock();
    if let Some(b) = st.rx.pop_front() {
        return Some(b);
    }
    if st.eof_pending {
        st.eof_pending = false;
        return Some(0x04);
    }
    None
}

fn ai_write_bytes(bytes: &[u8]) {
    const MAX_TX: usize = 64 * 1024;
    let mut st = AI_STATE.lock();
    for &b in bytes {
        if st.tx.len() >= MAX_TX {
            let _ = st.tx.pop_front();
        }
        st.tx.push_back(b);
    }
}

#[task]
pub async fn ai_tcp_bridge_task() {
    async move {
        if AI_TCP_BRIDGE_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }

        // Always route the bridge over the primary NIC. If another adapter exists and is broken
        // (e.g. RTL8125 TX wedge), routing bridge output over it can make the bridge look dead.
        // Pin routing to dev0 so bridge I/O stays isolated from secondary NIC failures.
        const OWNER: &str = "ai-qjs@0";
        let cmds = NetQueue::new_leaked("ai-qjs-cmd", 256);
        let events = NetQueue::new_leaked("ai-qjs-evt", 256);
        register_app_queues(OWNER, cmds, events);

        let _ = cmds.push(NetCommand::OpenTcpListen { port: AI_TCP_PORT });
        crate::log!(
            "ai-qjs: listening on tcp {} owner={} (hostfwd localhost:{} -> guest)\n",
            AI_TCP_PORT,
            OWNER,
            AI_TCP_PORT
        );

        let mut pending: Option<Vec<u8>> = None;
        let mut pending_handle: Option<NetHandle> = None;
        let mut pending_ticks: u32 = 0;
        let mut pending_len: usize = 0;
        let mut tx_log_budget: u32 = 12;
        let mut listen_handle: Option<NetHandle> = None;

        loop {
            if AI_TCP_CLOSE_REQUEST.load(Ordering::Acquire) {
                let handle = AI_STATE.lock().handle;
                if let Some(handle) = handle {
                    let _ = cmds.push(NetCommand::Close { handle });
                } else {
                    AI_TCP_CLOSE_REQUEST.store(false, Ordering::Release);
                }
            }

            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } => {
                        if kind == SocketKind::Tcp {
                            listen_handle = Some(handle);
                        }
                    }
                    NetEvent::TcpEstablished { handle } => {
                        let mut st = AI_STATE.lock();
                        st.handle = Some(handle);
                        st.rx.clear();
                        st.eof_pending = false;

                        pending = None;
                        pending_handle = Some(handle);
                        pending_ticks = 0;
                        pending_len = 0;
                        tx_log_budget = 12;

                        crate::log!("ai-qjs: tcp established handle={}\n", handle.0);
                    }
                    NetEvent::TcpData { handle, data } => {
                        let mut st = AI_STATE.lock();
                        if st.handle.is_none() {
                            st.handle = Some(handle);
                            st.eof_pending = false;
                        }
                        if st.handle != Some(handle) {
                            continue;
                        }

                        const MAX_RX: usize = 64 * 1024;
                        for b in data {
                            if st.rx.len() >= MAX_RX {
                                let _ = st.rx.pop_front();
                            }
                            st.rx.push_back(b);
                        }
                    }
                    NetEvent::TcpSent { handle, len } => {
                        if pending_handle != Some(handle) {
                            continue;
                        }

                        let mut st = AI_STATE.lock();
                        for _ in 0..len {
                            let _ = st.tx.pop_front();
                        }
                        pending = None;
                        pending_ticks = 0;
                        pending_len = 0;
                    }
                    NetEvent::Closed { handle } => {
                        let mut st = AI_STATE.lock();
                        if st.handle == Some(handle) {
                            st.handle = None;
                            st.rx.clear();
                            st.eof_pending = true;
                            pending = None;
                            pending_handle = None;
                            pending_ticks = 0;
                            pending_len = 0;
                            crate::log!("ai-qjs: tcp closed handle={}\n", handle.0);
                            AI_TCP_CLOSE_REQUEST.store(false, Ordering::Release);
                        }

                        if listen_handle == Some(handle) {
                            listen_handle = None;
                            let _ = cmds.push(NetCommand::OpenTcpListen { port: AI_TCP_PORT });
                        }
                    }
                    NetEvent::Error { msg } => {
                        crate::log!("ai-qjs: net error {}\n", msg);
                    }
                    NetEvent::UdpPacket { .. } => {}
                    NetEvent::UdpPacketV6 { .. } => {}
                    NetEvent::IcmpReply { .. } => {}
                    NetEvent::IcmpReplyV6 { .. } => {}
                }
            }

            if pending.is_none() {
                let (handle, chunk) = {
                    let st = AI_STATE.lock();
                    match st.handle {
                        None => (None, Vec::new()),
                        Some(handle) => {
                            if st.tx.is_empty() {
                                (Some(handle), Vec::new())
                            } else {
                                let mut v = Vec::with_capacity(1024);
                                for &b in st.tx.iter().take(1024) {
                                    v.push(b);
                                }
                                (Some(handle), v)
                            }
                        }
                    }
                };

                if let Some(handle) = handle
                    && !chunk.is_empty()
                {
                    pending_handle = Some(handle);
                    pending = Some(chunk.clone());
                    pending_ticks = 0;
                    pending_len = chunk.len();

                    if tx_log_budget > 0 {
                        tx_log_budget -= 1;
                        crate::log!("ai-qjs: tx queue handle={} len={}\n", handle.0, pending_len);
                    }

                    if cmds
                        .push(NetCommand::SendTcp {
                            handle,
                            data: chunk,
                        })
                        .is_err()
                    {
                        pending = None;
                        pending_ticks = 0;
                        pending_len = 0;
                        crate::log!("ai-qjs: tx queue full (dropping pending)\n");
                    }
                }
            }

            if pending.is_some() {
                pending_ticks = pending_ticks.wrapping_add(1);
                if pending_ticks == 250 {
                    crate::log!(
                        "ai-qjs: tx stalled (pending_len={}), retrying\n",
                        pending_len
                    );
                    pending = None;
                    pending_ticks = 0;
                    pending_len = 0;
                }
            }

            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
    .await;
}

#[task]
pub async fn ai_qjs_repl_task() {
    async fn run_line_command_session(io: &'static dyn ShellBackend) {
        let mut line: Vec<u8> = Vec::with_capacity(1024);
        let mut ignore_lf = false;

        loop {
            if !ai_is_connected() {
                break;
            }

            match io.read_byte() {
                Some(b) => {
                    if ignore_lf {
                        if b == b'\n' {
                            ignore_lf = false;
                            continue;
                        }
                        ignore_lf = false;
                    }

                    match b {
                        b'\r' | b'\n' => {
                            if b == b'\r' {
                                ignore_lf = true;
                            }

                            let line_len_before = line.len();
                            let mut stripped_nuls: usize = 0;
                            line.retain(|&byte| {
                                if byte == 0x00 {
                                    stripped_nuls += 1;
                                    false
                                } else {
                                    true
                                }
                            });
                            if stripped_nuls != 0 {
                                crate::log!(
                                    "ai-qjs: stripped NULs count={} line_len={}\n",
                                    stripped_nuls,
                                    line_len_before
                                );
                            }

                            let cmd = match core::str::from_utf8(&line) {
                                Ok(s) => s.trim(),
                                Err(_) => {
                                    io.write_str("qjs: invalid UTF-8\r\n");
                                    line.clear();
                                    continue;
                                }
                            };

                            if !cmd.is_empty() {
                                if cmd == ".exit" || cmd == ".quit" {
                                    break;
                                }
                                crate::shell::shellqjs::run(io, cmd).await;
                            }

                            line.clear();
                        }
                        0x04 => break,
                        0x08 | 0x7f => {
                            let _ = line.pop();
                        }
                        _ => {
                            if line.len() < 8192 {
                                line.push(b);
                            }
                        }
                    }
                }
                None => {
                    Timer::after(EmbassyDuration::from_millis(10)).await;
                }
            }
        }
    }

    async move {
        if AI_TCP_REPL_STARTED.swap(true, Ordering::SeqCst) {
            return;
        }

        loop {
            if !ai_is_connected() {
                Timer::after(EmbassyDuration::from_millis(50)).await;
                continue;
            }

            crate::log!("ai-qjs: command bridge attached\n");
            run_line_command_session(&AI_TCP_BACKEND).await;

            // End the TCP session when command session exits (`.exit` / EOF), so scripted
            // clients terminate cleanly instead of relying on external timeout.
            AI_TCP_CLOSE_REQUEST.store(true, Ordering::Release);
            let mut wait_loops: u32 = 0;
            while ai_is_connected() && wait_loops < 500 {
                Timer::after(EmbassyDuration::from_millis(10)).await;
                wait_loops = wait_loops.wrapping_add(1);
            }

            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
    .await;
}

impl ShellIo for AiTcpBackend {
    #[inline]
    fn write_str(&self, s: &str) {
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &AI_TCP_LAST_WAS_CR, |chunk| {
            ai_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_fmt(&self, args: core::fmt::Arguments<'_>) {
        struct Writer;

        impl Write for Writer {
            fn write_str(&mut self, s: &str) -> core::fmt::Result {
                crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &AI_TCP_LAST_WAS_CR, |chunk| {
                    ai_write_bytes(chunk);
                });
                Ok(())
            }
        }

        let _ = Writer.write_fmt(args);
    }

    #[inline]
    fn write_char(&self, ch: char) {
        let mut buf = [0u8; 4];
        let s = ch.encode_utf8(&mut buf);
        crate::shell::crlf::write_bytes_crlf(s.as_bytes(), &AI_TCP_LAST_WAS_CR, |chunk| {
            ai_write_bytes(chunk);
        });
    }

    #[inline]
    fn write_byte(&self, b: u8) {
        crate::shell::crlf::write_bytes_crlf(&[b], &AI_TCP_LAST_WAS_CR, |chunk| {
            ai_write_bytes(chunk);
        });
    }
}

impl ShellBackend for AiTcpBackend {
    #[inline]
    fn read_byte(&self) -> Option<u8> {
        ai_read_byte()
    }
}
