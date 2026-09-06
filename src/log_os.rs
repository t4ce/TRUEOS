//! TRUEOS logging policy and sink routing.
//!
//! A log record has two independent dimensions: an area and a level. Levels
//! are ordered from least verbose/most severe to most verbose as
//! `Error -> Important -> Warn -> Once -> Info -> Debug -> Trace`. Policies
//! use that order as follows:
//!
//! - `Up(level)` accepts `level` and every level to its left (more severe).
//!   For example, `Up(Warn)` accepts Error, Important, and Warn.
//! - `Down(level)` accepts `level` and every level to its right (more verbose).
//! - `Only(levels)` accepts exactly the selected set.
//!
//! Area policy is an acceptance gate, not a display preference. A record that
//! [`flags::area_log_enabled`] rejects is never delivered to the TCP ring or
//! emulator UART, so it cannot appear in any host-side logfile. Absence is
//! therefore expected when the record's area/level is filtered and is not
//! evidence that the instrumented code path did not run. Also account for
//! records emitted before dispatcher installation and explicit once/rate-limit
//! suppression before diagnosing a missing accepted record.
//!
//! Bare-metal TCP logs are drained from port 1 into rotating
//! `bld/baremetal-logs/trueos-baremetal.{0,1,2}.log` files; the current file is
//! linked as `bld/baremetal-logs/LatestOfThree.logs`. Emulator UART output is
//! captured from the QEMU serial endpoint into rotating
//! `bld/emulator-logs/trueos-emulator.{0,1,2}.log` files, with
//! `bld/emulator-logs/latest.log` pointing at the current one. These are host
//! capture files; this module itself keeps no disk logfile.

use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

extern crate alloc;

pub(crate) mod flags {
    use core::sync::atomic::AtomicBool;

    pub(crate) use log_os_core::{LogArea, LogLevelPolicy};
    use log_os_core::{LogLevel, LogLevelFilter};
    use spin::Once;

    /// Full forensic USB profile. The exact switch settings are preserved in
    /// `log_os.usb_full_diag_profile.txt` beside this source file.
    pub(crate) const USB_UAS_DIAG_PROFILE_ENABLED: bool = false;

    /// Operational USB profile enabled with the full forensic profile for the
    /// current CrabUSB controller/HID handoff capture. Keep this enabled while
    /// the passed-through ASMedia xHCI Heal experiment needs its Info-level
    /// admission, capability, and quarantine records in host captures.
    pub(crate) const USB_RUNTIME_DIAG_PROFILE_ENABLED: bool = true;

    /// Boot/network diagnostic profile for startup timing and first-net-process
    /// operability logs.
    pub(crate) const BOOT_DIAG_PROFILE_ENABLED: bool = false;

    /// Request-level HTTP diagnostics without per-packet trace traffic.
    pub(crate) const HTTP_FETCH_DIAG_PROFILE_ENABLED: bool = true;

    /// Focused Wi-Fi PCI bring-up profile. Keep this enabled while validating
    /// the passed-through Intel CNVi function: its claim, BAR discovery, and
    /// deferred driver-probe records are emitted at Net/Info.
    pub(crate) const WIFI_PCI_DIAG_PROFILE_ENABLED: bool = true;

    /// Focused Lumen inference performance profile.
    ///
    /// Global/Info carries the cold model pack/seal, sampled RCS phases, sparse
    /// read-only GT frequency observations, and Lumen-owned MOCS checkpoints.
    /// This switch also controls the phase sampler itself and admits
    /// reasoning/service-lane lifecycle plus LFM2.5 C++/IGC runtime/submission
    /// timing without enabling noisy render, storage, network, or per-dispatch
    /// trace traffic that would perturb the measurements. Functional upper-half
    /// MOCS ownership is independent of this logging switch. Setting it to
    /// false compile-folds Lumen submissions back to the unsampled path.
    pub(crate) const LUMEN_PERF_DIAG_PROFILE_ENABLED: bool = false;

    /// UI4 plane/lease/composition diagnostic profile.
    ///
    /// UI4 logs through the Global area, so its per-submission markers -
    /// compositor backend selection (`sprite-quad-runs` versus the layer
    /// kernel), deferred lease raises, frame retirement and per-drag frame
    /// motion - only appear once Global admits Trace. Enable this while
    /// working on the plane lease contract or the slot0 stack painter; it is
    /// noisy by design and belongs off in ordinary boots.
    pub(crate) const UI4_DIAG_PROFILE_ENABLED: bool = false;

    /// Focused Shell2 single-keystroke/render latency profile.
    ///
    /// The probe sites sample their first 16 observations and then one in 128.
    /// They deliberately emit through the already-admitted Render/Info lane
    /// instead of widening Render to Trace: enabling every Intel render trace
    /// would perturb the exact FontKernel/UI4 path this profile measures.
    pub(crate) const SHELL2_RENDER_DIAG_PROFILE_ENABLED: bool = true;
    pub(crate) const SHELL2_RENDER_DIAG_FIRST: u64 = 16;
    pub(crate) const SHELL2_RENDER_DIAG_EVERY: u64 = 128;

    /// Focused QuadTexture sampled indexed-draw preparation/submission probe.
    ///
    /// Gate sites on both this flag and the clip-position3/UV texture package.
    /// Emit preparation, admission, and retirement through Render/Info, which
    /// is already accepted. The `vgpu` target resolves to Global and its Info
    /// records are filtered in ordinary boots. Keep failures at Render/Warn
    /// with their original backend reason before mapping to DeviceLost (-32).
    /// Sample repeated successful stages; never suppress failure records.
    /// This flag does not widen any area policy or enable per-frame Trace.
    pub(crate) const QUAD_TEXTURE_DIAG_PROFILE_ENABLED: bool = true;
    pub(crate) const QUAD_TEXTURE_DIAG_FIRST: u64 = 8;
    pub(crate) const QUAD_TEXTURE_DIAG_EVERY: u64 = 128;

    /// Focused Helio/Intel graphics bring-up profile.
    ///
    /// This admits the iGPU device, display, render and direct-RCS readiness
    /// ladders needed to distinguish artifact upload, PPGTT residency, GuC
    /// admission and kernel-probe failures. Keep it explicit: GFX Debug can
    /// include per-frame/display state that is too noisy for normal boots.
    pub(crate) const HELIO_GFX_DIAG_PROFILE_ENABLED: bool = false;

    pub(crate) const GLOBAL_LOG_LEVEL: LogLevelPolicy =
        if USB_UAS_DIAG_PROFILE_ENABLED || UI4_DIAG_PROFILE_ENABLED {
            // Preserve the original USB hunt's full Global side, including Debug.
            LogLevelPolicy::up(LogLevelFilter::Trace)
        } else {
            LogLevelPolicy::up(LogLevelFilter::Warn)
        };
    pub(crate) const BOOT_LOG_LEVEL: LogLevelPolicy = if BOOT_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Trace)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Warn)
    };
    pub(crate) const SERVICE_LOG_LEVEL: LogLevelPolicy = if LUMEN_PERF_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Info)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Warn)
    };
    pub(crate) const NET_LOG_LEVEL: LogLevelPolicy = if BOOT_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Trace)
    } else if WIFI_PCI_DIAG_PROFILE_ENABLED || HTTP_FETCH_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Info)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Warn)
    };
    pub(crate) const USB_LOG_LEVEL: LogLevelPolicy = if USB_UAS_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Trace)
    } else if USB_RUNTIME_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Info)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Warn)
    };
    pub(crate) const STORAGE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LogLevelFilter::Info);
    /// The display/cursor side is per-flip chatty, so it normally stays at
    /// Warn; the explicit Helio profile temporarily admits its Debug ladder.
    pub(crate) const GFX_LOG_LEVEL: LogLevelPolicy = if HELIO_GFX_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Debug)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Warn)
    };
    pub(crate) const GPGPU_LOG_LEVEL: LogLevelPolicy = if HELIO_GFX_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Debug)
    } else if LUMEN_PERF_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Info)
    } else {
        // Keep ordinary GPGPU Info chatter filtered while admitting explicit
        // one-shot lifecycle/proof records such as the boot silicon probe.
        LogLevelPolicy::up(LogLevelFilter::Once)
    };
    pub(crate) const RENDER_LOG_LEVEL: LogLevelPolicy = if HELIO_GFX_DIAG_PROFILE_ENABLED {
        LogLevelPolicy::up(LogLevelFilter::Debug)
    } else {
        LogLevelPolicy::up(LogLevelFilter::Info)
    };
    pub(crate) const HDA_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LogLevelFilter::Warn);
    pub(crate) const HV_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LogLevelFilter::Trace);
    pub(crate) const APPS_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LogLevelFilter::Trace);
    pub(crate) const EXECUTOR_REALM_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LogLevelFilter::Info);
    pub(crate) const EXECUTOR_CACHE_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LogLevelFilter::Warn);
    pub(crate) const INTEL_MEDIA_NGIN_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LogLevelFilter::Info);
    pub(crate) const BLUEPRINT_LOG_LEVEL: LogLevelPolicy =
        LogLevelPolicy::up(LogLevelFilter::Trace);

    pub(crate) const NET_LOG_RX_TAP: bool = true;
    pub(crate) const NET_LOG_TX_TAP: bool = true;
    pub(crate) const NET_LOG_TCP_FLOW: bool = false;
    pub(crate) const NET_LOG_TCP_CONNECT_STATES: bool = false;
    pub(crate) const NET_LOG_TCP_CONNECT_WIRE: bool = true;
    pub(crate) const NET_LOG_TCP_SEND_FLUSH: bool = false;
    pub(crate) const NET_LOG_ARP_RX: bool = true;
    pub(crate) const NET_LOG_DHCP_VERBOSE: bool = true;
    pub(crate) const NET_LOG_IPV6_RA: bool = false;
    pub(crate) const NET_LOG_DHCP6_SAMPLES: usize = 8;
    pub(crate) const VNET_EXERCISE_LOGS: bool = false;
    pub(crate) const R8125_VERBOSE_LOGS: bool = true;
    pub(crate) const BOOT_INFO_LOGS: bool = BOOT_DIAG_PROFILE_ENABLED;
    pub(crate) const HV_LOGS: bool = true;
    pub(crate) const PORTAL_LOGS: bool = true;
    pub(crate) const HTML_SHACK_VERBOSE: bool = false;
    pub(crate) const HTML_SHACK_IDLE_LOGS: bool = false;
    // Stage1 suppresses the rate-limited present diagnostics when enabled.
    pub(crate) const INTEL_STAGE1_LOGS: bool = false;
    pub(crate) const INTEL_RENDER_NGIN_LOGS: bool = true;
    pub(crate) const INTEL_RENDER_NGIN_BATCH_LOGS: bool = true;
    pub(crate) const INTEL_DISPLAY_NGIN_LOGS: bool = true;
    pub(crate) const HID_DEBUG_REPORT_LOGS: bool = false;
    pub(crate) const USB_MASS_UAS_TRACE_LOGS: bool = USB_UAS_DIAG_PROFILE_ENABLED;
    pub(crate) const STORAGE_TRACE_LOGS: bool = false;
    pub(crate) const NVME_VERBOSE: bool = false;
    pub(crate) static BGRT_LOG_ONCE: Once<()> = Once::new();
    pub(crate) static USB_LOG_ALL: AtomicBool = AtomicBool::new(USB_UAS_DIAG_PROFILE_ENABLED);

    /// Returns the compile-time acceptance policy for one semantic log area.
    ///
    /// Keep all area/profile choices here: both sinks consult this same policy.
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
        }
    }

    /// True only when `level` survives `area`'s policy and may reach a sink.
    ///
    /// False means the record will not exist in either host capture stream.
    pub(crate) fn area_log_enabled(area: LogArea, level: LogLevel) -> bool {
        log_os_core::level_enabled(area_log_policy(area), level)
    }
}

static LOG_WRITE_LOCK: spin::Mutex<()> = spin::Mutex::new(());
static UART_LOG_WRITE_LOCK: spin::Mutex<()> = spin::Mutex::new(());
static EMULATOR_UART_LOGGING: AtomicBool = AtomicBool::new(false);
const LOG_ONCE_SITE_CAPACITY: usize = 512;
static LOG_ONCE_STATE: log_os_core::LogOnceState<LOG_ONCE_SITE_CAPACITY> =
    log_os_core::LogOnceState::new();

struct TcpLogSink;

impl log_os_core::GlobalLogSink for TcpLogSink {
    fn spec(&self) -> log_os_core::GlobalLogSinkSpec {
        log_os_core::GlobalLogSinkSpec::new(
            log_os_core::LogAreaSet::ALL,
            log_os_core::LogLevelPolicy::up(log_os_core::LogLevelFilter::Trace),
        )
    }

    fn level_policy(&self, area: flags::LogArea) -> log_os_core::LogLevelPolicy {
        flags::area_log_policy(area)
    }

    fn write_accepted(
        &self,
        area: flags::LogArea,
        _level: log_os_core::LogLevel,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        write_with_tags(area, purpose, args);
    }
}

struct EmulatorUartLogSink;

impl log_os_core::GlobalLogSink for EmulatorUartLogSink {
    fn spec(&self) -> log_os_core::GlobalLogSinkSpec {
        log_os_core::GlobalLogSinkSpec::new(
            log_os_core::LogAreaSet::ALL,
            log_os_core::LogLevelPolicy::up(log_os_core::LogLevelFilter::Trace),
        )
    }

    fn level_policy(&self, area: flags::LogArea) -> log_os_core::LogLevelPolicy {
        flags::area_log_policy(area)
    }

    fn accepts(&self, area: flags::LogArea, level: log_os_core::LogLevel) -> bool {
        EMULATOR_UART_LOGGING.load(Ordering::Acquire) && flags::area_log_enabled(area, level)
    }

    fn write_accepted(
        &self,
        area: flags::LogArea,
        _level: log_os_core::LogLevel,
        purpose: Option<&str>,
        args: fmt::Arguments<'_>,
    ) {
        write_uart_with_tags(area, purpose, args);
    }
}

static TCP_LOG_SINK: TcpLogSink = TcpLogSink;
static EMULATOR_UART_LOG_SINK: EmulatorUartLogSink = EmulatorUartLogSink;
static TRUEOS_LOG_SINKS: [&'static dyn log_os_core::GlobalLogSink; 2] =
    [&TCP_LOG_SINK, &EMULATOR_UART_LOG_SINK];
static TRUEOS_LOG_ROUTER: log_os_core::GlobalLogRouter =
    log_os_core::GlobalLogRouter::new(&TRUEOS_LOG_SINKS);
#[macro_export]
macro_rules! log {
    (purpose = $purpose:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_purpose(
            module_path!(),
            $crate::log_os::LogLevel::Info,
            Some($purpose),
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            module_path!(),
            $crate::log_os::LogLevel::Info,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_trace {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Trace,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Trace,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_debug {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Debug,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Debug,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_info {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Info,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Info,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_warn {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Warn,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Warn,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_important {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Important,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Important,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! log_once {
    (target: $target:expr; $($tt:tt)*) => {{
        const SITE: $crate::log_os::LogSiteId = $crate::log_os::LogSiteId::from_location(
            module_path!(),
            file!(),
            line!(),
            column!(),
        );
        let _ = $crate::log_os::log_once_with_target(
            SITE,
            $target,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        const SITE: $crate::log_os::LogSiteId = $crate::log_os::LogSiteId::from_location(
            module_path!(),
            file!(),
            line!(),
            column!(),
        );
        let _ = $crate::log_os::log_once_with_target(
            SITE,
            "boot",
            format_args!($($tt)*),
        );
    }};
}

/// Sample a recurring event at one call site without turning it into a
/// one-shot. The emitted record includes the cumulative occurrence and how
/// many matching records were suppressed immediately before it.
#[macro_export]
macro_rules! log_rate_limited {
    (target: $target:expr; level: $level:expr; first: $first:expr; every: $every:expr; $($tt:tt)*) => {{
        static STATE: $crate::log_os::LogRateLimitState = $crate::log_os::LogRateLimitState::new();
        let observation = STATE.observe($first, $every);
        if observation.should_emit() {
            $crate::log_os::log_with_target_level(
                $target,
                $level,
                format_args!(
                    "rate_limit occurrence={} suppressed_since_last={} {}",
                    observation.occurrence(),
                    observation.suppressed_since_last(),
                    format_args!($($tt)*),
                ),
            );
        }
    }};
}

/// Rate-bounded record for the Shell2 -> UI4 -> FontKernel latency hunt.
///
/// This is intentionally a semantic Render/Info lane even though the record
/// calls itself a trace. See `SHELL2_RENDER_DIAG_PROFILE_ENABLED`: admitting
/// the whole Render/Trace area also admits per-submit Intel render chatter.
#[macro_export]
macro_rules! log_shell2_render_trace {
    ($($tt:tt)*) => {{
        if $crate::log_os::flags::SHELL2_RENDER_DIAG_PROFILE_ENABLED {
            static STATE: $crate::log_os::LogRateLimitState =
                $crate::log_os::LogRateLimitState::new();
            let observation = STATE.observe(
                $crate::log_os::flags::SHELL2_RENDER_DIAG_FIRST,
                $crate::log_os::flags::SHELL2_RENDER_DIAG_EVERY,
            );
            if observation.should_emit() {
                $crate::log_os::log_with_area_purpose(
                    $crate::log_os::flags::LogArea::Render,
                    $crate::log_os::LogLevel::Info,
                    Some("shell2-render-trace"),
                    format_args!(
                        "sample={} suppressed_since_last={} {}",
                        observation.occurrence(),
                        observation.suppressed_since_last(),
                        format_args!($($tt)*),
                    ),
                );
            }
        }
    }};
}

#[macro_export]
macro_rules! log_error {
    (target: $target:expr; $($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            $target,
            $crate::log_os::LogLevel::Error,
            format_args!($($tt)*),
        );
    }};
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_target_level(
            "boot",
            $crate::log_os::LogLevel::Error,
            format_args!($($tt)*),
        );
    }};
}

#[macro_export]
macro_rules! audio_probe {
    ($($tt:tt)*) => {{
        $crate::log_os::log_with_area_purpose(
            $crate::log_os::flags::LogArea::Hda,
            $crate::log_os::LogLevel::Trace,
            Some("audio"),
            format_args!($($tt)*),
        );
    }};
}

pub fn log(args: fmt::Arguments<'_>) {
    log_os_core::log(&TRUEOS_LOG_ROUTER, args);
}

pub use log_os_core::{LogLevel, LogRateLimitState, LogSiteId};

pub(crate) fn purpose_for_level(level: LogLevel) -> &'static str {
    log_os_core::purpose_for_level(level)
}

pub(crate) fn hypervisor_line(level: LogLevel, args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Hv, level, args);
}

pub(crate) fn blueprint_line(level: LogLevel, args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Blueprint, level, args);
}

/// Emit a high-salience Blueprint lifecycle marker that is not a failure.
pub(crate) fn blueprint_important_line(args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Blueprint, LogLevel::Important, args);
}

/// Emit a high-salience service lifecycle marker that is not a failure.
pub(crate) fn service_important_line(args: fmt::Arguments<'_>) {
    log_with_area_level(flags::LogArea::Service, LogLevel::Important, args);
}

pub(crate) fn printer_discovered(name: &str, uri: &str) {
    log_with_area_level(
        flags::LogArea::Net,
        LogLevel::Info,
        format_args!("printer: discovered name={} uri={}\n", name, uri),
    );
}

pub(crate) fn printer_spooler_online() {
    log_with_area_level(
        flags::LogArea::Net,
        LogLevel::Info,
        format_args!(
            "print2d: kernel spooler online policy=single-default transport=ipp format=pwg-raster\n"
        ),
    );
}

pub(crate) fn gridpaper_print_requested(owner: u8, token: u32, generation: u64) {
    log_with_area_level(
        flags::LogArea::Net,
        LogLevel::Info,
        format_args!(
            "print2d: gridpaper request owner={} token={} generation={} trigger=PrintScreen\n",
            owner, token, generation
        ),
    );
}

pub(crate) fn print2d_job_state(job_id: u32, state: &str, detail: &str) {
    // Job transitions are sparse lifecycle evidence, not packet-level network
    // chatter. Keep them visible even when the Net area is raised to Warn so
    // a render failure cannot look like a silent printer or missing worker.
    log_with_target_level(
        "print2d",
        LogLevel::Info,
        format_args!("print2d: job={} state={} detail={}\n", job_id, state, detail),
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

fn write_uart_with_tags(area: flags::LogArea, purpose: Option<&str>, args: fmt::Arguments<'_>) {
    let _guard = UART_LOG_WRITE_LOCK.lock();
    crate::uart1_com1::write_fmt(format_args!("[{}] ", log_os_core::area_tag(area)));
    if let Some(purpose) = purpose {
        crate::uart1_com1::write_fmt(format_args!("[{}] ", purpose));
    }
    crate::uart1_com1::write_fmt(args);
}

pub(crate) fn set_emulator_uart_logging(enabled: bool) {
    if enabled {
        crate::uart1_com1::init();
    }
    EMULATOR_UART_LOGGING.store(enabled, Ordering::Release);
}

pub fn log_with_area_level(area: flags::LogArea, level: LogLevel, args: fmt::Arguments<'_>) {
    log_os_core::log_with_area_level(&TRUEOS_LOG_ROUTER, area, level, args);
}

pub fn log_with_area_purpose(
    area: flags::LogArea,
    level: LogLevel,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    log_os_core::log_with_area_purpose(&TRUEOS_LOG_ROUTER, area, level, purpose, args);
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub fn log_with_target_purpose(
    target: &str,
    level: LogLevel,
    purpose: Option<&str>,
    args: fmt::Arguments<'_>,
) {
    log_os_core::log_with_target_purpose(&TRUEOS_LOG_ROUTER, target, level, purpose, args);
}

pub fn log_with_target_level(target: &str, level: LogLevel, args: fmt::Arguments<'_>) {
    log_os_core::log_with_target_level(&TRUEOS_LOG_ROUTER, target, level, args);
}

pub fn log_once_with_target(
    site: log_os_core::LogSiteId,
    target: &str,
    args: fmt::Arguments<'_>,
) -> log_os_core::LogOnceObservation {
    log_os_core::log_once_with_area_purpose(
        &TRUEOS_LOG_ROUTER,
        &LOG_ONCE_STATE,
        site,
        log_os_core::target_log_area(target),
        Some(log_os_core::purpose_for_level(LogLevel::Once)),
        args,
    )
}

pub fn init_global_dispatch() {
    log_os_core::install_global_log_dispatch(&TRUEOS_LOG_ROUTER);
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

    #[trueos_executor::task]
    pub async fn logtotcp_task() {
        use trueos_time::{Duration as EmbassyDuration, Instant, Timer};

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
