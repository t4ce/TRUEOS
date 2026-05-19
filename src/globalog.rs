use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};
use log::{Metadata, Record};

extern crate alloc;

static USB_XHCI_COMPLETION_LAST_LOG_TICK: AtomicU64 = AtomicU64::new(0);
static LOG_WRITE_LOCK: spin::Mutex<()> = spin::Mutex::new(());

#[macro_export]
macro_rules! log {
    (purpose = $purpose:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_purpose(Some($purpose), format_args!($($tt)*));
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log(format_args!($($tt)*));
    }};
}

#[macro_export]
macro_rules! log_trace {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            $target,
            log::Level::Trace,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            "boot",
            log::Level::Trace,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_debug {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            $target,
            log::Level::Debug,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            "boot",
            log::Level::Debug,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            $target,
            log::Level::Info,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            "boot",
            log::Level::Info,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            $target,
            log::Level::Warn,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            "boot",
            log::Level::Warn,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_error {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            $target,
            log::Level::Error,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::globalog::log_with_concept_level(
            "boot",
            log::Level::Error,
            format_args!($($tt)*),
        );
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    log_with_level(log::Level::Info, args);
}

fn inferred_concept_for_rendered(rendered: &str) -> Option<&'static str> {
    let rendered = rendered.trim_start();
    if rendered.starts_with("crabusb:") || rendered.starts_with("crabusb/") {
        return Some("usb");
    }
    None
}

fn is_usb_vendor_metadata(metadata: &Metadata<'_>) -> bool {
    let target = metadata.target();
    target.contains("crab_usb") || target.contains("crab-usb")
}

fn is_usb_vendor_record(record: &Record<'_>) -> bool {
    let module_path = record.module_path().unwrap_or("");
    let target = record.target();
    module_path.contains("crab_usb")
        || module_path.contains("crab-usb")
        || target.contains("crab_usb")
        || target.contains("crab-usb")
}

pub fn log_with_purpose(purpose: Option<&str>, args: fmt::Arguments<'_>) {
    let _guard = LOG_WRITE_LOCK.lock();

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
                }
                self.wrote_prefix = true;
            }
            debugcon::log(format_args!("{}", s));
            logtotcp::log(format_args!("{}", s));
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

pub fn purpose_for_level(level: log::Level) -> &'static str {
    match level {
        log::Level::Trace => "trace",
        log::Level::Debug => "debug",
        log::Level::Info => "info",
        log::Level::Warn => "warn",
        log::Level::Error => "error",
    }
}

pub fn log_with_level(level: log::Level, args: fmt::Arguments<'_>) {
    let rendered = alloc::format!("{}", args);
    if let Some(concept) = inferred_concept_for_rendered(rendered.as_str()) {
        if !crate::logflag::concept_log_enabled(concept, level) {
            return;
        }
    }
    log_with_purpose(Some(purpose_for_level(level)), format_args!("{}", rendered));
}

pub fn log_with_concept_level(concept: &str, level: log::Level, args: fmt::Arguments<'_>) {
    if !crate::logflag::concept_log_enabled(concept, level) {
        return;
    }
    log_with_level(level, args);
}

fn one_second_rate_limit_allows(last_marker: &AtomicU64) -> bool {
    let interval = embassy_time_driver::TICK_HZ.max(1);
    let now = embassy_time_driver::now();
    let now_marker = now.saturating_add(1);
    let mut previous_marker = last_marker.load(Ordering::Relaxed);

    loop {
        if previous_marker != 0 {
            let previous = previous_marker.saturating_sub(1);
            if now >= previous && now.saturating_sub(previous) < interval {
                return false;
            }
        }

        match last_marker.compare_exchange_weak(
            previous_marker,
            now_marker,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return true,
            Err(actual) => previous_marker = actual,
        }
    }
}

fn usb_vendor_rendered_log_allowed(rendered: &str) -> bool {
    if rendered.starts_with("crabusb/xhci/ep: completion") {
        return one_second_rate_limit_allows(&USB_XHCI_COMPLETION_LAST_LOG_TICK);
    }
    true
}

struct KernelLogFacade;

impl log::Log for KernelLogFacade {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        if is_usb_vendor_metadata(metadata) {
            return crate::logflag::usb_log_enabled(metadata.level());
        }
        true
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let purpose = purpose_for_level(record.level());
        if is_usb_vendor_record(record) {
            let rendered = alloc::format!("{}", record.args());
            if !usb_vendor_rendered_log_allowed(rendered.as_str()) {
                return;
            }
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
    log::set_max_level(log::LevelFilter::Trace);
}

#[inline(always)]
pub(crate) fn debugcon_write_byte_raw(b: u8) {
    debugcon::write_byte_raw(b)
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

        crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

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
                    NetEvent::TcpEstablished { handle, .. } => {
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
