#![no_std]

extern crate alloc;

pub mod ast;
pub mod lexer;
pub mod parser;

pub use ast::{AssignKind, Expr, ExprKind, Program, Stmt, StmtKind, Symbol, Type};
pub use lexer::{LexError, Lexer, Span, Token, TokenKind};
pub use parser::{ParseError, Parser};

pub fn parse_program(source: &str) -> Result<Program, ParseError> {
    Parser::new(source)?.parse_program()
}
