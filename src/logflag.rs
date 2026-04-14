use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use spin::Once;

pub(crate) const USB_AUDIO_DEBUG_LOGS: bool = false;
pub(crate) const HID_DEBUG_REPORT_LOGS: bool = true;

pub(crate) const NET_LOG_RX_TAP: bool = false;
pub(crate) const NET_LOG_TX_TAP: bool = false;
pub(crate) const NET_LOG_TCP_FLOW: bool = false;
pub(crate) const NET_LOG_TCP_SEND_FLUSH: bool = false;
pub(crate) const NET_LOG_ARP_RX: bool = false;
pub(crate) const NET_LOG_DHCP_VERBOSE: bool = false;
pub(crate) const NET_LOG_IPV6_RA: bool = false;
pub(crate) const NET_LOG_DHCP6_SAMPLES: usize = 8;
pub(crate) const VNET_EXERCISE_LOGS: bool = false;
pub(crate) const ESP_GATE_DEFAULT_UPLOAD_LOGS: bool = false;

pub(crate) const R8125_VERBOSE_LOGS: bool = false;
pub(crate) const BOOT_INFO_LOGS: bool = true;

pub(crate) const UI2_ENABLE_VERBOSE_COMPOSE_LOGS: bool = false;
pub(crate) const BROWSER_VM_DEBUG_LOGS: bool = false;
pub(crate) const BROWSER_HTML_PREVIEW_LOGS: bool = false;
pub(crate) const VHTTPS_VERBOSE: bool = false;
pub(crate) const GFX_FRAME_PROGRESS_LOGS: bool = false;
pub(crate) const INTEL_GFX_DEBUG_LOGFLAG: bool = true;

pub(crate) const VIRGL_DRAW_DIAGNOSTICS_LOGS: bool = false;
pub(crate) static VIRGL_TEX_DEBUG_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_BLEND_BIND_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_BLEND_UNSUPPORTED_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_STATE_TRANSITION_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static VIRGL_PRESENT_DIAG_LOGS: AtomicU32 = AtomicU32::new(0);

pub(crate) static INTEL_RING_INIT_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static INTEL_SUBMIT_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static INTEL_PRESENT_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) const INTEL_CURSOR_PROBE_LOGS: bool = false;

pub(crate) const INTEL_RENDER_NGIN_LOGS: bool = true;
pub(crate) const INTEL_RENDER_NGIN_BATCH_LOGS: bool = true;
pub(crate) const INTEL_DISPLAY_NGIN_LOGS: bool = true;
pub(crate) const INTEL_MEDIA_NGIN_LOGS: bool = true;
pub(crate) const INTEL_COPY_NGIN_LOGS: bool = true;

pub(crate) const GFX_CABI_FRAME_DEBUG_LOGS: bool = false;
pub(crate) static GFX_CABI_SUBMIT_BUDGET_LOGS: AtomicU32 = AtomicU32::new(0);
pub(crate) static GFX_CABI_VIRGL_END_FRAME_DIAG_LOGS: AtomicU32 = AtomicU32::new(1);
pub(crate) static GFX_CABI_VIRGL_FIRST_FRAME_SEEN: AtomicBool = AtomicBool::new(true);

pub(crate) static USB_LOG_ALL: AtomicBool = AtomicBool::new(false);

pub(crate) const NVME_VERBOSE: bool = false;

pub(crate) static BGRT_LOG_ONCE: Once<()> = Once::new();
pub(crate) static TGA_MISSING_LOG_ONCE: Once<()> = Once::new();
pub(crate) static TGA_TASK_STARTED_LOG_ONCE: Once<()> = Once::new();
pub(crate) static AP_ACTIVITY_LOGGED: AtomicU64 = AtomicU64::new(0);
