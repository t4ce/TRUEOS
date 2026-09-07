//! Kernel-resident AI model services and their supporting assets.

pub mod ai_activity;
#[cfg(feature = "trueos_lumen")]
pub mod lfm25_boot_warm;
pub mod lfm25_decode;
pub mod lfm25_f32;
pub mod lfm25_hybrid_cpu_backend;
pub mod lfm25_model;
pub mod lfm25_tokenizer;
pub mod lumen_service;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_capture;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_kokoro;
#[cfg(feature = "trueos_ttstt")]
pub mod ttstt_service;
