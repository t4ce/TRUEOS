use core::sync::atomic::{AtomicBool, AtomicU32};
use log::{Level, LevelFilter};
use spin::Once;

pub(crate) const GLOBAL_LOG_LEVEL: LevelFilter = LevelFilter::Info;

#[allow(non_upper_case_globals)]
pub(crate) const dont_persist_globalog: bool = true;

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

pub(crate) const UI2_ENABLE_VERBOSE_COMPOSE_LOGS: bool = false;
pub(crate) const VHTTPS_VERBOSE: bool = false;
pub(crate) const HTML_SHACK_VERBOSE: bool = false;
pub(crate) const HTML_SHACK_IDLE_LOGS: bool = false;
pub(crate) const GFX_FRAME_PROGRESS_LOGS: bool = false;
pub(crate) const INTEL_STAGE1_LOGS: bool = true;

pub(crate) const VIRGL_DRAW_DIAGNOSTICS_LOGS: bool = false;
pub(crate) static VIRGL_TEX_DEBUG_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_BLEND_BIND_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_BLEND_UNSUPPORTED_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_STATE_TRANSITION_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_PRESENT_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);

pub(crate) const INTEL_CURSOR_PROBE_LOGS: bool = false;

pub(crate) const INTEL_DISPLAY_NGIN_LOGS: bool = true;
pub(crate) const INTEL_MEDIA_NGIN_LOGS: bool = true;
pub(crate) const INTEL_MEDIA_FS_CACHE_ENABLED: bool = true;
pub(crate) const INTEL_MEDIA_PRESENT_LUMA_ONLY: bool = true;

pub(crate) const GFX_CABI_FRAME_DEBUG_LOGS: bool = false;
pub(crate) static GFX_CABI_SUBMIT_BUDGET_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static GFX_CABI_VIRGL_END_FRAME_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static GFX_CABI_VIRGL_FIRST_FRAME_SEEN: AtomicBool = AtomicBool::new(true);

pub(crate) static USB_LOG_ALL: AtomicBool = AtomicBool::new(false);
pub(crate) const USB_VENDOR_LOG_LEVEL: LevelFilter = LevelFilter::Warn;
pub(crate) const BLUEPRINT_LOG_LEVEL: LevelFilter = LevelFilter::Warn;
pub(crate) const USB_AUDIO_DEBUG_LOGS: bool = false;
pub(crate) const HID_DEBUG_REPORT_LOGS: bool = false;
pub(crate) const USB_MASS_UAS_ADVANCED_PROBE_LOGS: bool = false;
pub(crate) const USB_MASS_UAS_TRACE_LOGS: bool = false;

pub(crate) const NVME_VERBOSE: bool = false;

pub(crate) static BGRT_LOG_ONCE: Once<()> = Once::new();
pub(crate) static TGA_MISSING_LOG_ONCE: Once<()> = Once::new();
pub(crate) static TGA_TASK_STARTED_LOG_ONCE: Once<()> = Once::new();

pub(crate) fn usb_vendor_log_enabled(level: Level) -> bool {
    level_enabled(USB_VENDOR_LOG_LEVEL, level)
}

pub(crate) fn blueprint_log_enabled(level: Level) -> bool {
    level_enabled(BLUEPRINT_LOG_LEVEL, level)
}

fn level_enabled(filter: LevelFilter, level: Level) -> bool {
    match filter {
        LevelFilter::Off => false,
        LevelFilter::Error => matches!(level, Level::Error),
        LevelFilter::Warn => matches!(level, Level::Warn | Level::Error),
        LevelFilter::Info => matches!(level, Level::Info | Level::Warn | Level::Error),
        LevelFilter::Debug => {
            matches!(level, Level::Debug | Level::Info | Level::Warn | Level::Error)
        }
        LevelFilter::Trace => true,
    }
}
