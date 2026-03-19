extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use crate::lexer::Span;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Char,
    Bool,
    Array { len: usize, of: Box<Type> },
}

impl Type {
    pub fn width(&self) -> usize {
        match self {
            Self::Int => 4,
            Self::Float => 8,
            Self::Char => 1,
            Self::Bool => 1,
            Self::Array { len, of } => len.saturating_mul(of.width()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    pub ty: Type,
    pub offset: usize,
    pub declared_at: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub block: Stmt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    Empty,
    Block(Vec<Stmt>),
    If {
        condition: Expr,
        then_branch: Box<Stmt>,
    },
    IfElse {
        condition: Expr,
        then_branch: Box<Stmt>,
        else_branch: Box<Stmt>,
    },
    While {
        condition: Expr,
        body: Box<Stmt>,
    },
    DoWhile {
        body: Box<Stmt>,
        condition: Expr,
    },
    For {
        init: AssignKind,
        condition: Expr,
        step: AssignKind,
        body: Box<Stmt>,
    },
    Break,
    Assign(AssignKind),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AssignKind {
    Var { target: Symbol, value: Expr },
    Index { target: Expr, value: Expr },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: Type,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Id(Symbol),
    Int(i64),
    Float(String),
    Bool(bool),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
    And,
    Or,
}
