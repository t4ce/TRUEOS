//! Long-lived kernel service authorities and worker control planes.

pub mod clipboard_service;
pub mod font_kernel_service;
pub mod font_plan_service;
pub mod font_producer_service;
pub mod gamepad_control_service;
pub mod gna_audio_frontend_service;
pub mod gridpaper_service;
pub mod hda_capture_lane;
pub mod hid_udp_service;
pub mod keyboard_control_service;
pub mod media_service;
pub mod mouse_motion_service;
pub(crate) mod oceancache;
pub mod spawn_service;
