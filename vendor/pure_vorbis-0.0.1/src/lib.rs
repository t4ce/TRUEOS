//! Safe [Vorbis](http://www.vorbis.com/) decoder implementation in pure Rust.
//!
//! The decoder is low-level and can only decode Vorbis packets directly (not wrapped in any
//! containers like Ogg).
//!
//! # Example
//!
//! ```rust,no_run
//! use std::io::Cursor;
//! use vorbis::{BitReader, Decoder};
//!
//! let ident_packet = &[]; // Replace with real data.
//! let comment_packet = &[]; // Replace with real data.
//! let setup_packet = &[]; // Replace with real data.
//!
//! let mut builder = Decoder::builder();
//! builder.read_ident_packet(&mut BitReader::new(Cursor::new(ident_packet)))
//!         .expect("Couldn't read ident packet");
//! builder.read_comment_packet(&mut BitReader::new(Cursor::new(comment_packet)))
//!         .expect("Couldn't read comment packet");
//! builder.read_setup_packet(&mut BitReader::new(Cursor::new(setup_packet)))
//!         .expect("Couldn't read setup packet");
//! let mut decoder = builder.build();
//!
//! let mut sample_buf = Vec::with_capacity(decoder.header().frame_lens().long() * decoder.header().channel_count());
//!
//! loop {
//!     let audio_packet = &[]; // Replace with real data.
//!     decoder.decode(&mut BitReader::new(Cursor::new(audio_packet)))
//!             .expect("Couldn't decode audio packet");
//!     if decoder.samples().is_empty() {
//!         continue;
//!     }
//!     sample_buf.truncate(0);
//!     sample_buf.extend(decoder.samples().interleave()
//!             .map(|s| (s * 32767.0 + 0.5).floor() as i16));
//!
//!     // Do something with the sample_buf.
//! }
//! ```

#![no_std]

#[macro_use]
extern crate alloc;
extern crate libm;

mod bitstream;
mod codebook;
mod decoder;
mod error;
mod floor;
mod header;
mod huffman;
mod mapping;
mod mdct;
mod mode;
mod residue;
mod util;
mod window;

pub use bitstream::{BitRead, BitReader};
pub use decoder::{Decoder, DecoderBuilder, ChannelIter, InterleavedSamplesIter, Samples};
pub use error::{Error, ErrorKind, Result};
pub use header::*;
