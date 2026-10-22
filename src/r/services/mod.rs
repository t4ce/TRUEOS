//! Long-lived kernel service authorities and worker control planes.
pub(crate) mod codex_progress_service;

pub mod gamepad_control_service;
pub mod gna_audio_frontend_service;
#[cfg(feature = "gridpaper")]
pub mod gridpaper_service;
pub mod hda_capture_lane;
pub mod hid_udp_service;
pub(crate) mod hull_font_service;
pub(crate) mod image_open_service;
pub mod keyboard_control_service;
pub mod media_service;
mod microfont_line_overlap;
pub(crate) mod microfont_line_preview;
pub(crate) mod microfont_log_service;
pub(crate) mod microfont_mask_service;
pub(crate) mod microfont_raster_preview;
pub mod mouse_motion_service;
#[cfg(feature = "gridpaper")]
pub(crate) mod pxeproc_service;
pub(crate) mod shell3_font_service;
pub mod spawn_service;
pub mod vcpy_service;
pub(crate) mod video_open_service;
pub mod video_service;
