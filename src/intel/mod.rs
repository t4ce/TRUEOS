mod intel;
mod intel_770_registers;
mod intel_guc;
mod intel_igpu770;
mod intel_igpu770_rcs;
pub(crate) mod render_demo;
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
pub(crate) use intel_guc::ready as guc_ready;
pub(crate) use intel_guc::status as guc_status;
pub(crate) use intel_igpu770::{
    Igpu770WarmState, dma_cache_flush_range, ggtt_bcs_smoke_test_once, ggtt_blt_smoke_frame,
    ggtt_blt_smoke_test_once, ggtt_map_screen_rgba_surface, rcs_clear_rgba_surface,
    rcs_clear_screen_rgba, rcs_draw_rgba_rgb_triangles, rcs_draw_screen_rgb_triangles,
    rcs_draw_screen_tex_triangles, rcs_present_rgba_frame, warm_state,
};
pub use render_demo::{
    intel_render_demo_task, isolated_triangle_mode_active, render_demo_mode_active,
};
pub(crate) use xelp_display_ngin::{
    bootstrap_primary_present_surface, owned_triangle_disable_non_primary_planes_pipe_a,
    plane_rebind_present_surface, primary_present_shadow_surface_gpu_addr, primary_present_surface,
    primary_present_surface_gpu_addr, primary_present_visible_surface_gpu_addr,
};
pub(crate) use xelp_media_ngin::{
    MediaKickoffState, MediaSurfaceWindow, demo_surface_window as media_demo_surface_window,
    kickoff_once as media_kickoff_once, kickoff_state as media_kickoff_state,
};
pub(crate) use xelp_render_ngin::TextureStoreSampleKind;
