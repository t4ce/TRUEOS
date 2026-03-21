use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::ecma48;
pub(crate) use super::shell2_qjs_c4_contract::{
    Analysis, Diagnostic, ResultHint, SymbolRef, SymbolRole,
};

const COLOR_STRING: (u8, u8, u8) = (80, 210, 80);
const COLOR_NUMBER: (u8, u8, u8) = (225, 190, 70);
const COLOR_BOOLISH: (u8, u8, u8) = (95, 185, 235);
const COLOR_FUNCTION: (u8, u8, u8) = (220, 120, 220);
const COLOR_OBJECTISH: (u8, u8, u8) = (130, 170, 255);

#[derive(Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    Ident,
    String,
    Number,
    True,
    False,
    Null,
    Undefined,
    Let,
    Const,
    Var,
    LParen,
    RParen,
    Dot,
    Comma,
    Semi,
    Eq,
    EqEq,
    EqEqEq,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

#[derive(Clone, Copy)]
struct Token {
    kind: TokenKind,
    start: usize,
    end: usize,
}

#[derive(Clone)]
enum ExprKind {
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    Identifier(String),
    Member {
        base: Box<Expr>,
        name: String,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: TokenKind,
        right: Box<Expr>,
    },
    Group(Box<Expr>),
}

#[derive(Clone)]
struct Expr {
    kind: ExprKind,
}

struct Lexer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.bytes.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            let Some(b) = self.peek() else {
                return;
            };
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
                self.pos += 1;
                continue;
            }

            if b == b'/' && self.peek_next() == Some(b'/') {
                self.pos += 2;
                while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == b'\n' {
                        break;
                    }
                }
                continue;
            }

            if b == b'/' && self.peek_next() == Some(b'*') {
                self.pos += 2;
                while self.pos + 1 < self.bytes.len() {
                    if self.bytes[self.pos] == b'*' && self.bytes[self.pos + 1] == b'/' {
                        self.pos += 2;
                        break;
                    }
                    self.pos += 1;
                }
                continue;
            }

            return;
        }
    }

    fn ident_or_keyword(&mut self, start: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
                self.pos += 1;
            } else {
                break;
            }
        }
        let end = self.pos;
        let text = &self.src[start..end];
        let kind = match text {
            "let" => TokenKind::Let,
            "const" => TokenKind::Const,
            "var" => TokenKind::Var,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "null" => TokenKind::Null,
            "undefined" => TokenKind::Undefined,
            _ => TokenKind::Ident,
        };
        Token { kind, start, end }
    }

    fn number(&mut self, start: usize) -> Token {
        while let Some(c) = self.peek() {
            if c.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            while let Some(c) = self.peek() {
                if c.is_ascii_digit() {
                    self.pos += 1;
                } else {
                    break;
                }
            }
        }
        Token {
            kind: TokenKind::Number,
            start,
            end: self.pos,
        }
    }

    fn string(&mut self, start: usize, quote: u8) -> Result<Token, Diagnostic> {
        let mut escaped = false;
        while let Some(c) = self.bump() {
            if escaped {
                escaped = false;
                continue;
            }
            if c == b'\\' {
                escaped = true;
                continue;
            }
            if c == quote {
                return Ok(Token {
                    kind: TokenKind::String,
                    start,
                    end: self.pos,
                });
            }
        }

        Err(Diagnostic {
            code: "E_C4_STR",
            message: String::from("unterminated string literal"),
            start,
            end: self.pos,
        })
    }

    fn next_token(&mut self) -> Result<Token, Diagnostic> {
        self.skip_ws_and_comments();
        let start = self.pos;
        let Some(c) = self.bump() else {
            return Ok(Token {
                kind: TokenKind::Eof,
                start,
                end: start,
            });
        };

        let tok = match c {
            b'(' => Token {
                kind: TokenKind::LParen,
                start,
                end: self.pos,
            },
            b')' => Token {
                kind: TokenKind::RParen,
                start,
                end: self.pos,
            },
            b'.' => Token {
                kind: TokenKind::Dot,
                start,
                end: self.pos,
            },
            b',' => Token {
                kind: TokenKind::Comma,
                start,
                end: self.pos,
            },
            b';' => Token {
                kind: TokenKind::Semi,
                start,
                end: self.pos,
            },
            b'+' => Token {
                kind: TokenKind::Plus,
                start,
                end: self.pos,
            },
            b'-' => Token {
                kind: TokenKind::Minus,
                start,
                end: self.pos,
            },
            b'*' => Token {
                kind: TokenKind::Star,
                start,
                end: self.pos,
            },
            b'/' => Token {
                kind: TokenKind::Slash,
                start,
                end: self.pos,
            },
            b'=' => {
                if self.peek() == Some(b'=') {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        Token {
                            kind: TokenKind::EqEqEq,
                            start,
                            end: self.pos,
                        }
                    } else {
                        Token {
                            kind: TokenKind::EqEq,
                            start,
                            end: self.pos,
                        }
                    }
                } else {
                    Token {
                        kind: TokenKind::Eq,
                        start,
                        end: self.pos,
                    }
                }
            }
            b'\'' | b'"' => return self.string(start, c),
            d if d.is_ascii_digit() => return Ok(self.number(start)),
            a if a.is_ascii_alphabetic() || a == b'_' || a == b'$' => {
                return Ok(self.ident_or_keyword(start));
            }
            _ => {
                return Err(Diagnostic {
                    code: "E_C4_TOK",
                    message: alloc::format!("unexpected character '{}'", c as char),
                    start,
                    end: self.pos,
                });
            }
        };

        Ok(tok)
    }
}

struct Parser<'a> {
    src: &'a str,
    tokens: Vec<Token>,
    idx: usize,
    symbols: Vec<SymbolRef>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            src,
            tokens,
            idx: 0,
            symbols: Vec::new(),
        }
    }

    fn current(&self) -> Token {
        self.tokens[self.idx]
    }

    fn at(&self, kind: TokenKind) -> bool {
        self.current().kind == kind
    }

    fn bump(&mut self) -> Token {
        let t = self.current();
        if self.idx + 1 < self.tokens.len() {
            self.idx += 1;
        }
        t
    }

    fn eat(&mut self, kind: TokenKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn token_text(&self, t: Token) -> &str {
        &self.src[t.start..t.end]
    }

    fn expect(
        &mut self,
        kind: TokenKind,
        code: &'static str,
        msg: &str,
    ) -> Result<Token, Diagnostic> {
        if self.at(kind) {
            Ok(self.bump())
        } else {
            let cur = self.current();
            Err(Diagnostic {
                code,
                message: String::from(msg),
                start: cur.start,
                end: cur.end,
            })
        }
    }

    fn parse_program(&mut self) -> Result<ResultHint, Diagnostic> {
        let mut hint = ResultHint::Unknown;
        while !self.at(TokenKind::Eof) {
            hint = self.parse_statement()?;
            let _ = self.eat(TokenKind::Semi);
        }
        Ok(hint)
    }

    fn parse_statement(&mut self) -> Result<ResultHint, Diagnostic> {
        if self.at(TokenKind::Let) || self.at(TokenKind::Const) || self.at(TokenKind::Var) {
            self.bump();
            let id = self.expect(
                TokenKind::Ident,
                "E_C4_DECL",
                "expected identifier in declaration",
            )?;
            self.symbols.push(SymbolRef {
                name: String::from(self.token_text(id)),
                role: SymbolRole::Decl,
            });

            if self.eat(TokenKind::Eq) {
                let rhs = self.parse_assignment()?;
                self.collect_reads(&rhs);
                return Ok(self.expr_hint(&rhs));
            }
            return Ok(ResultHint::Undefined);
        }

        let expr = self.parse_assignment()?;
        self.collect_reads(&expr);
        Ok(self.expr_hint(&expr))
    }

    fn parse_assignment(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_equality()?;
        if self.eat(TokenKind::Eq) {
            if let Some(name) = self.assignable_name(&lhs) {
                self.symbols.push(SymbolRef {
                    name: String::from(name),
                    role: SymbolRole::Assign,
                });
            } else {
                let cur = self.current();
                return Err(Diagnostic {
                    code: "E_C4_ASSIGN",
                    message: String::from("invalid assignment target"),
                    start: cur.start,
                    end: cur.end,
                });
            }

            let rhs = self.parse_assignment()?;
            return Ok(rhs);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_additive()?;
        while self.at(TokenKind::EqEq) || self.at(TokenKind::EqEqEq) {
            let op = self.bump().kind;
            let right = self.parse_additive()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(expr)
    }

    fn parse_additive(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_multiplicative()?;
        while self.at(TokenKind::Plus) || self.at(TokenKind::Minus) {
            let op = self.bump().kind;
            let right = self.parse_multiplicative()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_postfix()?;
        while self.at(TokenKind::Star) || self.at(TokenKind::Slash) {
            let op = self.bump().kind;
            let right = self.parse_postfix()?;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
            };
        }
        Ok(expr)
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(TokenKind::Dot) {
                let member = self.expect(TokenKind::Ident, "E_C4_MEM", "expected property name")?;
                expr = Expr {
                    kind: ExprKind::Member {
                        base: Box::new(expr),
                        name: String::from(self.token_text(member)),
                    },
                };
                continue;
            }

            if self.eat(TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.at(TokenKind::RParen) {
                    loop {
                        args.push(self.parse_assignment()?);
                        if !self.eat(TokenKind::Comma) {
                            break;
                        }
                    }
                }
                let _ = self.expect(TokenKind::RParen, "E_C4_CALL", "expected ')' after call")?;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                };
                continue;
            }

            break;
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, Diagnostic> {
        let tok = self.bump();
        let kind = match tok.kind {
            TokenKind::String => ExprKind::String,
            TokenKind::Number => ExprKind::Number,
            TokenKind::True | TokenKind::False => ExprKind::Boolean,
            TokenKind::Null => ExprKind::Null,
            TokenKind::Undefined => ExprKind::Undefined,
            TokenKind::Ident => ExprKind::Identifier(String::from(self.token_text(tok))),
            TokenKind::LParen => {
                let inner = self.parse_assignment()?;
                let _ = self.expect(
                    TokenKind::RParen,
                    "E_C4_PAREN",
                    "expected ')' after expression",
                )?;
                ExprKind::Group(Box::new(inner))
            }
            _ => {
                return Err(Diagnostic {
                    code: "E_C4_EXPR",
                    message: String::from("expected expression"),
                    start: tok.start,
                    end: tok.end,
                });
            }
        };
        Ok(Expr { kind })
    }

    fn assignable_name<'b>(&'b self, expr: &'b Expr) -> Option<&'b str> {
        match &expr.kind {
            ExprKind::Identifier(name) => Some(name.as_str()),
            ExprKind::Member { .. } => None,
            ExprKind::Group(inner) => self.assignable_name(inner),
            _ => None,
        }
    }

    fn expr_hint(&self, expr: &Expr) -> ResultHint {
        match &expr.kind {
            ExprKind::String => ResultHint::String,
            ExprKind::Number => ResultHint::Number,
            ExprKind::Boolean => ResultHint::Boolean,
            ExprKind::Null => ResultHint::Null,
            ExprKind::Undefined => ResultHint::Undefined,
            ExprKind::Identifier(_) => ResultHint::Unknown,
            ExprKind::Member { .. } => ResultHint::Unknown,
            ExprKind::Call { callee, .. } => match &callee.kind {
                ExprKind::Identifier(name) if name == "String" => ResultHint::String,
                ExprKind::Identifier(name) if name == "Number" => ResultHint::Number,
                _ => ResultHint::Unknown,
            },
            ExprKind::Binary { left, op, right } => match op {
                TokenKind::Plus => {
                    let l = self.expr_hint(left);
                    let r = self.expr_hint(right);
                    if l == ResultHint::String || r == ResultHint::String {
                        ResultHint::String
                    } else if l == ResultHint::Number && r == ResultHint::Number {
                        ResultHint::Number
                    } else {
                        ResultHint::Unknown
                    }
                }
                TokenKind::EqEq | TokenKind::EqEqEq => ResultHint::Boolean,
                TokenKind::Minus | TokenKind::Star | TokenKind::Slash => ResultHint::Number,
                _ => ResultHint::Unknown,
            },
            ExprKind::Group(inner) => self.expr_hint(inner),
        }
    }

    fn collect_reads(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Identifier(name) => {
                self.symbols.push(SymbolRef {
                    name: name.clone(),
                    role: SymbolRole::Read,
                });
            }
            ExprKind::Member { base, name } => {
                self.collect_reads(base);
                self.symbols.push(SymbolRef {
                    name: name.clone(),
                    role: SymbolRole::Read,
                });
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Identifier(name) = &callee.kind {
                    self.symbols.push(SymbolRef {
                        name: name.clone(),
                        role: SymbolRole::Call,
                    });
                }
                self.collect_reads(callee);
                for arg in args {
                    self.collect_reads(arg);
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_reads(left);
                self.collect_reads(right);
            }
            ExprKind::Group(inner) => self.collect_reads(inner),
            ExprKind::String
            | ExprKind::Number
            | ExprKind::Boolean
            | ExprKind::Null
            | ExprKind::Undefined => {}
        }
    }
}

pub(crate) fn analyze(source: &str) -> Analysis {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();

    loop {
        match lexer.next_token() {
            Ok(token) => {
                let done = token.kind == TokenKind::Eof;
                tokens.push(token);
                if done {
                    break;
                }
            }
            Err(diag) => {
                return Analysis {
                    hint: ResultHint::Unknown,
                    symbols: Vec::new(),
                    diagnostic: Some(diag),
                };
            }
        }
    }

    let mut parser = Parser::new(source, tokens);
    match parser.parse_program() {
        Ok(hint) => Analysis {
            hint,
            symbols: parser.symbols,
            diagnostic: None,
        },
        Err(diag) => Analysis {
            hint: ResultHint::Unknown,
            symbols: parser.symbols,
            diagnostic: Some(diag),
        },
    }
}

pub(crate) fn format_tiny_diagnostic(diag: &Diagnostic) -> String {
    alloc::format!(
        "c4 {} @{}..{}: {}",
        diag.code,
        diag.start,
        diag.end,
        diag.message
    )
}

pub(crate) fn format_symbol_summary(analysis: &Analysis) -> Option<String> {
    if analysis.symbols.is_empty() {
        return None;
    }

    let mut parts = Vec::new();
    for sym in analysis.symbols.iter().take(4) {
        let role = match sym.role {
            SymbolRole::Decl => "decl",
            SymbolRole::Assign => "assign",
            SymbolRole::Read => "read",
            SymbolRole::Call => "call",
        };
        parts.push(alloc::format!("{} {}", role, sym.name));
    }

    let mut text = alloc::format!("c4: {}", parts.join(", "));
    if analysis.symbols.len() > parts.len() {
        text.push_str(", ...");
    }

    Some(alloc::format!("{}", ecma48::style(text.as_str()).dim()))
}

fn hinted_or_text_kind(hint: ResultHint, text: &str) -> ResultHint {
    if hint != ResultHint::Unknown {
        return hint;
    }

    let trimmed = text.trim();
    if trimmed == "undefined" {
        return ResultHint::Undefined;
    }
    if trimmed == "null" {
        return ResultHint::Null;
    }
    if trimmed == "true" || trimmed == "false" {
        return ResultHint::Boolean;
    }
    if trimmed.starts_with('"') || trimmed.starts_with('\'') || trimmed.starts_with('`') {
        return ResultHint::String;
    }
    if trimmed.parse::<f64>().is_ok() {
        return ResultHint::Number;
    }
    if trimmed.starts_with("function") || trimmed.contains("=>") {
        return ResultHint::Function;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return ResultHint::Object;
    }
    ResultHint::Unknown
}

pub(crate) fn format_result_text(text: &str, hint: ResultHint) -> String {
    let kind = hinted_or_text_kind(hint, text);
    match kind {
        ResultHint::String => alloc::format!("{}", ecma48::style(text).fg(COLOR_STRING)),
        ResultHint::Number => alloc::format!("{}", ecma48::style(text).fg(COLOR_NUMBER)),
        ResultHint::Boolean | ResultHint::Null | ResultHint::Undefined => {
            alloc::format!("{}", ecma48::style(text).fg(COLOR_BOOLISH).bold())
        }
        ResultHint::Function => alloc::format!("{}", ecma48::style(text).fg(COLOR_FUNCTION)),
        ResultHint::Object => alloc::format!("{}", ecma48::style(text).fg(COLOR_OBJECTISH)),
        ResultHint::Unknown => String::from(text),
    }
}
