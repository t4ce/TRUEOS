// Copyright 2015-2016 Benjamin Fry
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// https://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// https://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;
use core::{char, iter::Peekable};

use crate::serialize::txt::errors::{LexerError, LexerErrorKind, LexerResult};

/// A Lexer for Zone files
pub(crate) struct Lexer<'a> {
    txt: Peekable<CowChars<'a>>,
    state: State,
}

impl<'a> Lexer<'a> {
    /// Creates a new lexer with the given data to parse
    pub(crate) fn new(txt: impl Into<Cow<'a, str>>) -> Self {
        Lexer {
            txt: CowChars {
                data: txt.into(),
                offset: 0,
            }
            .peekable(),
            state: State::StartLine,
        }
    }

    /// Return the next Token in the string
    pub(crate) fn next_token(&mut self) -> LexerResult<Option<Token>> {
        let mut char_data_vec: Option<Vec<String>> = None;
        let mut char_data: Option<String> = None;

        for i in 0..4_096 {
            // max chars in a single lex, helps with issues in the lexer...
            assert!(i < 4095); // keeps the bounds of the loop defined (nothing lasts forever)

            // This is to get around mutability rules such that we can peek at the iter without moving next...
            let ch: Option<char> = self.peek();

            // handy line for debugging
            // debug!("ch = {:?}; state = {:?}(c: {:?}, v: {:?})", ch, self.state, char_data, char_data_vec);

            // continuing states should pass back the state as the last statement,
            //  terminal states should set the state internally and return the proper Token::*.
            // TODO: there is some non-ideal copying going on in here...
            match self.state {
                State::StartLine => {
                    match ch {
                        Some('\r') | Some('\n') => {
                            self.state = State::EOL;
                        }
                        // white space at the start of line is a Blank
                        Some(ch) if ch.is_whitespace() => self.state = State::Blank,
                        Some(_) => self.state = State::RestOfLine,
                        None => {
                            self.state = State::EOF;
                        }
                    }
                }
                State::RestOfLine => {
                    match ch {
                        Some('@') => self.state = State::At,
                        Some('(') => {
                            self.txt.next();
                            char_data_vec = Some(Vec::new());
                            self.state = State::List;
                        }
                        Some(ch @ ')') => return Err(LexerErrorKind::IllegalCharacter(ch).into()),
                        Some('$') => {
                            self.txt.next();
                            char_data = Some(String::new());
                            self.state = State::Dollar;
                        }
                        Some('\r') | Some('\n') => {
                            self.state = State::EOL;
                        }
                        Some('"') => {
                            self.txt.next();
                            char_data = Some(String::new());
                            self.state = State::Quote;
                        }
                        Some(';') => self.state = State::Comment { is_list: false },
                        Some(ch) if ch.is_whitespace() => {
                            self.txt.next();
                        } // gobble other whitespace
                        Some(ch) if !ch.is_control() && !ch.is_whitespace() => {
                            char_data = Some(String::new());
                            self.state = State::CharData { is_list: false };
                        }
                        Some(ch) => return Err(LexerErrorKind::UnrecognizedChar(ch).into()),
                        None => {
                            self.state = State::EOF;
                        }
                    }
                }
                State::Blank => {
                    // consume the whitespace
                    self.txt.next();
                    self.state = State::RestOfLine;
                    return Ok(Some(Token::Blank));
                }
                State::Comment { is_list } => {
                    match ch {
                        Some('\r') | Some('\n') => {
                            self.state = if is_list { State::List } else { State::EOL };
                        } // out of the comment
                        Some(_) => {
                            self.txt.next();
                        } // advance the token by default and maintain state
                        None => {
                            self.state = State::EOF;
                        }
                    }
                }
                State::Quote => {
                    match ch {
                        // end and gobble the '"'
                        Some('"') => {
                            self.state = State::RestOfLine;
                            self.txt.next();
                            return Ok(Some(Token::CharData(
                                char_data.take().unwrap_or_else(|| "".into()),
                            )));
                        }
                        Some('\\') => {
                            Self::push_to_str(&mut char_data, self.escape_seq()?)?;
                        }
                        Some(ch) => {
                            self.txt.next();
                            Self::push_to_str(&mut char_data, ch)?;
                        }
                        None => return Err(LexerErrorKind::UnclosedQuotedString.into()),
                    }
                }
                State::Dollar => {
                    match ch {
                        // even this is a little broad for what's actually possible in a dollar...
                        Some(ch @ 'A'..='Z') => {
                            self.txt.next();
                            Self::push_to_str(&mut char_data, ch)?;
                        }
                        // finishes the Dollar...
                        Some(_) | None => {
                            self.state = State::RestOfLine;
                            let dollar: String = char_data.take().ok_or_else(|| {
                                LexerError::from(LexerErrorKind::IllegalState(
                                    "char_data \
                                     is None",
                                ))
                            })?;

                            return Ok(Some(match dollar.as_str() {
                                "INCLUDE" => Token::Include,
                                "ORIGIN" => Token::Origin,
                                "TTL" => Token::Ttl,
                                _ => {
                                    return Err(LexerErrorKind::UnrecognizedDollar(
                                        char_data.take().unwrap_or_else(|| "".into()),
                                    )
                                    .into());
                                }
                            }));
                        }
                    }
                }
                State::List => match ch {
                    Some(';') => {
                        self.txt.next();
                        self.state = State::Comment { is_list: true }
                    }
                    Some(')') => {
                        self.txt.next();
                        self.state = State::RestOfLine;
                        return char_data_vec
                            .take()
                            .ok_or_else(|| {
                                LexerErrorKind::IllegalState("char_data_vec is None").into()
                            })
                            .map(|v| Some(Token::List(v)));
                    }
                    Some(ch) if ch.is_whitespace() => {
                        self.txt.next();
                    }
                    Some(ch) if !ch.is_control() && !ch.is_whitespace() => {
                        char_data = Some(String::new());
                        self.state = State::CharData { is_list: true }
                    }
                    Some(ch) => return Err(LexerErrorKind::UnrecognizedChar(ch).into()),
                    None => return Err(LexerErrorKind::UnclosedList.into()),
                },
                State::CharData { is_list } => {
                    match ch {
                        Some(ch @ ')') if !is_list => {
                            return Err(LexerErrorKind::IllegalCharacter(ch).into());
                        }
                        Some(ch) if ch.is_whitespace() || ch == ')' || ch == ';' => {
                            if is_list {
                                char_data_vec
                                    .as_mut()
                                    .ok_or_else(|| {
                                        LexerError::from(LexerErrorKind::IllegalState(
                                            "char_data_vec is None",
                                        ))
                                    })
                                    .and_then(|v| {
                                        let char_data = char_data.take().ok_or(
                                            LexerErrorKind::IllegalState("char_data is None"),
                                        )?;

                                        v.push(char_data);
                                        Ok(())
                                    })?;
                                self.state = State::List;
                            } else {
                                self.state = State::RestOfLine;
                                let result = char_data.take().ok_or_else(|| {
                                    LexerErrorKind::IllegalState("char_data is None").into()
                                });
                                let opt = result.map(|s| Some(Token::CharData(s)));
                                return opt;
                            }
                        }
                        // TODO: this next one can be removed, but will keep unescaping for quoted strings
                        //Some('\\') => { try!(Self::push_to_str(&mut char_data, try!(self.escape_seq()))); },
                        Some(ch) if !ch.is_control() && !ch.is_whitespace() => {
                            self.txt.next();
                            Self::push_to_str(&mut char_data, ch)?;
                        }
                        Some(ch) => return Err(LexerErrorKind::UnrecognizedChar(ch).into()),
                        None => {
                            self.state = State::EOF;
                            return char_data
                                .take()
                                .ok_or_else(|| {
                                    LexerErrorKind::IllegalState("char_data is None").into()
                                })
                                .map(|s| Some(Token::CharData(s)));
                        }
                    }
                }
                State::At => {
                    self.txt.next();
                    self.state = State::RestOfLine;
                    return Ok(Some(Token::At));
                }
                State::EOL => match ch {
                    Some('\r') => {
                        self.txt.next();
                    }
                    Some('\n') => {
                        self.txt.next();
                        self.state = State::StartLine;
                        return Ok(Some(Token::EOL));
                    }
                    Some(ch) => return Err(LexerErrorKind::IllegalCharacter(ch).into()),
                    None => return Err(LexerErrorKind::EOF.into()),
                },
                // to exhaust all cases, this should never be run...
                State::EOF => {
                    self.txt.next(); // making sure we consume the last... it will always return None after.
                    return Ok(None);
                }
            }
        }

        unreachable!("The above match statement should have found a terminal state");
    }

    fn push_to_str(collect: &mut Option<String>, ch: char) -> LexerResult<()> {
        collect
            .as_mut()
            .ok_or_else(|| LexerErrorKind::IllegalState("collect is None").into())
            .map(|s| {
                s.push(ch);
            })
    }

    fn escape_seq(&mut self) -> LexerResult<char> {
        // escaped character, let's decode it.
        self.txt.next(); // consume the escape
        let ch = self
            .peek()
            .ok_or_else(|| LexerError::from(LexerErrorKind::EOF))?;

        if !ch.is_control() {
            if ch.is_numeric() {
                // in this case it's an escaped octal: \DDD
                let d1: u32 = self
                    .txt
                    .next()
                    .ok_or_else(|| LexerError::from(LexerErrorKind::EOF))
                    .map(|c| {
                        c.to_digit(10)
                            .ok_or_else(|| LexerError::from(LexerErrorKind::IllegalCharacter(c)))
                    })??; // gobble
                let d2: u32 = self
                    .txt
                    .next()
                    .ok_or_else(|| LexerError::from(LexerErrorKind::EOF))
                    .map(|c| {
                        c.to_digit(10)
                            .ok_or_else(|| LexerError::from(LexerErrorKind::IllegalCharacter(c)))
                    })??; // gobble
                let d3: u32 = self
                    .txt
                    .next()
                    .ok_or_else(|| LexerError::from(LexerErrorKind::EOF))
                    .map(|c| {
                        c.to_digit(10)
                            .ok_or_else(|| LexerError::from(LexerErrorKind::IllegalCharacter(c)))
                    })??; // gobble

                let val: u32 = (d1 << 16) + (d2 << 8) + d3;
                let ch: char = char::from_u32(val)
                    .ok_or_else(|| LexerError::from(LexerErrorKind::UnrecognizedOctet(val)))?;

                Ok(ch)
            } else {
                // this is an escaped char: \X
                self.txt.next(); // gobble the char
                Ok(ch)
            }
        } else {
            Err(LexerErrorKind::IllegalCharacter(ch).into())
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.txt.peek().copied()
    }
}

struct CowChars<'a> {
    data: Cow<'a, str>,
    offset: usize,
}

impl Iterator for CowChars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<char> {
        let mut iter = self.data[self.offset..].char_indices();
        let (_, ch) = iter.next()?; // The returned index is always `0`
        match iter.next() {
            Some((idx, _)) => self.offset += idx,
            None => self.offset = self.data.len(),
        }

        Some(ch)
    }
}

#[doc(hidden)]
#[derive(Copy, Clone, PartialEq, Debug)]
pub(crate) enum State {
    StartLine,
    RestOfLine,
    Blank,                      // only if the first part of the line
    List,                       // (..)
    CharData { is_list: bool }, // [a-zA-Z, non-control utf8]+
    //  Name,              // CharData + '.' + CharData
    Comment { is_list: bool }, // ;.*
    At,                        // @
    Quote,                     // ".*"
    Dollar,                    // $
    EOL,                       // \n or \r\n
    EOF,
}

/// Tokens emited from each Lexer pass
#[derive(Eq, PartialEq, Debug, Clone)]
pub enum Token {
    /// only if the first part of the line
    Blank,
    /// (..) TODO, this is probably wrong, List maybe should just skip line endings
    List(Vec<String>),
    /// [a-zA-Z, non-control utf8, ., -, 0-9]+, ".*"
    CharData(String),
    /// @
    At,
    /// $INCLUDE
    Include,
    /// $ORIGIN
    Origin,
    /// $TTL
    Ttl,
    /// \n or \r\n
    EOL,
}

#[cfg(test)]
mod lex_test {
    use alloc::string::ToString;

    use super::*;

    #[allow(clippy::uninlined_format_args)]
    fn next_token(lexer: &mut Lexer<'_>) -> Option<Token> {
        let result = lexer.next_token();
        assert!(result.is_ok(), "{:?}", result);
        result.unwrap()
    }





    // fun with tests!!! lots of options


}
