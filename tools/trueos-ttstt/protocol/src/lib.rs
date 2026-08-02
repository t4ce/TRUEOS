use std::error::Error;
use std::fmt;
use std::io::{self, Read, Write};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const DEFAULT_ADDRESS: &str = "127.0.0.1:1100";
pub const PROTOCOL_VERSION: u8 = 1;
pub const MAX_JSON_PAYLOAD: usize = 64 * 1024;
pub const MAX_PCM_PAYLOAD: usize = 8 * 1024;
pub const MAX_FRAME_PAYLOAD: usize = 1024 * 1024;

const MAGIC: [u8; 4] = *b"TTST";
const HEADER_LEN: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Kind {
    Hello = 1,
    HelloOk = 2,
    Error = 3,
    Ping = 4,
    Pong = 5,
    SttOpen = 10,
    SttReady = 11,
    SttPcm = 12,
    SttFinish = 13,
    SttCancel = 14,
    SttText = 15,
    SttDone = 16,
    TtsSpeak = 20,
    TtsAccepted = 21,
    TtsCancel = 22,
    TtsDone = 23,
}

impl TryFrom<u8> for Kind {
    type Error = ProtocolError;

    fn try_from(value: u8) -> Result<Self, ProtocolError> {
        match value {
            1 => Ok(Self::Hello),
            2 => Ok(Self::HelloOk),
            3 => Ok(Self::Error),
            4 => Ok(Self::Ping),
            5 => Ok(Self::Pong),
            10 => Ok(Self::SttOpen),
            11 => Ok(Self::SttReady),
            12 => Ok(Self::SttPcm),
            13 => Ok(Self::SttFinish),
            14 => Ok(Self::SttCancel),
            15 => Ok(Self::SttText),
            16 => Ok(Self::SttDone),
            20 => Ok(Self::TtsSpeak),
            21 => Ok(Self::TtsAccepted),
            22 => Ok(Self::TtsCancel),
            23 => Ok(Self::TtsDone),
            other => Err(ProtocolError::InvalidKind(other)),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub struct Frame {
    pub kind: Kind,
    pub request_id: u32,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn empty(kind: Kind, request_id: u32) -> Self {
        Self {
            kind,
            request_id,
            payload: Vec::new(),
        }
    }

    pub fn json<T: Serialize>(
        kind: Kind,
        request_id: u32,
        value: &T,
    ) -> Result<Self, ProtocolError> {
        let payload = serde_json::to_vec(value).map_err(ProtocolError::Json)?;
        validate_payload(kind, payload.len())?;
        Ok(Self {
            kind,
            request_id,
            payload,
        })
    }

    pub fn decode_json<T: DeserializeOwned>(&self) -> Result<T, ProtocolError> {
        if self.payload.len() > MAX_JSON_PAYLOAD {
            return Err(ProtocolError::PayloadTooLarge {
                kind: self.kind,
                length: self.payload.len(),
                maximum: MAX_JSON_PAYLOAD,
            });
        }
        serde_json::from_slice(&self.payload).map_err(ProtocolError::Json)
    }
}

#[derive(Debug)]
pub enum ProtocolError {
    Io(io::Error),
    Json(serde_json::Error),
    BadMagic([u8; 4]),
    UnsupportedVersion(u8),
    UnsupportedFlags(u16),
    InvalidKind(u8),
    PayloadTooLarge {
        kind: Kind,
        length: usize,
        maximum: usize,
    },
    OddPcmLength(usize),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "protocol I/O error: {error}"),
            Self::Json(error) => write!(formatter, "invalid protocol JSON: {error}"),
            Self::BadMagic(magic) => write!(formatter, "invalid protocol magic {magic:?}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "unsupported protocol version {version}")
            }
            Self::UnsupportedFlags(flags) => write!(formatter, "unsupported frame flags {flags}"),
            Self::InvalidKind(kind) => write!(formatter, "unknown frame kind {kind}"),
            Self::PayloadTooLarge {
                kind,
                length,
                maximum,
            } => write!(
                formatter,
                "{kind:?} payload is {length} bytes; maximum is {maximum}"
            ),
            Self::OddPcmLength(length) => {
                write!(formatter, "PCM payload has odd byte length {length}")
            }
        }
    }
}

impl Error for ProtocolError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProtocolError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

pub fn read_frame(reader: &mut impl Read) -> Result<Option<Frame>, ProtocolError> {
    let mut header = [0_u8; HEADER_LEN];
    let first = loop {
        match reader.read(&mut header[..1]) {
            Ok(0) => return Ok(None),
            Ok(1) => break 1,
            Ok(_) => unreachable!("one-byte read returned more than one byte"),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(ProtocolError::Io(error)),
        }
    };
    debug_assert_eq!(first, 1);
    reader.read_exact(&mut header[1..])?;

    let magic = [header[0], header[1], header[2], header[3]];
    if magic != MAGIC {
        return Err(ProtocolError::BadMagic(magic));
    }
    if header[4] != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(header[4]));
    }
    let kind = Kind::try_from(header[5])?;
    let flags = u16::from_be_bytes([header[6], header[7]]);
    if flags != 0 {
        return Err(ProtocolError::UnsupportedFlags(flags));
    }
    let request_id = u32::from_be_bytes([header[8], header[9], header[10], header[11]]);
    let length = u32::from_be_bytes([header[12], header[13], header[14], header[15]]) as usize;
    validate_payload(kind, length)?;

    let mut payload = vec![0_u8; length];
    reader.read_exact(&mut payload)?;
    Ok(Some(Frame {
        kind,
        request_id,
        payload,
    }))
}

pub fn write_frame(writer: &mut impl Write, frame: &Frame) -> Result<(), ProtocolError> {
    validate_payload(frame.kind, frame.payload.len())?;
    let length =
        u32::try_from(frame.payload.len()).map_err(|_| ProtocolError::PayloadTooLarge {
            kind: frame.kind,
            length: frame.payload.len(),
            maximum: MAX_FRAME_PAYLOAD,
        })?;

    let mut header = [0_u8; HEADER_LEN];
    header[..4].copy_from_slice(&MAGIC);
    header[4] = PROTOCOL_VERSION;
    header[5] = frame.kind as u8;
    header[8..12].copy_from_slice(&frame.request_id.to_be_bytes());
    header[12..16].copy_from_slice(&length.to_be_bytes());
    writer.write_all(&header)?;
    writer.write_all(&frame.payload)?;
    writer.flush()?;
    Ok(())
}

fn validate_payload(kind: Kind, length: usize) -> Result<(), ProtocolError> {
    let maximum = match kind {
        Kind::SttPcm => MAX_PCM_PAYLOAD,
        _ => MAX_JSON_PAYLOAD,
    };
    if length > maximum || length > MAX_FRAME_PAYLOAD {
        return Err(ProtocolError::PayloadTooLarge {
            kind,
            length,
            maximum: maximum.min(MAX_FRAME_PAYLOAD),
        });
    }
    if kind == Kind::SttPcm && !length.is_multiple_of(2) {
        return Err(ProtocolError::OddPcmLength(length));
    }
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Hello {
    pub client: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HelloOk {
    pub server: String,
    pub protocol: u8,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SttOpen {
    pub language: String,
    /// Detect the spoken language for this session instead of using `language`.
    #[serde(default)]
    pub detect_language: bool,
    pub translate: bool,
    pub prompt: Option<String>,
    pub vad_threshold: f32,
    pub no_speech_threshold: f32,
}

impl Default for SttOpen {
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            detect_language: false,
            translate: false,
            prompt: None,
            vad_threshold: 0.01,
            no_speech_threshold: 0.2,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SttText {
    pub sequence: u64,
    pub text: String,
    pub utterance_end: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Done {
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TtsSpeak {
    pub text: String,
    pub voice: String,
    pub speed: f32,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TtsDone {
    pub reason: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ErrorMessage {
    pub code: String,
    pub message: String,
    pub fatal: bool,
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{
        Frame, HEADER_LEN, Hello, Kind, MAGIC, MAX_PCM_PAYLOAD, PROTOCOL_VERSION, ProtocolError,
        read_frame, write_frame,
    };

    #[test]
    fn frame_round_trip_handles_partial_reads() {
        let frame = Frame::json(
            Kind::Hello,
            0,
            &Hello {
                client: "txt".to_owned(),
            },
        )
        .unwrap();
        let mut bytes = Vec::new();
        write_frame(&mut bytes, &frame).unwrap();

        let mut reader = OneByteReader(Cursor::new(bytes));
        let decoded = read_frame(&mut reader).unwrap().unwrap();
        assert_eq!(decoded, frame);
        assert_eq!(decoded.decode_json::<Hello>().unwrap().client, "txt");
        assert!(read_frame(&mut reader).unwrap().is_none());
    }

    #[test]
    fn bad_magic_is_rejected() {
        let mut bytes = vec![0_u8; 16];
        bytes[4] = 1;
        bytes[5] = Kind::Ping as u8;
        let error = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert!(matches!(error, ProtocolError::BadMagic(_)));
    }

    #[test]
    fn odd_pcm_payload_is_rejected() {
        let frame = Frame {
            kind: Kind::SttPcm,
            request_id: 1,
            payload: vec![0; 3],
        };
        assert!(matches!(
            write_frame(&mut Vec::new(), &frame),
            Err(ProtocolError::OddPcmLength(3))
        ));
    }

    #[test]
    fn oversized_pcm_payload_is_rejected_before_allocation() {
        let frame = Frame {
            kind: Kind::SttPcm,
            request_id: 1,
            payload: vec![0; MAX_PCM_PAYLOAD + 2],
        };
        assert!(matches!(
            write_frame(&mut Vec::new(), &frame),
            Err(ProtocolError::PayloadTooLarge { .. })
        ));
    }

    #[test]
    fn oversized_declared_payload_is_rejected_before_reading_it() {
        let mut header = encoded_header(Kind::SttPcm, (MAX_PCM_PAYLOAD + 2) as u32);
        let error = read_frame(&mut Cursor::new(&mut header)).unwrap_err();
        assert!(matches!(error, ProtocolError::PayloadTooLarge { .. }));
    }

    #[test]
    fn clean_eof_is_distinct_from_a_truncated_header() {
        assert!(
            read_frame(&mut Cursor::new(Vec::<u8>::new()))
                .unwrap()
                .is_none()
        );

        let header = encoded_header(Kind::Ping, 0);
        let error = read_frame(&mut Cursor::new(&header[..HEADER_LEN - 1])).unwrap_err();
        assert!(matches!(
            error,
            ProtocolError::Io(error) if error.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }

    #[test]
    fn truncated_payload_is_an_error() {
        let mut bytes = encoded_header(Kind::Hello, 4).to_vec();
        bytes.extend_from_slice(b"{}");
        let error = read_frame(&mut Cursor::new(bytes)).unwrap_err();
        assert!(matches!(
            error,
            ProtocolError::Io(error) if error.kind() == std::io::ErrorKind::UnexpectedEof
        ));
    }

    #[test]
    fn older_stt_open_payloads_keep_language_detection_disabled() {
        let request: super::SttOpen = serde_json::from_str(
            r#"{"language":"en","translate":false,"prompt":null,"vad_threshold":0.01,"no_speech_threshold":0.2}"#,
        )
        .unwrap();
        assert!(!request.detect_language);
        assert_eq!(request.language, "en");
    }

    fn encoded_header(kind: Kind, payload_length: u32) -> [u8; HEADER_LEN] {
        let mut header = [0_u8; HEADER_LEN];
        header[..4].copy_from_slice(&MAGIC);
        header[4] = PROTOCOL_VERSION;
        header[5] = kind as u8;
        header[8..12].copy_from_slice(&1_u32.to_be_bytes());
        header[12..16].copy_from_slice(&payload_length.to_be_bytes());
        header
    }

    struct OneByteReader<R>(R);

    impl<R: std::io::Read> std::io::Read for OneByteReader<R> {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let length = buffer.len().min(1);
            self.0.read(&mut buffer[..length])
        }
    }
}
