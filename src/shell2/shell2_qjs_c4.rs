use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use super::ecma48;
pub(crate) use super::shell2_qjs_c4_contract::{
    Analysis, Diagnostic, ExprNodeKind, NodeRef, ResultHint, Span, SymbolRef, SymbolRole,
    TokenKind as C4TokenKind, TokenRef,
};

const COLOR_STRING: (u8, u8, u8) = (80, 210, 80);
const COLOR_NUMBER: (u8, u8, u8) = (225, 190, 70);
const COLOR_BOOLISH: (u8, u8, u8) = (95, 185, 235);
const COLOR_FUNCTION: (u8, u8, u8) = (220, 120, 220);
const COLOR_OBJECTISH: (u8, u8, u8) = (130, 170, 255);
const COLOR_KEY: (u8, u8, u8) = (110, 160, 245);
const COLOR_PUNCT: (u8, u8, u8) = (140, 140, 140);
const PRETTY_MAX_BYTES: usize = 2 * 1024;
const PRETTY_MAX_TOKENS: usize = 512;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormatFallback {
    LimitReached,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum LexTokenKind {
    Ident,
    String,
    Number,
    True,
    False,
    Null,
    Undefined,
    Function,
    Async,
    Let,
    Const,
    Var,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Dot,
    Comma,
    Colon,
    Semi,
    Eq,
    EqEq,
    EqEqEq,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
}

#[derive(Clone, Copy)]
struct Token {
    kind: LexTokenKind,
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
        op: LexTokenKind,
        right: Box<Expr>,
    },
    Group(Box<Expr>),
}

#[derive(Clone)]
struct Expr {
    kind: ExprKind,
    start: usize,
    end: usize,
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
            "let" => LexTokenKind::Let,
            "const" => LexTokenKind::Const,
            "var" => LexTokenKind::Var,
            "function" => LexTokenKind::Function,
            "async" => LexTokenKind::Async,
            "true" => LexTokenKind::True,
            "false" => LexTokenKind::False,
            "null" => LexTokenKind::Null,
            "undefined" => LexTokenKind::Undefined,
            _ => LexTokenKind::Ident,
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
            kind: LexTokenKind::Number,
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
                    kind: LexTokenKind::String,
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
                kind: LexTokenKind::Eof,
                start,
                end: start,
            });
        };

        let tok = match c {
            b'(' => Token {
                kind: LexTokenKind::LParen,
                start,
                end: self.pos,
            },
            b')' => Token {
                kind: LexTokenKind::RParen,
                start,
                end: self.pos,
            },
            b'{' => Token {
                kind: LexTokenKind::LBrace,
                start,
                end: self.pos,
            },
            b'}' => Token {
                kind: LexTokenKind::RBrace,
                start,
                end: self.pos,
            },
            b'[' => Token {
                kind: LexTokenKind::LBracket,
                start,
                end: self.pos,
            },
            b']' => Token {
                kind: LexTokenKind::RBracket,
                start,
                end: self.pos,
            },
            b'.' => Token {
                kind: LexTokenKind::Dot,
                start,
                end: self.pos,
            },
            b',' => Token {
                kind: LexTokenKind::Comma,
                start,
                end: self.pos,
            },
            b':' => Token {
                kind: LexTokenKind::Colon,
                start,
                end: self.pos,
            },
            b';' => Token {
                kind: LexTokenKind::Semi,
                start,
                end: self.pos,
            },
            b'+' => Token {
                kind: LexTokenKind::Plus,
                start,
                end: self.pos,
            },
            b'-' => Token {
                kind: LexTokenKind::Minus,
                start,
                end: self.pos,
            },
            b'*' => Token {
                kind: LexTokenKind::Star,
                start,
                end: self.pos,
            },
            b'/' => Token {
                kind: LexTokenKind::Slash,
                start,
                end: self.pos,
            },
            b'=' => {
                if self.peek() == Some(b'>') {
                    self.pos += 1;
                    Token {
                        kind: LexTokenKind::Arrow,
                        start,
                        end: self.pos,
                    }
                } else if self.peek() == Some(b'=') {
                    self.pos += 1;
                    if self.peek() == Some(b'=') {
                        self.pos += 1;
                        Token {
                            kind: LexTokenKind::EqEqEq,
                            start,
                            end: self.pos,
                        }
                    } else {
                        Token {
                            kind: LexTokenKind::EqEq,
                            start,
                            end: self.pos,
                        }
                    }
                } else {
                    Token {
                        kind: LexTokenKind::Eq,
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
    nodes: Vec<NodeRef>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            src,
            tokens,
            idx: 0,
            symbols: Vec::new(),
            nodes: Vec::new(),
        }
    }

    fn push_node(&mut self, kind: ExprNodeKind, start: usize, end: usize) {
        self.nodes.push(NodeRef {
            kind,
            span: Span { start, end },
        });
    }

    fn current(&self) -> Token {
        self.tokens[self.idx]
    }

    fn at(&self, kind: LexTokenKind) -> bool {
        self.current().kind == kind
    }

    fn bump(&mut self) -> Token {
        let t = self.current();
        if self.idx + 1 < self.tokens.len() {
            self.idx += 1;
        }
        t
    }

    fn eat(&mut self, kind: LexTokenKind) -> bool {
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
        kind: LexTokenKind,
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
        let start = self.current().start;
        while !self.at(LexTokenKind::Eof) {
            hint = self.parse_statement()?;
            let _ = self.eat(LexTokenKind::Semi);
        }
        let end = self.current().end;
        self.push_node(ExprNodeKind::Program, start, end);
        Ok(hint)
    }

    fn parse_statement(&mut self) -> Result<ResultHint, Diagnostic> {
        if self.at(LexTokenKind::Let) || self.at(LexTokenKind::Const) || self.at(LexTokenKind::Var)
        {
            let decl_start = self.current().start;
            self.bump();
            let id = self.expect(
                LexTokenKind::Ident,
                "E_C4_DECL",
                "expected identifier in declaration",
            )?;
            self.symbols.push(SymbolRef {
                name: String::from(self.token_text(id)),
                role: SymbolRole::Decl,
            });

            if self.eat(LexTokenKind::Eq) {
                let rhs = self.parse_assignment()?;
                self.collect_reads(&rhs);
                self.collect_nodes(&rhs);
                self.push_node(ExprNodeKind::Declaration, decl_start, rhs.end);
                return Ok(self.expr_hint(&rhs));
            }
            self.push_node(ExprNodeKind::Declaration, decl_start, id.end);
            return Ok(ResultHint::Undefined);
        }

        let expr = self.parse_assignment()?;
        self.collect_reads(&expr);
        self.collect_nodes(&expr);
        Ok(self.expr_hint(&expr))
    }

    fn parse_assignment(&mut self) -> Result<Expr, Diagnostic> {
        let lhs = self.parse_equality()?;
        if self.eat(LexTokenKind::Eq) {
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
            self.push_node(ExprNodeKind::Assignment, lhs.start, rhs.end);
            return Ok(rhs);
        }
        Ok(lhs)
    }

    fn parse_equality(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_additive()?;
        while self.at(LexTokenKind::EqEq) || self.at(LexTokenKind::EqEqEq) {
            let op = self.bump().kind;
            let right = self.parse_additive()?;
            let start = expr.start;
            let end = right.end;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                start,
                end,
            };
        }
        Ok(expr)
    }

    fn parse_additive(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_multiplicative()?;
        while self.at(LexTokenKind::Plus) || self.at(LexTokenKind::Minus) {
            let op = self.bump().kind;
            let right = self.parse_multiplicative()?;
            let start = expr.start;
            let end = right.end;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                start,
                end,
            };
        }
        Ok(expr)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_postfix()?;
        while self.at(LexTokenKind::Star) || self.at(LexTokenKind::Slash) {
            let op = self.bump().kind;
            let right = self.parse_postfix()?;
            let start = expr.start;
            let end = right.end;
            expr = Expr {
                kind: ExprKind::Binary {
                    left: Box::new(expr),
                    op,
                    right: Box::new(right),
                },
                start,
                end,
            };
        }
        Ok(expr)
    }

    fn parse_postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expr = self.parse_primary()?;
        loop {
            if self.eat(LexTokenKind::Dot) {
                let member =
                    self.expect(LexTokenKind::Ident, "E_C4_MEM", "expected property name")?;
                let start = expr.start;
                expr = Expr {
                    kind: ExprKind::Member {
                        base: Box::new(expr),
                        name: String::from(self.token_text(member)),
                    },
                    start,
                    end: member.end,
                };
                continue;
            }

            if self.eat(LexTokenKind::LParen) {
                let mut args = Vec::new();
                if !self.at(LexTokenKind::RParen) {
                    loop {
                        args.push(self.parse_assignment()?);
                        if !self.eat(LexTokenKind::Comma) {
                            break;
                        }
                    }
                }
                let close =
                    self.expect(LexTokenKind::RParen, "E_C4_CALL", "expected ')' after call")?;
                let start = expr.start;
                expr = Expr {
                    kind: ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    start,
                    end: close.end,
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
            LexTokenKind::String => ExprKind::String,
            LexTokenKind::Number => ExprKind::Number,
            LexTokenKind::True | LexTokenKind::False => ExprKind::Boolean,
            LexTokenKind::Null => ExprKind::Null,
            LexTokenKind::Undefined => ExprKind::Undefined,
            LexTokenKind::Ident => ExprKind::Identifier(String::from(self.token_text(tok))),
            LexTokenKind::LParen => {
                let inner = self.parse_assignment()?;
                let close = self.expect(
                    LexTokenKind::RParen,
                    "E_C4_PAREN",
                    "expected ')' after expression",
                )?;
                return Ok(Expr {
                    kind: ExprKind::Group(Box::new(inner)),
                    start: tok.start,
                    end: close.end,
                });
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
        Ok(Expr {
            kind,
            start: tok.start,
            end: tok.end,
        })
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
                LexTokenKind::Plus => {
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
                LexTokenKind::EqEq | LexTokenKind::EqEqEq => ResultHint::Boolean,
                LexTokenKind::Minus | LexTokenKind::Star | LexTokenKind::Slash => {
                    ResultHint::Number
                }
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

    fn collect_nodes(&mut self, expr: &Expr) {
        let kind = match expr.kind {
            ExprKind::String => ExprNodeKind::StringLiteral,
            ExprKind::Number => ExprNodeKind::NumberLiteral,
            ExprKind::Boolean => ExprNodeKind::BooleanLiteral,
            ExprKind::Null => ExprNodeKind::NullLiteral,
            ExprKind::Undefined => ExprNodeKind::UndefinedLiteral,
            ExprKind::Identifier(_) => ExprNodeKind::Identifier,
            ExprKind::Member { .. } => ExprNodeKind::MemberAccess,
            ExprKind::Call { .. } => ExprNodeKind::Call,
            ExprKind::Binary { .. } => ExprNodeKind::Binary,
            ExprKind::Group(_) => ExprNodeKind::Group,
        };

        self.push_node(kind, expr.start, expr.end);

        match &expr.kind {
            ExprKind::Member { base, .. } => self.collect_nodes(base),
            ExprKind::Call { callee, args } => {
                self.collect_nodes(callee);
                for arg in args {
                    self.collect_nodes(arg);
                }
            }
            ExprKind::Binary { left, right, .. } => {
                self.collect_nodes(left);
                self.collect_nodes(right);
            }
            ExprKind::Group(inner) => self.collect_nodes(inner),
            ExprKind::String
            | ExprKind::Number
            | ExprKind::Boolean
            | ExprKind::Null
            | ExprKind::Undefined
            | ExprKind::Identifier(_) => {}
        }
    }
}

pub(crate) fn analyze(source: &str) -> Analysis {
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();

    loop {
        match lexer.next_token() {
            Ok(token) => {
                let done = token.kind == LexTokenKind::Eof;
                tokens.push(token);
                if done {
                    break;
                }
            }
            Err(diag) => {
                let token_refs = to_token_refs(&tokens);
                let mut token_refs = token_refs;
                token_refs.push(TokenRef {
                    kind: C4TokenKind::Unknown,
                    span: Span {
                        start: diag.start,
                        end: diag.end,
                    },
                });
                return Analysis {
                    schema_version: super::shell2_qjs_c4_contract::version(),
                    hint: ResultHint::Unknown,
                    symbols: Vec::new(),
                    tokens: token_refs,
                    nodes: Vec::new(),
                    diagnostic: Some(diag),
                };
            }
        }
    }

    let token_refs = to_token_refs(&tokens);

    let mut parser = Parser::new(source, tokens);
    match parser.parse_program() {
        Ok(hint) => Analysis {
            schema_version: super::shell2_qjs_c4_contract::version(),
            hint,
            symbols: parser.symbols,
            tokens: token_refs,
            nodes: parser.nodes,
            diagnostic: None,
        },
        Err(diag) => Analysis {
            schema_version: super::shell2_qjs_c4_contract::version(),
            hint: ResultHint::Unknown,
            symbols: parser.symbols,
            tokens: token_refs,
            nodes: parser.nodes,
            diagnostic: Some(diag),
        },
    }
}

fn to_token_kind(kind: LexTokenKind) -> C4TokenKind {
    match kind {
        LexTokenKind::Ident => C4TokenKind::Ident,
        LexTokenKind::String => C4TokenKind::String,
        LexTokenKind::Number => C4TokenKind::Number,
        LexTokenKind::True => C4TokenKind::True,
        LexTokenKind::False => C4TokenKind::False,
        LexTokenKind::Null => C4TokenKind::Null,
        LexTokenKind::Undefined => C4TokenKind::Undefined,
        LexTokenKind::Function => C4TokenKind::Function,
        LexTokenKind::Async => C4TokenKind::Async,
        LexTokenKind::Let => C4TokenKind::Let,
        LexTokenKind::Const => C4TokenKind::Const,
        LexTokenKind::Var => C4TokenKind::Var,
        LexTokenKind::LParen => C4TokenKind::LParen,
        LexTokenKind::RParen => C4TokenKind::RParen,
        LexTokenKind::LBrace => C4TokenKind::LBrace,
        LexTokenKind::RBrace => C4TokenKind::RBrace,
        LexTokenKind::LBracket => C4TokenKind::LBracket,
        LexTokenKind::RBracket => C4TokenKind::RBracket,
        LexTokenKind::Dot => C4TokenKind::Dot,
        LexTokenKind::Comma => C4TokenKind::Comma,
        LexTokenKind::Colon => C4TokenKind::Colon,
        LexTokenKind::Semi => C4TokenKind::Semi,
        LexTokenKind::Eq => C4TokenKind::Assign,
        LexTokenKind::EqEq => C4TokenKind::Eq,
        LexTokenKind::EqEqEq => C4TokenKind::StrictEq,
        LexTokenKind::Arrow => C4TokenKind::Arrow,
        LexTokenKind::Plus => C4TokenKind::Plus,
        LexTokenKind::Minus => C4TokenKind::Minus,
        LexTokenKind::Star => C4TokenKind::Star,
        LexTokenKind::Slash => C4TokenKind::Slash,
        LexTokenKind::Eof => C4TokenKind::Eof,
    }
}

fn to_token_refs(tokens: &[Token]) -> Vec<TokenRef> {
    let mut out = Vec::new();
    for token in tokens {
        out.push(TokenRef {
            kind: to_token_kind(token.kind),
            span: Span {
                start: token.start,
                end: token.end,
            },
        });
    }
    out
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
    if is_special_number_text(trimmed) || is_bigint_text(trimmed) {
        return ResultHint::Number;
    }
    if trimmed.starts_with("function") || trimmed.contains("=>") {
        return ResultHint::Function;
    }
    if trimmed.starts_with("async function") || trimmed.starts_with("async (") {
        return ResultHint::Function;
    }
    if trimmed.starts_with('{') || trimmed.starts_with('[') {
        return ResultHint::Object;
    }
    ResultHint::Unknown
}

fn style_punct(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_PUNCT).dim())
}

fn style_string(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_STRING))
}

fn style_key(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_KEY))
}

fn style_number(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_NUMBER))
}

fn style_boolish(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_BOOLISH).bold())
}

fn style_function(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_FUNCTION))
}

fn style_function_name(text: &str) -> String {
    alloc::format!("{}", ecma48::style(text).fg(COLOR_FUNCTION).bold())
}

fn is_special_number_text(text: &str) -> bool {
    matches!(text, "NaN" | "Infinity" | "-Infinity")
}

fn is_bigint_text(text: &str) -> bool {
    let Some(number) = text.strip_suffix('n') else {
        return false;
    };

    !number.is_empty()
        && number != "-"
        && number.bytes().all(|b| {
            matches!(
                b,
                b'0'..=b'9'
                    | b'_'
                    | b'x'
                    | b'X'
                    | b'o'
                    | b'O'
                    | b'b'
                    | b'B'
                    | b'a'..=b'f'
                    | b'A'..=b'F'
                    | b'-'
            )
        })
}

fn token_text<'a>(text: &'a str, span: &Span) -> Option<&'a str> {
    if span.start > span.end || span.end > text.len() {
        return None;
    }
    if !text.is_char_boundary(span.start) || !text.is_char_boundary(span.end) {
        return None;
    }
    Some(&text[span.start..span.end])
}

fn prev_token(tokens: &[TokenRef], idx: usize) -> Option<&TokenRef> {
    tokens[..idx]
        .iter()
        .rev()
        .find(|token| token.kind != C4TokenKind::Eof)
}

fn next_token(tokens: &[TokenRef], idx: usize) -> Option<&TokenRef> {
    tokens[idx + 1..]
        .iter()
        .find(|token| token.kind != C4TokenKind::Eof)
}

fn token_has_covering_node(analysis: &Analysis, idx: usize, kind: ExprNodeKind) -> bool {
    let span = &analysis.tokens[idx].span;
    analysis
        .nodes
        .iter()
        .any(|node| node.kind == kind && node.span.start <= span.start && node.span.end >= span.end)
}

fn style_token_segment(text: &str, analysis: &Analysis, idx: usize) -> Option<String> {
    let tokens = &analysis.tokens;
    let token = &tokens[idx];
    let raw = token_text(text, &token.span)?;
    let prev = prev_token(tokens, idx);
    let next = next_token(tokens, idx);

    Some(match token.kind {
        C4TokenKind::String => {
            if matches!(next.map(|token| token.kind), Some(C4TokenKind::Colon)) {
                style_key(raw)
            } else {
                style_string(raw)
            }
        }
        C4TokenKind::Number => style_number(raw),
        C4TokenKind::True | C4TokenKind::False | C4TokenKind::Null | C4TokenKind::Undefined => {
            style_boolish(raw)
        }
        C4TokenKind::Function | C4TokenKind::Async => style_function(raw),
        C4TokenKind::Arrow => style_function(raw),
        C4TokenKind::Ident => {
            if matches!(next.map(|token| token.kind), Some(C4TokenKind::Colon)) {
                style_key(raw)
            } else if matches!(prev.map(|token| token.kind), Some(C4TokenKind::Function))
                && matches!(next.map(|token| token.kind), Some(C4TokenKind::LParen))
            {
                style_function_name(raw)
            } else if matches!(prev.map(|token| token.kind), Some(C4TokenKind::Dot))
                || token_has_covering_node(analysis, idx, ExprNodeKind::MemberAccess)
            {
                alloc::format!("{}", ecma48::style(raw).fg(COLOR_OBJECTISH))
            } else {
                String::from(raw)
            }
        }
        C4TokenKind::LParen
        | C4TokenKind::RParen
        | C4TokenKind::LBrace
        | C4TokenKind::RBrace
        | C4TokenKind::LBracket
        | C4TokenKind::RBracket
        | C4TokenKind::Dot
        | C4TokenKind::Comma
        | C4TokenKind::Colon
        | C4TokenKind::Semi
        | C4TokenKind::Assign
        | C4TokenKind::Eq
        | C4TokenKind::StrictEq
        | C4TokenKind::Plus
        | C4TokenKind::Minus
        | C4TokenKind::Star
        | C4TokenKind::Slash => style_punct(raw),
        C4TokenKind::Let | C4TokenKind::Const | C4TokenKind::Var => {
            alloc::format!("{}", ecma48::style(raw).fg(COLOR_OBJECTISH).bold())
        }
        C4TokenKind::Eof | C4TokenKind::Unknown => String::from(raw),
    })
}

fn format_contract_tokens(text: &str, analysis: &Analysis) -> Result<String, FormatFallback> {
    if text.len() > PRETTY_MAX_BYTES {
        return Err(FormatFallback::LimitReached);
    }
    if analysis.schema_version != super::shell2_qjs_c4_contract::version() {
        return Err(FormatFallback::Unsupported);
    }

    let mut out = String::new();
    let mut cursor = 0usize;
    let mut tokens = 0usize;

    for idx in 0..analysis.tokens.len() {
        let token = &analysis.tokens[idx];
        if token.kind == C4TokenKind::Eof {
            continue;
        }
        if tokens >= PRETTY_MAX_TOKENS {
            return Err(FormatFallback::LimitReached);
        }
        if token.span.start < cursor {
            return Err(FormatFallback::Unsupported);
        }
        let raw = token_text(text, &token.span).ok_or(FormatFallback::Unsupported)?;
        out.push_str(&text[cursor..token.span.start]);
        out.push_str(
            style_token_segment(text, analysis, idx)
                .as_deref()
                .unwrap_or(raw),
        );
        cursor = token.span.end;
        tokens += 1;
    }

    out.push_str(&text[cursor..]);
    if tokens == 0 {
        return Err(FormatFallback::Unsupported);
    }

    Ok(out)
}

pub(crate) fn format_js_value_pretty(
    text: &str,
    hint: ResultHint,
) -> Result<String, FormatFallback> {
    let kind = hinted_or_text_kind(hint, text);
    if text.len() > PRETTY_MAX_BYTES {
        return Err(FormatFallback::LimitReached);
    }

    match kind {
        ResultHint::String => Ok(style_string(text)),
        ResultHint::Number => Ok(style_number(text)),
        ResultHint::Boolean | ResultHint::Null | ResultHint::Undefined => Ok(style_boolish(text)),
        ResultHint::Function | ResultHint::Object => format_contract_tokens(text, &analyze(text)),
        ResultHint::Unknown => Err(FormatFallback::Unsupported),
    }
}
