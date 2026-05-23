//! DNSSEC trust anchor file parser
//!
//! A trust anchor file largely adheres to the syntax of a zone file but may only contain
//! DNSKEY or DS records. DS records are currently unsupported

use alloc::{borrow::Cow, string::String, vec::Vec};
use core::str::FromStr;

use crate::{
    dnssec::rdata::DNSKEY,
    rr::{DNSClass, Name, RecordData, RecordType},
    serialize::txt::{
        ParseError, ParseErrorKind, ParseResult,
        rdata_parsers::dnskey,
        zone,
        zone_lex::{Lexer, Token as LexToken},
    },
};

/// DNSSEC trust anchor file parser
pub struct Parser<'a> {
    lexer: Lexer<'a>,
}

impl<'a> Parser<'a> {
    /// Returns a new trust anchor file parser
    pub fn new(input: impl Into<Cow<'a, str>>) -> Self {
        Self {
            lexer: Lexer::new(input),
        }
    }

    /// Parse a file from the Lexer
    ///
    /// Returns the records found in the file
    pub fn parse(mut self) -> ParseResult<Vec<Entry>> {
        let mut state = State::StartLine;
        let mut records = vec![];

        while let Some(token) = self.lexer.next_token()? {
            let token: Token = token.try_into()?;
            state = match state {
                State::StartLine => match token {
                    Token::Blank | Token::EOL => State::StartLine,
                    Token::CharData(data) => {
                        let name = Name::parse(&data, None)?;
                        State::Ttl { name }
                    }
                },

                State::Ttl { name } => {
                    if let Token::CharData(data) = token {
                        if let Ok(class) = DNSClass::from_str(&data) {
                            State::Type {
                                name,
                                ttl: None,
                                class,
                            }
                        } else {
                            let ttl = zone::Parser::parse_time(&data)?;
                            State::Class { name, ttl }
                        }
                    } else {
                        return Err(ParseErrorKind::UnexpectedToken(token.into()).into());
                    }
                }

                State::Class { name, ttl } => {
                    if let Token::CharData(mut data) = token {
                        data.make_ascii_uppercase();
                        let class = DNSClass::from_str(&data)?;
                        State::Type {
                            name,
                            ttl: Some(ttl),
                            class,
                        }
                    } else {
                        return Err(ParseErrorKind::UnexpectedToken(token.into()).into());
                    }
                }

                State::Type { name, ttl, class } => {
                    if let Token::CharData(data) = token {
                        let rtype = RecordType::from_str(&data)?;

                        if !matches!(rtype, RecordType::DNSKEY) {
                            return Err(ParseErrorKind::UnsupportedRecordType(rtype).into());
                        }

                        State::RData {
                            name,
                            ttl,
                            class,
                            parts: vec![],
                        }
                    } else {
                        return Err(ParseErrorKind::UnexpectedToken(token.into()).into());
                    }
                }

                State::RData {
                    name,
                    ttl,
                    class,
                    parts,
                } => match token {
                    Token::EOL => {
                        Self::flush_record(parts, name, ttl, class, &mut records)?;
                        State::StartLine
                    }

                    Token::CharData(data) => {
                        let mut parts = parts;
                        parts.push(data);
                        State::RData {
                            name,
                            ttl,
                            class,
                            parts,
                        }
                    }

                    _ => return Err(ParseErrorKind::UnexpectedToken(token.into()).into()),
                },
            };
        }

        if let State::RData {
            name,
            ttl,
            class,
            parts,
        } = state
        {
            Self::flush_record(parts, name, ttl, class, &mut records)?;
        }

        Ok(records)
    }

    fn flush_record(
        rdata_parts: Vec<String>,
        name: Name,
        ttl: Option<u32>,
        class: DNSClass,
        records: &mut Vec<Entry>,
    ) -> ParseResult<()> {
        let dnskey = dnskey::parse(rdata_parts.iter().map(AsRef::as_ref))?;

        let record = Record {
            name_labels: name,
            dns_class: class,
            ttl,
            rdata: dnskey,
        };

        records.push(Entry::DNSKEY(record));

        Ok(())
    }
}

/// An entry in the trust anchor file
#[derive(Debug)]
#[non_exhaustive]
pub enum Entry {
    /// A DNSKEY record
    DNSKEY(Record<DNSKEY>),
}

/// A resource record as it appears in a zone file
// like `rr::Record` but with optional TTL field
#[derive(Debug)]
pub struct Record<R> {
    name_labels: Name,
    dns_class: DNSClass,
    ttl: Option<u32>,
    rdata: R,
}

impl<R> Record<R> {
    /// Returns the Record Data, i.e. the record information
    pub fn data(&self) -> &R {
        &self.rdata
    }

    /// Returns the DNSClass of the Record, generally IN for internet
    pub fn dns_class(&self) -> DNSClass {
        self.dns_class
    }

    /// Returns the time-to-live of the record, if present
    pub fn ttl(&self) -> Option<u32> {
        self.ttl
    }

    /// Returns the name of the record
    pub fn name(&self) -> &Name {
        &self.name_labels
    }
}

impl<R: RecordData> Record<R> {
    /// Returns the type of the RecordData in the record
    pub fn record_type(&self) -> RecordType {
        self.data().record_type()
    }
}

enum State {
    /// Initial state
    StartLine,
    /// Expects TTL field (but it may be omitted)
    Ttl { name: Name },
    /// Expects Class field (after TTL has been parsed)
    Class { name: Name, ttl: u32 },
    /// Expects RecordType field
    Type {
        name: Name,
        ttl: Option<u32>,
        class: DNSClass,
    },
    /// Expects RDATA field
    RData {
        name: Name,
        ttl: Option<u32>,
        class: DNSClass,
        parts: Vec<String>,
    },
}

enum Token {
    Blank,
    CharData(String),
    EOL,
}

impl TryFrom<LexToken> for Token {
    type Error = ParseError;

    fn try_from(token: LexToken) -> Result<Self, Self::Error> {
        let token = match token {
            LexToken::At
            | LexToken::Include
            | LexToken::Origin
            | LexToken::Ttl
            | LexToken::List(_) => return Err(ParseErrorKind::UnexpectedToken(token).into()),
            LexToken::Blank => Self::Blank,
            LexToken::CharData(data) => Self::CharData(data),
            LexToken::EOL => Self::EOL,
        };
        Ok(token)
    }
}

impl From<Token> for LexToken {
    fn from(token: Token) -> Self {
        match token {
            Token::Blank => Self::Blank,
            Token::CharData(data) => Self::CharData(data),
            Token::EOL => Self::EOL,
        }
    }
}
