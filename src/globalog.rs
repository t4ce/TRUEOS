use core::fmt;
use log::{LevelFilter, Metadata, Record};

extern crate alloc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogRange {
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogAmount {
    Lines,
    Chars,
}

pub mod range {
    use super::LogRange;
    pub const START: LogRange = LogRange::Start;
    pub const BEGIN: LogRange = LogRange::Start;
    pub const FRONT: LogRange = LogRange::Start;
    pub const END: LogRange = LogRange::End;
    pub const BACK: LogRange = LogRange::End;
    pub const REAR: LogRange = LogRange::End;
}

#[macro_export]
macro_rules! log {
    (purpose = $purpose:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_purpose(Some($purpose), format_args!($($tt)*));
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log(format_args!($($tt)*));
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    log_with_purpose(None, args);
}

pub fn log_with_purpose(purpose: Option<&str>, args: fmt::Arguments<'_>) {
    struct PurposeWriter<'a> {
        purpose: Option<&'a str>,
        wrote_prefix: bool,
    }

    impl fmt::Write for PurposeWriter<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if !self.wrote_prefix {
                if let Some(purpose) = self.purpose {
                    debugcon::log(format_args!("[{}] ", purpose));
                    logtotcp::log(format_args!("[{}] ", purpose));
                    placeholder::log(format_args!("[{}] ", purpose));
                }
                self.wrote_prefix = true;
            }
            debugcon::log(format_args!("{}", s));
            logtotcp::log(format_args!("{}", s));
            placeholder::log(format_args!("{}", s));
            Ok(())
        }
    }

    //crate::usb::truekey::push_fmt(args);
    let mut writer = PurposeWriter {
        purpose,
        wrote_prefix: false,
    };
    let _ = fmt::write(&mut writer, args);
}

struct KernelLogFacade;

impl log::Log for KernelLogFacade {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let purpose = match record.level() {
            log::Level::Trace => "trace",
            log::Level::Debug => "debug",
            log::Level::Info => "info",
            log::Level::Warn => "warn",
            log::Level::Error => "error",
        };
        let module_path = record.module_path().unwrap_or("");
        let target = record.target();
        let is_usb_vendor_log = module_path.contains("crab_usb") || target.contains("crab_usb");
        if is_usb_vendor_log {
            if !crate::logflag::usb_vendor_log_enabled(record.level()) {
                return;
            }
            let rendered = alloc::format!("{}", record.args());
            let rendered = rendered.trim_end();
            log_with_purpose(Some(purpose), format_args!("crabusb: {}\n", rendered));
            return;
        }
        log_with_purpose(Some(purpose), format_args!("{}\n", record.args()));
    }

    fn flush(&self) {}
}

static KERNEL_LOG_FACADE: KernelLogFacade = KernelLogFacade;

pub fn init_log_facade() {
    let _ = log::set_logger(&KERNEL_LOG_FACADE);
    log::set_max_level(LevelFilter::Trace);
}

pub fn log_excerpt(src: &str, range: LogRange, amount: LogAmount, count: usize) {
    let excerpt = match amount {
        LogAmount::Lines => excerpt_lines(src, range, count),
        LogAmount::Chars => excerpt_chars(src, range, count),
    };
    log(format_args!("{}\n", excerpt));
}

fn excerpt_chars(src: &str, range: LogRange, count: usize) -> alloc::string::String {
    if count == 0 || src.is_empty() {
        return alloc::string::String::new();
    }

    match range {
        LogRange::Start => src.chars().take(count).collect(),
        LogRange::End => src
            .chars()
            .rev()
            .take(count)
            .collect::<alloc::vec::Vec<_>>()
            .into_iter()
            .rev()
            .collect(),
    }
}

fn excerpt_lines(src: &str, range: LogRange, count: usize) -> alloc::string::String {
    if count == 0 || src.is_empty() {
        return alloc::string::String::new();
    }

    let lines: alloc::vec::Vec<&str> = src.lines().collect();
    if lines.is_empty() {
        return alloc::string::String::new();
    }

    let slice = match range {
        LogRange::Start => &lines[..core::cmp::min(count, lines.len())],
        LogRange::End => &lines[lines.len().saturating_sub(count)..],
    };

    let mut out = alloc::string::String::new();
    for (idx, line) in slice.iter().enumerate() {
        if idx != 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

pub fn snapshot() -> alloc::vec::Vec<u8> {
    placeholder::snapshot()
}

pub(crate) fn append_raw(bytes: &[u8]) {
    placeholder::write_bytes_raw(bytes);
}

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    debugcon::write_byte_raw(b)
}

#[embassy_executor::task]
pub async fn persist_once_task() {
    use embassy_time::{Duration as EmbassyDuration, Timer};

    const RETRY_MS: u64 = 1_000;

    Timer::after(EmbassyDuration::from_secs(10)).await;

    let snapshot = placeholder::snapshot();
    if snapshot.is_empty() {
        return;
    }

    loop {
        let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
            Timer::after(EmbassyDuration::from_millis(RETRY_MS)).await;
            continue;
        };

        if let Err(e) = persist_snapshot(disk, snapshot.as_slice()).await {
            crate::log!("globalog: persist failed: {:?}\n", e);
            return;
        }

        crate::log!("[globalog] the log has been written\n");
        return;
    }
}

async fn persist_snapshot(
    disk: crate::disc::block::DeviceHandle,
    bytes: &[u8],
) -> Result<(), crate::disc::block::Error> {
    const PATH: &str = "logs/globalog.txt";
    const CHUNK_BYTES: usize = 64 * 1024;

    let Some(handle) =
        crate::r::fs::trueosfs::file_write_begin_async(disk, PATH, bytes.len() as u64).await?
    else {
        return Err(crate::disc::block::Error::Io);
    };

    for chunk in bytes.chunks(CHUNK_BYTES) {
        if let Err(e) = crate::r::fs::trueosfs::file_write_chunk_async(handle, chunk).await {
            let _ = crate::r::fs::trueosfs::file_write_abort_async(handle).await;
            return Err(e);
        }
    }

    crate::r::fs::trueosfs::file_write_finish_async(handle).await
}

pub mod logtotcp {
    use alloc::vec::Vec;
    use core::{cmp::min, fmt};
    use spin::Mutex;

    const MAX_BYTES: usize = 256 * 1024;

    struct TcpLogRing {
        buf: [u8; MAX_BYTES],
        head: usize,
        len: usize,
    }

    impl TcpLogRing {
        const fn new() -> Self {
            Self {
                buf: [0; MAX_BYTES],
                head: 0,
                len: 0,
            }
        }

        #[inline]
        fn write_bytes(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }

            if bytes.len() >= MAX_BYTES {
                let keep = &bytes[bytes.len() - MAX_BYTES..];
                self.buf.copy_from_slice(keep);
                self.head = 0;
                self.len = MAX_BYTES;
                return;
            }

            let first = min(bytes.len(), MAX_BYTES - self.head);
            self.buf[self.head..self.head + first].copy_from_slice(&bytes[..first]);

            let rest = bytes.len() - first;
            if rest != 0 {
                self.buf[..rest].copy_from_slice(&bytes[first..]);
            }

            self.head = (self.head + bytes.len()) % MAX_BYTES;
            self.len = min(self.len + bytes.len(), MAX_BYTES);
        }

        #[inline]
        fn oldest_index(&self) -> usize {
            (self.head + MAX_BYTES - self.len) % MAX_BYTES
        }

        fn drain_bytes(&mut self, max: usize) -> Vec<u8> {
            let take = self.len.min(max);
            if take == 0 {
                return Vec::new();
            }

            let start = self.oldest_index();
            let first = min(take, MAX_BYTES - start);
            let mut out = Vec::with_capacity(take);
            out.extend_from_slice(&self.buf[start..start + first]);
            if take > first {
                out.extend_from_slice(&self.buf[..take - first]);
            }

            self.len -= take;
            out
        }
    }

    static RING: Mutex<TcpLogRing> = Mutex::new(TcpLogRing::new());

    pub(super) fn log(args: fmt::Arguments<'_>) {
        struct Writer;

        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                RING.lock().write_bytes(s.as_bytes());
                Ok(())
            }
        }

        let _ = fmt::write(&mut Writer, args);
    }

    fn drain_bytes(max: usize) -> Vec<u8> {
        RING.lock().drain_bytes(max)
    }

    #[embassy_executor::task]
    pub async fn logtotcp_task() {
        use embassy_time::{Duration as EmbassyDuration, Timer};

        use crate::net::adapter::{
            NetCommand, NetEvent, NetHandle, NetQueue, SocketKind, register_app_queues,
        };
        use crate::r::net::ports;

        const OWNER: &str = "logtotcp";
        const DRAIN_CHUNK: usize = 4096;

        crate::r::readiness::wait_for(crate::r::readiness::NET_CONFIGURED).await;

        let cmds = NetQueue::new_leaked("logtotcp-cmd", 64);
        let events = NetQueue::new_leaked("logtotcp-evt", 64);
        register_app_queues(OWNER, cmds, events);

        let _ = cmds.push(NetCommand::OpenTcpListen {
            port: ports::LOGTOTCP_TCP_PORT,
        });
        crate::log!("logtotcp: listening on tcp {}\n", ports::LOGTOTCP_TCP_PORT);

        let mut tcp_handle: Option<NetHandle> = None;
        let mut conn_handle: Option<NetHandle> = None;
        let mut pending: bool = false;

        loop {
            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } if kind == SocketKind::Tcp => {
                        tcp_handle = Some(handle);
                    }
                    NetEvent::TcpEstablished { handle } => {
                        conn_handle = Some(handle);
                        pending = false;
                        crate::log!("logtotcp: client connected handle={}\n", handle.0);
                    }
                    NetEvent::TcpSent { handle, .. } if conn_handle == Some(handle) => {
                        pending = false;
                    }
                    NetEvent::Closed { handle } => {
                        if conn_handle == Some(handle) {
                            conn_handle = None;
                            pending = false;
                            crate::log!("logtotcp: client disconnected handle={}\n", handle.0);
                        }
                        if tcp_handle == Some(handle) {
                            tcp_handle = None;
                            let _ = cmds.push(NetCommand::OpenTcpListen {
                                port: ports::LOGTOTCP_TCP_PORT,
                            });
                        }
                    }
                    _ => {}
                }
            }

            if !pending {
                if let Some(handle) = conn_handle {
                    let chunk = drain_bytes(DRAIN_CHUNK);
                    if !chunk.is_empty() {
                        if cmds
                            .push(NetCommand::SendTcp {
                                handle,
                                data: chunk,
                            })
                            .is_ok()
                        {
                            pending = true;
                        }
                    }
                }
            }

            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
}

mod debugcon {
    use core::fmt;

    #[inline(always)]
    pub(super) fn write_byte_raw(b: u8) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            crate::portio::outb(0xE9, b)
        };

        #[cfg(target_arch = "aarch64")]
        let _ = b;
    }

    struct Writer;

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            for &b in s.as_bytes() {
                write_byte_raw(b);
            }
            Ok(())
        }
    }

    pub(super) fn log(args: fmt::Arguments<'_>) {
        let _ = fmt::write(&mut Writer, args);
    }
}

mod placeholder {
    use alloc::vec::Vec;
    use core::{cmp::min, fmt};
    use spin::Mutex;

    const BRINGUP_LOG_BYTES: usize = 2 * 1024 * 1024;

    struct BringupLog {
        buf: [u8; BRINGUP_LOG_BYTES],
        head: usize,
        len: usize,
    }

    impl BringupLog {
        const fn new() -> Self {
            Self {
                buf: [0; BRINGUP_LOG_BYTES],
                head: 0,
                len: 0,
            }
        }

        #[inline]
        fn write_bytes(&mut self, bytes: &[u8]) {
            if bytes.is_empty() {
                return;
            }

            if bytes.len() >= BRINGUP_LOG_BYTES {
                let keep = &bytes[bytes.len() - BRINGUP_LOG_BYTES..];
                self.buf.copy_from_slice(keep);
                self.head = 0;
                self.len = BRINGUP_LOG_BYTES;
                return;
            }

            let first = min(bytes.len(), BRINGUP_LOG_BYTES - self.head);
            self.buf[self.head..self.head + first].copy_from_slice(&bytes[..first]);

            let rest = bytes.len() - first;
            if rest != 0 {
                self.buf[..rest].copy_from_slice(&bytes[first..]);
            }

            self.head = (self.head + bytes.len()) % BRINGUP_LOG_BYTES;
            self.len = min(self.len + bytes.len(), BRINGUP_LOG_BYTES);
        }

        #[inline]
        fn oldest_index(&self) -> usize {
            (self.head + BRINGUP_LOG_BYTES - self.len) % BRINGUP_LOG_BYTES
        }

        fn snapshot(&self) -> Vec<u8> {
            if self.len == 0 {
                return Vec::new();
            }

            let start = self.oldest_index();
            let first = min(self.len, BRINGUP_LOG_BYTES - start);
            let mut out = Vec::with_capacity(self.len);
            out.extend_from_slice(&self.buf[start..start + first]);
            if self.len > first {
                out.extend_from_slice(&self.buf[..self.len - first]);
            }
            out
        }
    }

    static BRINGUP_LOG: Mutex<BringupLog> = Mutex::new(BringupLog::new());

    pub(super) fn log(args: fmt::Arguments<'_>) {
        struct Writer;

        impl fmt::Write for Writer {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                BRINGUP_LOG.lock().write_bytes(s.as_bytes());
                Ok(())
            }
        }

        let _ = fmt::write(&mut Writer, args);
    }

    pub(super) fn snapshot() -> Vec<u8> {
        BRINGUP_LOG.lock().snapshot()
    }

    pub(super) fn write_bytes_raw(bytes: &[u8]) {
        BRINGUP_LOG.lock().write_bytes(bytes);
    }
}
