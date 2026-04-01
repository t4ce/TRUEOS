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
