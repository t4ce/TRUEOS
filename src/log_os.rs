use core::fmt;

extern crate alloc;

pub(crate) mod flags {
    use core::sync::atomic::AtomicBool;

    use log::{Level, LevelFilter};
    pub(crate) use log_os_core::{LogArea, LogLevelPolicy};
    use spin::Once;

    pub(crate) const GLOBAL_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const BOOT_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const SERVICE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const NET_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const USB_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const STORAGE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const GFX_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const GPGPU_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const HDA_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const HV_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const APPS_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const EXECUTOR_REALM_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const EXECUTOR_CACHE_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const INTEL_MEDIA_NGIN_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const BLUEPRINT_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);

    pub(crate) const NET_LOG_RX_TAP: bool = false;
    pub(crate) const NET_LOG_TX_TAP: bool = false;
    pub(crate) const NET_LOG_TCP_FLOW: bool = false;
    pub(crate) const NET_LOG_TCP_CONNECT_STATES: bool = false;
    pub(crate) const NET_LOG_TCP_CONNECT_WIRE: bool = false;
    pub(crate) const NET_LOG_TCP_SEND_FLUSH: bool = false;
    pub(crate) const NET_LOG_ARP_RX: bool = false;
    pub(crate) const NET_LOG_DHCP_VERBOSE: bool = false;
    pub(crate) const NET_LOG_IPV6_RA: bool = false;
    pub(crate) const NET_LOG_DHCP6_SAMPLES: usize = 8;
    pub(crate) const VNET_EXERCISE_LOGS: bool = false;
    pub(crate) const R8125_VERBOSE_LOGS: bool = false;
    pub(crate) const BOOT_INFO_LOGS: bool = false;
    pub(crate) const HV_LOGS: bool = true;
    pub(crate) const PORTAL_LOGS: bool = true;
    pub(crate) const HTML_SHACK_VERBOSE: bool = false;
    pub(crate) const HTML_SHACK_IDLE_LOGS: bool = false;
    pub(crate) const INTEL_STAGE1_LOGS: bool = true;
    pub(crate) const INTEL_RENDER_NGIN_LOGS: bool = true;
    pub(crate) const INTEL_RENDER_NGIN_BATCH_LOGS: bool = false;
    pub(crate) const INTEL_CURSOR_PROBE_LOGS: bool = false;
    pub(crate) const INTEL_DISPLAY_NGIN_LOGS: bool = true;
    pub(crate) const HID_DEBUG_REPORT_LOGS: bool = false;
    pub(crate) const USB_MASS_UAS_TRACE_LOGS: bool = false;
    pub(crate) const STORAGE_TRACE_LOGS: bool = false;
    pub(crate) const NVME_VERBOSE: bool = false;
    pub(crate) static BGRT_LOG_ONCE: Once<()> = Once::new();
    pub(crate) static TGA_MISSING_LOG_ONCE: Once<()> = Once::new();
    pub(crate) static TGA_TASK_STARTED_LOG_ONCE: Once<()> = Once::new();
    pub(crate) static USB_LOG_ALL: AtomicBool = AtomicBool::new(true);

    pub(crate) const fn area_log_policy(area: LogArea) -> LogLevelPolicy {
        match area {
            LogArea::Global => GLOBAL_LOG_LEVEL,
            LogArea::Boot => BOOT_LOG_LEVEL,
            LogArea::Service => SERVICE_LOG_LEVEL,
            LogArea::Net => NET_LOG_LEVEL,
            LogArea::Usb => USB_LOG_LEVEL,
            LogArea::Storage => STORAGE_LOG_LEVEL,
            LogArea::Gfx => GFX_LOG_LEVEL,
            LogArea::Gpgpu => GPGPU_LOG_LEVEL,
            LogArea::Hda => HDA_LOG_LEVEL,
            LogArea::Hv => HV_LOG_LEVEL,
            LogArea::Apps => APPS_LOG_LEVEL,
            LogArea::ExecutorRealm => EXECUTOR_REALM_LOG_LEVEL,
            LogArea::ExecutorCache => EXECUTOR_CACHE_LOG_LEVEL,
            LogArea::IntelMediaNgin => INTEL_MEDIA_NGIN_LOG_LEVEL,
            LogArea::Blueprint => BLUEPRINT_LOG_LEVEL,
            _ => log_os_core::default_area_log_policy(area),
        }
    }

    pub(crate) fn area_log_enabled(area: LogArea, level: Level) -> bool {
        log_os_core::level_enabled(area_log_policy(area), level)
    }
}

static LOG_WRITE_LOCK: spin::Mutex<()> = spin::Mutex::new(());

struct TcpLogSink;

impl log_os_core::GlobalLogSink for TcpLogSink {
    fn spec(&self) -> log_os_core::GlobalLogSinkSpec {
        log_os_core::GlobalLogSinkSpec::new(
            log_os_core::LogAreaSet::ALL,
            log_os_core::LogLevelPolicy::up(log::LevelFilter::Trace),
        )
    }

    fn level_policy(&self, area: flags::LogArea) -> log_os_core::LogLevelPolicy {
        flags::area_log_policy(area)
    }

    fn write_accepted(&self, purpose: Option<&str>, args: fmt::Arguments<'_>) {
        write_with_purpose(purpose, args);
    }
}

static TCP_LOG_SINK: TcpLogSink = TcpLogSink;
static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 1] = [&TCP_LOG_SINK];
static TRUEOS_LOG_ROUTER: log_os_core::GlobalLogRouter =
    log_os_core::GlobalLogRouter::new(&TRUEOS_LOG_SINKS);
static KERNEL_LOG_FACADE: log_os_core::GlobalLogFacade<log_os_core::GlobalLogRouter> =
    log_os_core::GlobalLogFacade::new(&TRUEOS_LOG_ROUTER);

#[macro_export]
macro_rules! log {
    (purpose = $purpose:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_area_purpose(
            $crate::log_os::flags::LogArea::Global,
            log::Level::Info,
            Some($purpose),
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log(format_args!($($tt)*));
    }};
}

#[macro_export]
macro_rules! log_trace {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            log::Level::Trace,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            log::Level::Trace,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_debug {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            log::Level::Debug,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            log::Level::Debug,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            log::Level::Info,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            log::Level::Info,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            log::Level::Warn,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            log::Level::Warn,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_error {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            log::Level::Error,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            log::Level::Error,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! audio_probe {
    ($($tt:tt)*) => {{
        $crate::log_os::audio_probe(format_args!($($tt)*));
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    log_os_core::log(&TRUEOS_LOG_ROUTER, args);
}

pub(crate) fn audio_probe(args: fmt::Arguments<'_>) {
    let _guard = LOG_WRITE_LOCK.lock();
    logtotcp::log(format_args!("[audio] "));
    logtotcp::log(args);
}

fn write_with_purpose(purpose: Option<&str>, args: fmt::Arguments<'_>) {
    let _guard = LOG_WRITE_LOCK.lock();

    struct PurposeWriter<'a> {
        purpose: Option<&'a str>,
        wrote_prefix: bool,
    }

    impl fmt::Write for PurposeWriter<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if !self.wrote_prefix {
                if let Some(purpose) = self.purpose {
                    logtotcp::log(format_args!("[{}] ", purpose));
                }
                self.wrote_prefix = true;
            }
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

pub fn log_with_area_level(area: flags::LogArea, level: log::Level, args: fmt::Arguments<'_>) {
    log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, area, level, args);
}

pub fn log_with_area_purpose(
    area: flags::LogArea,
    level: log::Level,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    log_os_core::log_with_area_purpose(&TRUEOS_LOG_ROUTER, area, level, purpose, args);
}

pub fn log_with_target_level(target: &str, level: log::Level, args: fmt::Arguments<'_>) {
    log_os_core::log_with_target_level(&TRUEOS_LOG_ROUTER, target, level, args);
}

pub fn init_log_facade() {
    let _ = log::set_logger(&KERNEL_LOG_FACADE);
    log::set_max_level(log::LevelFilter::Trace);
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

    pub(crate) fn log(args: fmt::Arguments<'_>) {
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
