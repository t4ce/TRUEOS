//! Rust port of the legacy `inter` Java package.
//!
//! The legacy package used class inheritance.  This port uses small data
//! structs for the former base classes and enums for polymorphic values, while
//! retaining the original node names and visitor dispatch points.

#[path = "Access.rs"]
mod access;
#[path = "And.rs"]
mod and;
#[path = "Arith.rs"]
mod arith;
#[path = "AssignElem.rs"]
mod assign_elem;
#[path = "AssignId.rs"]
mod assign_id;
#[path = "AssignStmt.rs"]
mod assign_stmt;
#[path = "Assignment.rs"]
mod assignment;
#[path = "Block.rs"]
mod block;
#[path = "Break.rs"]
mod break_stmt;
#[path = "Constant.rs"]
mod constant;
#[path = "Do.rs"]
mod do_stmt;
#[path = "Else.rs"]
mod else_stmt;
#[path = "EmptyStmt.rs"]
mod empty_stmt;
#[path = "Expr.rs"]
mod expr;
#[path = "For.rs"]
mod for_stmt;
#[path = "Id.rs"]
mod id;
#[path = "If.rs"]
mod if_stmt;
#[path = "Logical.rs"]
mod logical;
#[path = "Node.rs"]
mod node;
#[path = "Not.rs"]
mod not;
#[path = "Op.rs"]
mod op;
#[path = "Or.rs"]
mod or;
#[path = "Program.rs"]
mod program;
#[path = "Rel.rs"]
mod rel;
#[path = "Seq.rs"]
mod seq;
#[path = "Singleton.rs"]
mod singleton;
#[path = "Stmt.rs"]
mod stmt;
#[path = "Temp.rs"]
mod temp;
#[path = "Unary.rs"]
mod unary;
#[path = "While.rs"]
mod while_stmt;

pub use access::Access;
pub use and::And;
pub use arith::Arith;
pub use assign_elem::AssignElem;
pub use assign_id::AssignId;
pub use assign_stmt::AssignStmt;
pub use assignment::{Assignment, AssignmentData, AssignmentNode};
pub use block::Block;
pub use break_stmt::Break;
pub use constant::{Constant, FALSE, TRUE};
pub use do_stmt::Do;
pub use else_stmt::Else;
pub use empty_stmt::{EmptyStmt, NULL};
pub use expr::{Expr, ExprData, ExpressionNode};
pub use for_stmt::For;
pub use id::Id;
pub use if_stmt::If;
pub use logical::LogicalData;
pub use node::{lexer_line, set_lexer_line, Node, NodeData, TreeWalker, Walk};
pub use not::Not;
pub use op::OpData;
pub use or::Or;
pub use program::Program;
pub use rel::Rel;
pub use seq::Seq;
pub use singleton::{Singleton, SingletonData};
pub use stmt::{StatementNode, Stmt, StmtData};
pub use temp::Temp;
pub use unary::Unary;
pub use while_stmt::While;

use std::fmt;

/// Token categories needed by the legacy intermediate tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tag {
    Index,
    Num,
    True,
    False,
    Temp,
    Other(String),
}

/// The token payload consumed by expression nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub lexeme: String,
    pub tag: Tag,
}

impl Token {
    pub fn new(lexeme: impl Into<String>, tag: Tag) -> Self {
        Self {
            lexeme: lexeme.into(),
            tag,
        }
    }

    pub fn integer(value: i32) -> Self {
        Self::new(value.to_string(), Tag::Num)
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.lexeme)
    }
}

/// C4 value types. `None` on an expression corresponds to Java's `null` type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Type {
    Int,
    Float,
    Char,
    Bool,
    Array { len: usize, element: Box<Type> },
}

impl Type {
    pub fn width(&self) -> usize {
        match self {
            Self::Int => 4,
            Self::Float => 8,
            Self::Char | Self::Bool => 1,
            Self::Array { len, element } => len.saturating_mul(element.width()),
        }
    }
}

/// Rust representation of the four three-address instructions emitted here.
#[derive(Clone, Debug, PartialEq)]
pub enum DreiAdrCode {
    Assign {
        target: Id,
        value: Singleton,
    },
    Unary {
        target: Id,
        operator: Token,
        value: Singleton,
    },
    Binary {
        target: Id,
        left: Singleton,
        operator: Token,
        right: Singleton,
    },
    ArrayRef {
        target: Id,
        array: Id,
        index: Singleton,
    },
}

impl fmt::Display for DreiAdrCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Assign { target, value } => write!(f, "{target} = {value}"),
            Self::Unary {
                target,
                operator,
                value,
            } => {
                write!(f, "{target} = {operator} {value}")
            }
            Self::Binary {
                target,
                left,
                operator,
                right,
            } => {
                write!(f, "{target} = {left} {operator} {right}")
            }
            Self::ArrayRef {
                target,
                array,
                index,
            } => {
                write!(f, "{target} = {array}[{index}]")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn singleton_assignment_matches_legacy_text() {
        let target = Id::new(Token::new("answer", Tag::Other("id".into())), Type::Int, 4);
        let value = Constant::from_int(42);
        let code = value.code_for_value_to(target);
        assert_eq!(code.to_string(), "answer = 42");
    }

    #[test]
    fn temporary_counter_can_be_reset() {
        Temp::reset_counter();
        assert_eq!(Temp::new(Type::Int).to_string(), "t1");
        assert_eq!(Temp::new(Type::Int).to_string(), "t2");
        Temp::reset_counter();
        assert_eq!(Temp::new(Type::Int).to_string(), "t1");
    }
}
