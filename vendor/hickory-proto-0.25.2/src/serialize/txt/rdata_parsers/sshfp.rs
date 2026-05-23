// Copyright 2019 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! SSHFP records for SSH public key fingerprints


use crate::rr::rdata::{SSHFP, sshfp};
use crate::serialize::txt::errors::{ParseError, ParseErrorKind, ParseResult};

/// Parse the RData from a set of Tokens
///
/// [RFC 4255](https://tools.ietf.org/html/rfc4255#section-3.2)
///
/// ```text
/// 3.2.  Presentation Format of the SSHFP RR
///
///    The RDATA of the presentation format of the SSHFP resource record
///    consists of two numbers (algorithm and fingerprint type) followed by
///    the fingerprint itself, presented in hex, e.g.:
///
///        host.example.  SSHFP 2 1 123456789abcdef67890123456789abcdef67890
///
///    The use of mnemonics instead of numbers is not allowed.
/// ```
pub(crate) fn parse<'i, I: Iterator<Item = &'i str>>(mut tokens: I) -> ParseResult<SSHFP> {
    fn missing_field<E: From<ParseErrorKind>>(field: &str) -> E {
        ParseErrorKind::Msg(format!("SSHFP {field} field missing")).into()
    }
    let (algorithm, fingerprint_type) = {
        let mut parse_u8 = |field: &str| {
            tokens
                .next()
                .ok_or_else(|| missing_field(field))
                .and_then(|t| t.parse::<u8>().map_err(ParseError::from))
        };
        (
            parse_u8("algorithm")?.into(),
            parse_u8("fingerprint type")?.into(),
        )
    };
    let fingerprint = sshfp::HEX.decode(
        tokens
            .next()
            .filter(|fp| !fp.is_empty())
            .ok_or_else(|| missing_field::<ParseError>("fingerprint"))?
            .as_bytes(),
    )?;
    Some(SSHFP::new(algorithm, fingerprint_type, fingerprint))
        .filter(|_| tokens.next().is_none())
        .ok_or_else(|| ParseErrorKind::Message("too many fields for SSHFP").into())
}

