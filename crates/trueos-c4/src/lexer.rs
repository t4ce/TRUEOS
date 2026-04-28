extern crate alloc;

use alloc::string::{String, ToString};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TokenKind {
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    LParen,
    RParen,
    Assign,
    Less,
    Greater,
    Plus,
    Minus,
    Div,
    Mul,
    Not,
    Semi,
    Comma,
    AndAnd,
    OrOr,
    EqEq,
    GreaterEq,
    LessEq,
    NotEq,
    Break,
    Do,
    Else,
    False,
    For,
    If,
    NoMain,
    NoStd,
    True,
    While,
    Basic(super::ast::Type),
    Id(String),
    Num(i64),
    Real(String),
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub message: String,
    pub span: Span,
}

pub struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    idx: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            idx: 0,
            line: 1,
            col: 1,
        }
    }

    fn span(&self) -> Span {
        Span {
            line: self.line,
            column: self.col,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.idx).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.idx + 1).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let ch = self.peek()?;
        self.idx += 1;
        if ch == b'\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(ch)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
                let _ = self.bump();
            }
            if self.peek() == Some(b'/') && self.peek_next() == Some(b'/') {
                while let Some(ch) = self.bump() {
                    if ch == b'\n' {
                        break;
                    }
                }
                continue;
            }
            break;
        }
    }

    pub fn next_token(&mut self) -> Result<Token, LexError> {
        self.skip_ws_and_comments();
        let span = self.span();
        let Some(ch) = self.peek() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                span,
            });
        };

        if self.src[self.idx..].starts_with("#![no_std]") {
            for _ in 0.."#![no_std]".len() {
                let _ = self.bump();
            }
            return Ok(Token {
                kind: TokenKind::NoStd,
                span,
            });
        }
        if self.src[self.idx..].starts_with("#![no_main]") {
            for _ in 0.."#![no_main]".len() {
                let _ = self.bump();
            }
            return Ok(Token {
                kind: TokenKind::NoMain,
                span,
            });
        }

        let one = match ch {
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b';' => Some(TokenKind::Semi),
            b',' => Some(TokenKind::Comma),
            b'+' => Some(TokenKind::Plus),
            b'-' => Some(TokenKind::Minus),
            b'*' => Some(TokenKind::Mul),
            _ => None,
        };
        if let Some(kind) = one {
            let _ = self.bump();
            return Ok(Token { kind, span });
        }

        match (ch, self.peek_next()) {
            (b'&', Some(b'&')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::AndAnd,
                    span,
                });
            }
            (b'|', Some(b'|')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::OrOr,
                    span,
                });
            }
            (b'=', Some(b'=')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::EqEq,
                    span,
                });
            }
            (b'>', Some(b'=')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::GreaterEq,
                    span,
                });
            }
            (b'<', Some(b'=')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::LessEq,
                    span,
                });
            }
            (b'!', Some(b'=')) => {
                let _ = self.bump();
                let _ = self.bump();
                return Ok(Token {
                    kind: TokenKind::NotEq,
                    span,
                });
            }
            _ => {}
        }

        let single = match ch {
            b'=' => Some(TokenKind::Assign),
            b'<' => Some(TokenKind::Less),
            b'>' => Some(TokenKind::Greater),
            b'/' => Some(TokenKind::Div),
            b'!' => Some(TokenKind::Not),
            _ => None,
        };
        if let Some(kind) = single {
            let _ = self.bump();
            return Ok(Token { kind, span });
        }

        if ch.is_ascii_digit() {
            let start = self.idx;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                let _ = self.bump();
            }
            if self.peek() == Some(b'.') {
                let _ = self.bump();
                while matches!(self.peek(), Some(c) if c.is_ascii_digit()) {
                    let _ = self.bump();
                }
                return Ok(Token {
                    kind: TokenKind::Real(self.src[start..self.idx].to_string()),
                    span,
                });
            }
            let value = self.src[start..self.idx]
                .parse::<i64>()
                .map_err(|_| LexError {
                    message: "invalid integer literal".to_string(),
                    span,
                })?;
            return Ok(Token {
                kind: TokenKind::Num(value),
                span,
            });
        }

        if ch.is_ascii_alphabetic() {
            let start = self.idx;
            while matches!(self.peek(), Some(c) if c.is_ascii_alphanumeric()) {
                let _ = self.bump();
            }
            let ident = &self.src[start..self.idx];
            let kind = match ident {
                "break" => TokenKind::Break,
                "do" => TokenKind::Do,
                "else" => TokenKind::Else,
                "false" => TokenKind::False,
                "for" => TokenKind::For,
                "if" => TokenKind::If,
                "true" => TokenKind::True,
                "while" => TokenKind::While,
                "int" => TokenKind::Basic(super::ast::Type::Int),
                "float" => TokenKind::Basic(super::ast::Type::Float),
                "char" => TokenKind::Basic(super::ast::Type::Char),
                "bool" => TokenKind::Basic(super::ast::Type::Bool),
                _ => TokenKind::Id(ident.to_string()),
            };
            return Ok(Token { kind, span });
        }

        Err(LexError {
            message: "unknown character".to_string(),
            span,
        })
    }
}
