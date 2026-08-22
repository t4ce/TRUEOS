#![no_std]
#![deny(unsafe_code)]

//! Bounded English G2P and Kokoro v1.0 text preparation for TRUEOS.
//!
//! The `G2P2` parser and pair-joint n-gram decoder are adapted from
//! [`g2p2-core` 0.2.0](https://github.com/jqueguiner/g2p2), copyright © 2026
//! jqueguiner, under MIT OR Apache-2.0. This implementation changes the parser
//! to return errors for untrusted bytes, borrows all model strings, replaces
//! boxed hash-map keys with fixed contiguous records, and adds a Kokoro-specific
//! English text and tokenization layer.

extern crate alloc;

mod decode;
mod ipa;
mod model;
mod text;

pub use decode::{DecodeError, PronunciationLookup};
pub use ipa::{EncodedPhonemes, IpaError, canonicalize_ipa, kokoro_token_id};
pub use model::{
    MAX_NGRAM_ORDER, MemoryUsage, Model, ModelError, PINNED_ENGLISH_BYTES, PINNED_ENGLISH_PATH,
    PINNED_ENGLISH_SHA256,
};
pub use text::{
    CHUNK_FALLBACK_MAX, CHUNK_TARGET_MAX, CHUNK_TARGET_MIN, EnglishToken, EnglishTokenKind,
    EnglishTokens, FrontendError, FrontendOutput, KOKORO_BOUNDARY_TOKEN, KOKORO_MODEL_MAX,
    chunk_ranges, prepare_english, prepare_english_with,
};

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests;
