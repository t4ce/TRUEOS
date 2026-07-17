use core::fmt;
#[cfg(target_arch = "x86_64")]
use core::sync::atomic::{AtomicBool, Ordering};

extern crate alloc;

pub(crate) mod flags {
    use core::sync::atomic::AtomicBool;

    use log::{Level, LevelFilter};
    pub(crate) use log_os_core::{LogArea, LogLevelPolicy, LogLevelSet};
    use spin::Once;

    // Intel-first GPGPU diagnostic profile. Keep failures, lifecycle summaries,
    // and the lowest-level trace records while deliberately leaving Debug out:
    // LogLevelPolicy::Only lets this profile select non-contiguous levels.
    const GPGPU_DIAG_LEVELS: LogLevelSet = LogLevelSet::ERROR
        .union(LogLevelSet::WARN)
        .union(LogLevelSet::INFO)
        .union(LogLevelSet::TRACE);

    pub(crate) const GLOBAL_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const BOOT_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const SERVICE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const NET_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Info);
    pub(crate) const USB_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const STORAGE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    // GPGPU diagnosis needs the Intel device/display setup that surrounds
    // kernel upload and submission, not just the GPGPU records themselves.
    pub(crate) const GFX_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::only(GPGPU_DIAG_LEVELS);
    pub(crate) const GPGPU_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::only(GPGPU_DIAG_LEVELS);
    pub(crate) const RENDER_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::only(GPGPU_DIAG_LEVELS);
    pub(crate) const HDA_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const HV_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Info);
    // Blueprint log facades remain Info by default and opt individual hunt
    // targets into Debug/Trace before crossing the ABI. Accept those selected
    // records here without opening the kernel network hot paths at Trace.
    pub(crate) const APPS_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
    pub(crate) const EXECUTOR_REALM_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const EXECUTOR_CACHE_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const INTEL_MEDIA_NGIN_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LevelFilter::Warn);
    pub(crate) const BLUEPRINT_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Info);

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
    // Stage1 suppresses the rate-limited present diagnostics when enabled.
    pub(crate) const INTEL_STAGE1_LOGS: bool = false;
    pub(crate) const INTEL_RENDER_NGIN_LOGS: bool = true;
    pub(crate) const INTEL_RENDER_NGIN_BATCH_LOGS: bool = true;
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
            LogArea::Render => RENDER_LOG_LEVEL,
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
#[cfg(target_arch = "x86_64")]
static UART_LOG_WRITE_LOCK: spin::Mutex<()> = spin::Mutex::new(());
#[cfg(target_arch = "x86_64")]
static EMULATOR_UART_LOGGING: AtomicBool = AtomicBool::new(false);

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

    fn write_accepted(
        &self,
        area: flags::LogArea,
        _level: log::Level,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        write_with_tags(area, purpose, args);
    }
}

#[cfg(target_arch = "x86_64")]
struct EmulatorUartLogSink;

#[cfg(target_arch = "x86_64")]
impl log_os_core::GlobalLogSink for EmulatorUartLogSink {
    fn spec(&self) -> log_os_core::GlobalLogSinkSpec {
        log_os_core::GlobalLogSinkSpec::new(
            log_os_core::LogAreaSet::ALL,
            log_os_core::LogLevelPolicy::up(log::LevelFilter::Trace),
        )
    }

    fn level_policy(&self, area: flags::LogArea) -> log_os_core::LogLevelPolicy {
        flags::area_log_policy(area)
    }

    fn accepts(&self, area: flags::LogArea, level: log::Level) -> bool {
        EMULATOR_UART_LOGGING.load(Ordering::Acquire) && flags::area_log_enabled(area, level)
    }

    fn write_accepted(
        &self,
        area: flags::LogArea,
        _level: log::Level,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        write_uart_with_tags(area, purpose, args);
    }
}

static TCP_LOG_SINK: TcpLogSink = TcpLogSink;
#[cfg(target_arch = "x86_64")]
static EMULATOR_UART_LOG_SINK: EmulatorUartLogSink = EmulatorUartLogSink;
#[cfg(target_arch = "x86_64")]
static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 2] =
    [&TCP_LOG_SINK, &EMULATOR_UART_LOG_SINK];
#[cfg(not(target_arch = "x86_64"))]
static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 1] = [&TCP_LOG_SINK];
static TRUEOS_LOG_ROUTER: log_os_core::GlobalLogRouter =
    log_os_core::GlobalLogRouter::new(&TRUEOS_LOG_SINKS);
static KERNEL_LOG_FACADE: log_os_core::GlobalLogFacade<log_os_core::GlobalLogRouter> =
    log_os_core::GlobalLogFacade::new(&TRUEOS_LOG_ROUTER);

#[macro_export]
macro_rules! log {
    (purpose = $purpose:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_purpose(
            module_path!(),
            log::Level::Info,
            Some($purpose),
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            module_path!(),
            log::Level::Info,
            format_args!($($tt)*),
        );
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
        $crate::log_os::log_with_area_purpose(
            $crate::log_os::flags::LogArea::Hda,
            log::Level::Trace,
            Some("audio"),
            format_args!($($tt)*),
        );
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    log_os_core::log(&TRUEOS_LOG_ROUTER, args);
}

pub(crate) fn purpose_for_level(level: log::Level) -> &'static str {
    log_os_core::purpose_for_level(level)
}

pub(crate) fn hypervisor_line(level: log::Level, args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Hv, level, args);
}

pub(crate) fn blueprint_line(level: log::Level, args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Blueprint, level, args);
}

pub(crate) fn printer_discovered(name: &str, uri: &str) {
    log_with_area_level(
        flags::LogArea::Net,
        log::Level::Info,
        format_args!("printer: discovered name={} uri={}\n", name, uri),
    );
}

fn write_with_tags(area: flags::LogArea, purpose: Option<&str>, args: fmt::Arguments<'_>) {
    let _guard = LOG_WRITE_LOCK.lock();

    struct TagWriter<'a> {
        area: flags::LogArea,
        purpose: Option<&'a str>,
        wrote_prefix: bool,
    }

    impl fmt::Write for TagWriter<'_> {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if !self.wrote_prefix {
                logtotcp::log(format_args!("[{}] ", log_os_core::area_tag(self.area)));
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
    let mut writer = TagWriter {
        area,
        purpose,
        wrote_prefix: false,
    };
    let _ = fmt::write(&mut writer, args);
}

#[cfg(target_arch = "x86_64")]
fn write_uart_with_tags(area: flags::LogArea, purpose: Option<&str>, args: fmt::Arguments<'_>) {
    let _guard = UART_LOG_WRITE_LOCK.lock();
    crate::uart1_com1::write_fmt(format_args!("[{}] ", log_os_core::area_tag(area)));
    if let Some(purpose) = purpose {
        crate::uart1_com1::write_fmt(format_args!("[{}] ", purpose));
    }
    crate::uart1_com1::write_fmt(args);
}

#[cfg(target_arch = "x86_64")]
pub(crate) fn set_emulator_uart_logging(enabled: bool) {
    if enabled {
        crate::uart1_com1::init();
    }
    EMULATOR_UART_LOGGING.store(enabled, Ordering::Release);
}

#[cfg(not(target_arch = "x86_64"))]
pub(crate) fn set_emulator_uart_logging(_enabled: bool) {}

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

#[allow(dead_code)]
pub fn log_with_target_purpose(
    target: &str,
    level: log::Level,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    log_os_core::log_with_target_purpose(&TRUEOS_LOG_ROUTER, target, level, purpose, args);
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
        use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

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
        crate::log!(
            "logtotcp: listening on tcp {} ms={}\n",
            ports::LOGTOTCP_TCP_PORT,
            Instant::now().as_millis()
        );

        let mut tcp_handle: Option<NetHandle> = None;
        let mut conn_handle: Option<NetHandle> = None;
        // A chunk is pending only until the adapter command queue accepts it.
        // The adapter owns an internal TCP backlog after that point, so waiting
        // for a TcpSent event here can wedge logging if that notification drops.
        let mut pending_chunk: Option<Vec<u8>> = None;

        loop {
            for ev in events.drain(32) {
                match ev {
                    NetEvent::Opened { handle, kind } if kind == SocketKind::Tcp => {
                        tcp_handle = Some(handle);
                    }
                    NetEvent::TcpEstablished { handle, .. } => {
                        conn_handle = Some(handle);
                        pending_chunk = None;
                        crate::log!(
                            "logtotcp: client connected handle={} ms={}\n",
                            handle.0,
                            Instant::now().as_millis()
                        );
                    }
                    NetEvent::Closed { handle } => {
                        if conn_handle == Some(handle) {
                            conn_handle = None;
                            pending_chunk = None;
                            crate::log!(
                                "logtotcp: client disconnected handle={} ms={}\n",
                                handle.0,
                                Instant::now().as_millis()
                            );
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

            if let Some(handle) = conn_handle {
                if pending_chunk.is_none() {
                    let chunk = drain_bytes(DRAIN_CHUNK);
                    if !chunk.is_empty() {
                        pending_chunk = Some(chunk);
                    }
                }

                if let Some(chunk) = pending_chunk.as_ref()
                    && cmds
                        .push(NetCommand::SendTcp {
                            handle,
                            data: chunk.clone(),
                        })
                        .is_ok()
                {
                    pending_chunk = None;
                }
            }

            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
}
