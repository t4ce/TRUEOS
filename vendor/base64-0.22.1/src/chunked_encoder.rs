use crate::{
    encode::add_padding,
    engine::{Config, Engine},
};
#[cfg(any(feature = "alloc", test))]
use alloc::string::String;
#[cfg(any(feature = "alloc", test))]
use core::str;

/// The output mechanism for ChunkedEncoder's encoded bytes.
pub trait Sink {
    type Error;

    /// Handle a chunk of encoded base64 data (as UTF-8 bytes)
    fn write_encoded_bytes(&mut self, encoded: &[u8]) -> Result<(), Self::Error>;
}

/// A base64 encoder that emits encoded bytes in chunks without heap allocation.
pub struct ChunkedEncoder<'e, E: Engine + ?Sized> {
    engine: &'e E,
}

impl<'e, E: Engine + ?Sized> ChunkedEncoder<'e, E> {
    pub fn new(engine: &'e E) -> ChunkedEncoder<'e, E> {
        ChunkedEncoder { engine }
    }

    pub fn encode<S: Sink>(&self, bytes: &[u8], sink: &mut S) -> Result<(), S::Error> {
        const BUF_SIZE: usize = 1024;
        const CHUNK_SIZE: usize = BUF_SIZE / 4 * 3;

        let mut buf = [0; BUF_SIZE];
        for chunk in bytes.chunks(CHUNK_SIZE) {
            let mut len = self.engine.internal_encode(chunk, &mut buf);
            if chunk.len() != CHUNK_SIZE && self.engine.config().encode_padding() {
                // Final, potentially partial, chunk.
                // Only need to consider if padding is needed on a partial chunk since full chunk
                // is a multiple of 3, which therefore won't be padded.
                // Pad output to multiple of four bytes if required by config.
                len += add_padding(len, &mut buf[len..]);
            }
            sink.write_encoded_bytes(&buf[..len])?;
        }

        Ok(())
    }
}

// A really simple sink that just appends to a string
#[cfg(any(feature = "alloc", test))]
pub(crate) struct StringSink<'a> {
    string: &'a mut String,
}

#[cfg(any(feature = "alloc", test))]
impl<'a> StringSink<'a> {
    pub(crate) fn new(s: &mut String) -> StringSink<'_> {
        StringSink { string: s }
    }
}

#[cfg(any(feature = "alloc", test))]
impl<'a> Sink for StringSink<'a> {
    type Error = ();

    fn write_encoded_bytes(&mut self, s: &[u8]) -> Result<(), Self::Error> {
        self.string.push_str(str::from_utf8(s).unwrap());

        Ok(())
    }
}
