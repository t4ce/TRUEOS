// Copyright 2015-2023 Benjamin Fry <benjaminfry@me.com>
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

//! record data enum variants
use alloc::vec::Vec;

#[cfg(feature = "__dnssec")]
use crate::dnssec::rdata::DNSSECRData;
use crate::{
    rr::{
        Name, RData, RecordType,
        rdata::{ANAME, CNAME, HTTPS, NS, PTR},
    },
    serialize::txt::{
        errors::{ParseError, ParseErrorKind, ParseResult},
        rdata_parsers::*,
        zone_lex::Lexer,
    },
};

use super::Token;

/// Extension on RData for text parsing
pub trait RDataParser: Sized {
    /// Attempts to parse a stream of tokenized strs into the RData of the specified record type
    fn parse<'i, I: Iterator<Item = &'i str>>(
        record_type: RecordType,
        tokens: I,
        origin: Option<&Name>,
    ) -> ParseResult<Self>;

    /// Parse RData from a string
    fn try_from_str(record_type: RecordType, s: &str) -> ParseResult<Self> {
        let mut lexer = Lexer::new(s);
        let mut rdata = Vec::new();

        while let Some(token) = lexer.next_token()? {
            match token {
                Token::List(list) => rdata.extend(list),
                Token::CharData(s) => rdata.push(s),
                Token::EOL | Token::Blank => (),
                _ => {
                    return Err(ParseError::from(format!(
                        "unexpected token in record data: {token:?}"
                    )));
                }
            }
        }

        Self::parse(record_type, rdata.iter().map(AsRef::as_ref), None)
    }
}

#[warn(clippy::wildcard_enum_match_arm)] // make sure all cases are handled
impl RDataParser for RData {
    /// Parse the RData from a set of Tokens
    fn parse<'i, I: Iterator<Item = &'i str>>(
        record_type: RecordType,
        tokens: I,
        origin: Option<&Name>,
    ) -> ParseResult<Self> {
        let rdata = match record_type {
            RecordType::A => Self::A(a::parse(tokens)?),
            RecordType::AAAA => Self::AAAA(aaaa::parse(tokens)?),
            RecordType::ANAME => Self::ANAME(ANAME(name::parse(tokens, origin)?)),
            RecordType::ANY => return Err(ParseError::from("parsing ANY doesn't make sense")),
            RecordType::AXFR => return Err(ParseError::from("parsing AXFR doesn't make sense")),
            RecordType::CAA => caa::parse(tokens).map(Self::CAA)?,
            RecordType::CERT => Self::CERT(cert::parse(tokens)?),
            RecordType::CNAME => Self::CNAME(CNAME(name::parse(tokens, origin)?)),
            RecordType::CSYNC => csync::parse(tokens).map(Self::CSYNC)?,
            RecordType::HINFO => Self::HINFO(hinfo::parse(tokens)?),
            RecordType::HTTPS => svcb::parse(tokens).map(HTTPS).map(Self::HTTPS)?,
            RecordType::IXFR => return Err(ParseError::from("parsing IXFR doesn't make sense")),
            RecordType::MX => Self::MX(mx::parse(tokens, origin)?),
            RecordType::NAPTR => Self::NAPTR(naptr::parse(tokens, origin)?),
            RecordType::NULL => Self::NULL(null::parse(tokens)?),
            RecordType::NS => Self::NS(NS(name::parse(tokens, origin)?)),
            RecordType::OPENPGPKEY => Self::OPENPGPKEY(openpgpkey::parse(tokens)?),
            RecordType::OPT => return Err(ParseError::from("parsing OPT doesn't make sense")),
            RecordType::PTR => Self::PTR(PTR(name::parse(tokens, origin)?)),
            RecordType::SOA => Self::SOA(soa::parse(tokens, origin)?),
            RecordType::SRV => Self::SRV(srv::parse(tokens, origin)?),
            RecordType::SSHFP => Self::SSHFP(sshfp::parse(tokens)?),
            RecordType::SVCB => svcb::parse(tokens).map(Self::SVCB)?,
            RecordType::TLSA => Self::TLSA(tlsa::parse(tokens)?),
            RecordType::TXT => Self::TXT(txt::parse(tokens)?),
            RecordType::SIG => return Err(ParseError::from("parsing SIG doesn't make sense")),
            RecordType::DNSKEY => {
                return Err(ParseError::from("DNSKEY should be dynamically generated"));
            }
            RecordType::CDNSKEY => {
                return Err(ParseError::from("CDNSKEY should be dynamically generated"));
            }
            RecordType::KEY => return Err(ParseError::from("KEY should be dynamically generated")),
            #[cfg(feature = "__dnssec")]
            RecordType::DS => Self::DNSSEC(DNSSECRData::DS(ds::parse(tokens)?)),
            #[cfg(not(feature = "__dnssec"))]
            RecordType::DS => return Err(ParseError::from("DS should be dynamically generated")),
            RecordType::CDS => return Err(ParseError::from("CDS should be dynamically generated")),
            RecordType::NSEC => {
                return Err(ParseError::from("NSEC should be dynamically generated"));
            }
            RecordType::NSEC3 => {
                return Err(ParseError::from("NSEC3 should be dynamically generated"));
            }
            RecordType::NSEC3PARAM => {
                return Err(ParseError::from(
                    "NSEC3PARAM should be dynamically generated",
                ));
            }
            RecordType::RRSIG => {
                return Err(ParseError::from("RRSIG should be dynamically generated"));
            }
            RecordType::TSIG => return Err(ParseError::from("TSIG is only used during AXFR")),
            #[allow(deprecated)]
            RecordType::ZERO => Self::ZERO,
            r @ RecordType::Unknown(..) => {
                // TODO: add a way to associate generic record types to the zone
                return Err(ParseError::from(ParseErrorKind::UnsupportedRecordType(r)));
            }
        };

        Ok(rdata)
    }
}
