//! TRUEOS graphics boundary.
//!
//! This module is the home for pure-ish image/vector/font graphics code that
//! should not conceptually belong to UI3. Some implementations still live in
//! their historical files during the migration; UI3 re-exports these modules
//! for compatibility, while new call sites should prefer `crate::graphics`.
pub(crate) mod decoder;
pub(crate) mod encoder;
pub(crate) mod font;
pub(crate) mod path_mesh;
pub(crate) use self::decoder::jpeg as jpeg_codec;
pub(crate) use self::decoder::jpeg_layout;
pub(crate) use self::decoder::png as png_codec;
pub(crate) use self::decoder::png_decode_pool;
pub(crate) mod image;
pub(crate) mod primitives;
