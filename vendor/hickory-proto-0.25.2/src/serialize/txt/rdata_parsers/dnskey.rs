use alloc::string::String;
use core::str::FromStr as _;

use crate::dnssec::rdata::dnskey::DNSKEY;
use crate::dnssec::{Algorithm, PublicKeyBuf};
use crate::serialize::txt::{ParseError, ParseErrorKind, ParseResult};

pub(crate) fn parse<'i>(mut tokens: impl Iterator<Item = &'i str>) -> ParseResult<DNSKEY> {
    let flags_str = tokens
        .next()
        .ok_or_else(|| ParseError::from(ParseErrorKind::Message("flags not present")))?;
    let protocol_str = tokens
        .next()
        .ok_or_else(|| ParseError::from(ParseErrorKind::Message("protocol not present")))?;
    let algorithm_str = tokens
        .next()
        .ok_or_else(|| ParseError::from(ParseErrorKind::Message("algorithm not present")))?;

    let flags = u16::from_str(flags_str)?;

    let protocol = u8::from_str(protocol_str)?;

    if protocol != 3 {
        return Err(ParseError::from(ParseErrorKind::Message(
            "protocol field must be 3",
        )));
    }

    let algorithm = Algorithm::from_u8(algorithm_str.parse()?);

    let public_key_str: String = tokens.collect();
    if public_key_str.is_empty() {
        return Err(ParseError::from(ParseErrorKind::Message(
            "public key not present",
        )));
    }

    let public_key = data_encoding::BASE64.decode(public_key_str.as_bytes())?;

    Ok(DNSKEY::with_flags(
        flags,
        PublicKeyBuf::new(public_key, algorithm),
    ))
}
