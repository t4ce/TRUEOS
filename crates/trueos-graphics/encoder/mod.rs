//! Image encoder front door for TRUEOS graphics.
//!
//! Encoders live here as pure byte writers. File serving, data URLs, capture
//! policy, and persistence belong to their callers.

pub(crate) mod png;
