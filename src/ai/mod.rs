//! Speech services and compatibility boundaries for retired local inference.

pub mod ai_activity;
pub mod lumen_service;
pub(crate) mod spirit_abi;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_capture;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_kokoro;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_service;
