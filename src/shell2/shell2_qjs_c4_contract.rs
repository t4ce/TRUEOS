use alloc::string::String;
use alloc::vec::Vec;

pub(crate) const C4_CORE_CONTRACT_VERSION: u16 = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
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
    Assign,
    Eq,
    StrictEq,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Eof,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExprNodeKind {
    StringLiteral,
    NumberLiteral,
    BooleanLiteral,
    NullLiteral,
    UndefinedLiteral,
    Identifier,
    MemberAccess,
    Call,
    Binary,
    Group,
    Declaration,
    Assignment,
    Program,
}

#[derive(Clone)]
pub(crate) struct Span {
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone)]
pub(crate) struct TokenRef {
    pub(crate) kind: TokenKind,
    pub(crate) span: Span,
}

#[derive(Clone)]
pub(crate) struct NodeRef {
    pub(crate) kind: ExprNodeKind,
    pub(crate) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResultHint {
    String,
    Number,
    Boolean,
    Null,
    Undefined,
    Function,
    Object,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SymbolRole {
    Decl,
    Assign,
    Read,
    Call,
}

#[derive(Clone)]
pub(crate) struct SymbolRef {
    pub(crate) name: String,
    pub(crate) role: SymbolRole,
}

#[derive(Clone)]
pub(crate) struct Diagnostic {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

#[derive(Clone)]
pub(crate) struct Analysis {
    pub(crate) schema_version: u16,
    pub(crate) hint: ResultHint,
    pub(crate) symbols: Vec<SymbolRef>,
    pub(crate) tokens: Vec<TokenRef>,
    pub(crate) nodes: Vec<NodeRef>,
    pub(crate) diagnostic: Option<Diagnostic>,
}

pub(crate) const fn version() -> u16 {
    C4_CORE_CONTRACT_VERSION
}
