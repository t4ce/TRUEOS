#![no_std]
//! C4 is the inherited teaching compiler: a compact academic compiler from a
//! professor's lecture material, carried forward as a TRUEOS-native compiler
//! path.
//!
//! TRUEOS turns it from lecture artifact into a living in-kernel compiler
//! service. This crate is the Rust-side C4 front end that can grow toward a
//! C4-to-Blueprint backend.

extern crate alloc;

pub mod ast;
pub mod backend_rust;
pub mod backend_vm;
pub mod lexer;
pub mod parser;

pub use ast::{AssignKind, Expr, ExprKind, Program, Stmt, StmtKind, Symbol, Type};
pub use backend_rust::emit_rust;
pub use backend_vm::{
    VmLocalReport, VmObject, VmObjectError, VmRunError, VmRunReport, VmValue, emit_vm_object,
    run_vm_object,
};
pub use lexer::{LexError, Lexer, Span, Token, TokenKind};
pub use parser::{ParseError, Parser};

pub fn parse_program(source: &str) -> Result<Program, ParseError> {
    Parser::new(source)?.parse_program()
}
