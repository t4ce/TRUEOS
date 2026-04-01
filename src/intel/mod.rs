mod intel;
mod intel_770_registers;
mod intel_guc;
mod intel_igpu770;
mod intel_igpu770_rcs;
#[path = "XeLp_copy_ngin.rs"]
pub(crate) mod xelp_copy_ngin;
#[path = "XeLp_display_ngin.rs"]
pub(crate) mod xelp_display_ngin;
#[path = "XeLp_media_mp4.rs"]
pub(crate) mod xelp_media_mp4;
#[path = "XeLp_media_ngin.rs"]
pub(crate) mod xelp_media_ngin;
#[path = "XeLp_render_ngin.rs"]
mod xelp_render_ngin;

pub use intel::*;
pub(crate) use intel_guc::status as guc_status;
pub(crate) use intel_igpu770::{
    Igpu770WarmState, dma_cache_flush_range, ggtt_bcs_smoke_test_once, ggtt_blt_smoke_test_once,
    warm_state,
};
pub(crate) use xelp_media_ngin::{
    MediaKickoffState, MediaSurfaceWindow, demo_surface_window as media_demo_surface_window,
    kickoff_once as media_kickoff_once, kickoff_state as media_kickoff_state,
};
