use super::encoder::EncoderWriter;
use crate::engine::Engine;
use std::io;

/// A `Write` implementation that base64-encodes data using the provided config and accumulates the
/// resulting base64 utf8 `&str` in a [StrConsumer] implementation (typically `String`), which is
/// then exposed via `into_inner()`.
///
/// # Examples
///
/// Buffer base64 in a new String:
///
/// ```
/// use std::io::Write;
/// use base64::engine::general_purpose;
///
/// let mut enc = base64::write::EncoderStringWriter::new(&general_purpose::STANDARD);
///
/// enc.write_all(b"asdf").unwrap();
///
/// // get the resulting String
/// let b64_string = enc.into_inner();
///
/// assert_eq!("YXNkZg==", &b64_string);
/// ```
///
/// Or, append to an existing `String`, which implements `StrConsumer`:
///
/// ```
/// use std::io::Write;
/// use base64::engine::general_purpose;
///
/// let mut buf = String::from("base64: ");
///
/// let mut enc = base64::write::EncoderStringWriter::from_consumer(
///     &mut buf,
///     &general_purpose::STANDARD);
///
/// enc.write_all(b"asdf").unwrap();
///
/// // release the &mut reference on buf
/// let _ = enc.into_inner();
///
/// assert_eq!("base64: YXNkZg==", &buf);
/// ```
///
/// # Performance
///
/// Because it has to validate that the base64 is UTF-8, it is about 80% as fast as writing plain
/// bytes to a `io::Write`.
pub struct EncoderStringWriter<'e, E: Engine, S: StrConsumer> {
    encoder: EncoderWriter<'e, E, Utf8SingleCodeUnitWriter<S>>,
}

impl<'e, E: Engine, S: StrConsumer> EncoderStringWriter<'e, E, S> {
    /// Create a EncoderStringWriter that will append to the provided `StrConsumer`.
    pub fn from_consumer(str_consumer: S, engine: &'e E) -> Self {
        EncoderStringWriter {
            encoder: EncoderWriter::new(Utf8SingleCodeUnitWriter { str_consumer }, engine),
        }
    }

    /// Encode all remaining buffered data, including any trailing incomplete input triples and
    /// associated padding.
    ///
    /// Returns the base64-encoded form of the accumulated written data.
    pub fn into_inner(mut self) -> S {
        self.encoder
            .finish()
            .expect("Writing to a consumer should never fail")
            .str_consumer
    }
}

impl<'e, E: Engine> EncoderStringWriter<'e, E, String> {
    /// Create a EncoderStringWriter that will encode into a new `String` with the provided config.
    pub fn new(engine: &'e E) -> Self {
        EncoderStringWriter::from_consumer(String::new(), engine)
    }
}

impl<'e, E: Engine, S: StrConsumer> io::Write for EncoderStringWriter<'e, E, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.encoder.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.encoder.flush()
    }
}

/// An abstraction around consuming `str`s produced by base64 encoding.
pub trait StrConsumer {
    /// Consume the base64 encoded data in `buf`
    fn consume(&mut self, buf: &str);
}

/// As for io::Write, `StrConsumer` is implemented automatically for `&mut S`.
impl<S: StrConsumer + ?Sized> StrConsumer for &mut S {
    fn consume(&mut self, buf: &str) {
        (**self).consume(buf);
    }
}

/// Pushes the str onto the end of the String
impl StrConsumer for String {
    fn consume(&mut self, buf: &str) {
        self.push_str(buf);
    }
}

/// A `Write` that only can handle bytes that are valid single-byte UTF-8 code units.
///
/// This is safe because we only use it when writing base64, which is always valid UTF-8.
struct Utf8SingleCodeUnitWriter<S: StrConsumer> {
    str_consumer: S,
}

impl<S: StrConsumer> io::Write for Utf8SingleCodeUnitWriter<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Because we expect all input to be valid utf-8 individual bytes, we can encode any buffer
        // length
        let s = core::str::from_utf8(buf).expect("Input must be valid UTF-8");

        self.str_consumer.consume(s);

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // no op
        Ok(())
    }
}
