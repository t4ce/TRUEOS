mod intel_igpu770;
mod intel_guc;
mod intel_igpu770_rcs;
mod intel_770_registers;
#[path = "XeLp_copy_ngin.rs"]
pub(crate) mod xelp_copy_ngin;
#[path = "XeLp_display_ngin.rs"]
pub(crate) mod xelp_display_ngin;
#[path = "XeLp_render_ngin.rs"]
mod xelp_render_ngin;
mod intel;

pub use intel::*;
