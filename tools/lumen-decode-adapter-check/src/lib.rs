#![no_std]

#[cfg(test)]
extern crate std;

#[path = "../../../src/r/lfm25_decode.rs"]
pub mod lfm25_decode;

pub mod r {
    pub use crate::lfm25_decode;
}

#[path = "../../../src/lumen/decode.rs"]
pub mod decode;
