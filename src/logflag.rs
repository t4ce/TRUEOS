use core::sync::atomic::AtomicBool;
use log::{Level, LevelFilter};
pub(crate) use log_os::{LogArea, LogLevelPolicy};
use spin::Once;

pub(crate) const GLOBAL_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const BOOT_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const SERVICE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const NET_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
pub(crate) const USB_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const STORAGE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const GFX_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const GPGPU_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
pub(crate) const RENDER_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Info);
pub(crate) const HDA_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const HV_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
pub(crate) const APPS_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Trace);
pub(crate) const EXECUTOR_REALM_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
pub(crate) const EXECUTOR_CACHE_LOG_LEVEL: LogLevelPolicy = LogLevelPolicy::up(LevelFilter::Warn);
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

pub(crate) fn area_log_enabled(area: LogArea, level: Level) -> bool {
    log_os::level_enabled(area_log_policy(area), level)
}
