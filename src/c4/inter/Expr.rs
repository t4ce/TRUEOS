use std::fmt;

use super::{
    Access, And, Arith, Constant, DreiAdrCode, Id, Node, NodeData, Not, Or, Rel, Singleton, Temp,
    Token, TreeWalker, Type, Unary, Walk,
};

/// State inherited by every Java `Expr`.
#[derive(Clone, Debug, PartialEq)]
pub struct ExprData {
    pub(crate) node: NodeData,
    op: Token,
    ty: Option<Type>,
}

impl ExprData {
    pub fn new(op: Token, ty: Option<Type>) -> Self {
        Self {
            node: NodeData::new(),
            op,
            ty,
        }
    }
}

pub trait ExpressionNode: Node {
    fn expr_data(&self) -> &ExprData;
    fn expr_data_mut(&mut self) -> &mut ExprData;

    fn get_op(&self) -> &Token {
        &self.expr_data().op
    }

    fn get_type(&self) -> Option<&Type> {
        self.expr_data().ty.as_ref()
    }

    fn set_op(&mut self, op: Token) {
        self.expr_data_mut().op = op;
    }

    fn set_type(&mut self, ty: Type) {
        self.expr_data_mut().ty = Some(ty);
    }

    fn is_singleton(&self) -> bool {
        false
    }

    fn is_constant(&self) -> bool {
        false
    }
}

/// Polymorphic expression value replacing Java references to abstract `Expr`.
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Access(Box<Access>),
    And(Box<And>),
    Arith(Box<Arith>),
    Constant(Box<Constant>),
    Id(Box<Id>),
    Not(Box<Not>),
    Or(Box<Or>),
    Rel(Box<Rel>),
    Temp(Box<Temp>),
    Unary(Box<Unary>),
}

impl Expr {
    pub fn code_for_value_to(&self, id: Id) -> Option<DreiAdrCode> {
        match self {
            Self::Access(node) => Some(node.code_for_value_to(id)),
            Self::Arith(node) => Some(node.code_for_value_to(id)),
            Self::Constant(node) => Some(node.code_for_value_to(id)),
            Self::Id(node) => Some(node.code_for_value_to(id)),
            Self::Temp(node) => Some(node.code_for_value_to(id)),
            Self::Unary(node) => Some(node.code_for_value_to(id)),
            _ => None,
        }
    }

    pub fn as_singleton(&self) -> Option<Singleton> {
        match self {
            Self::Constant(node) => Some(Singleton::Constant((**node).clone())),
            Self::Id(node) => Some(Singleton::Id((**node).clone())),
            Self::Temp(node) => Some(Singleton::Temp((**node).clone())),
            _ => None,
        }
    }

    pub fn as_id(&self) -> Option<Id> {
        match self {
            Self::Id(node) => Some((**node).clone()),
            Self::Temp(node) => Some(node.as_id().clone()),
            _ => None,
        }
    }
}

macro_rules! impl_from_expr {
    ($variant:ident, $ty:ty) => {
        impl From<$ty> for Expr {
            fn from(value: $ty) -> Self {
                Self::$variant(Box::new(value))
            }
        }
    };
}

impl_from_expr!(Access, Access);
impl_from_expr!(And, And);
impl_from_expr!(Arith, Arith);
impl_from_expr!(Constant, Constant);
impl_from_expr!(Id, Id);
impl_from_expr!(Not, Not);
impl_from_expr!(Or, Or);
impl_from_expr!(Rel, Rel);
impl_from_expr!(Temp, Temp);
impl_from_expr!(Unary, Unary);

impl Node for Expr {
    fn node_data(&self) -> &NodeData {
        match self {
            Self::Access(n) => n.node_data(),
            Self::And(n) => n.node_data(),
            Self::Arith(n) => n.node_data(),
            Self::Constant(n) => n.node_data(),
            Self::Id(n) => n.node_data(),
            Self::Not(n) => n.node_data(),
            Self::Or(n) => n.node_data(),
            Self::Rel(n) => n.node_data(),
            Self::Temp(n) => n.node_data(),
            Self::Unary(n) => n.node_data(),
        }
    }
}

impl ExpressionNode for Expr {
    fn expr_data(&self) -> &ExprData {
        match self {
            Self::Access(n) => n.expr_data(),
            Self::And(n) => n.expr_data(),
            Self::Arith(n) => n.expr_data(),
            Self::Constant(n) => n.expr_data(),
            Self::Id(n) => n.expr_data(),
            Self::Not(n) => n.expr_data(),
            Self::Or(n) => n.expr_data(),
            Self::Rel(n) => n.expr_data(),
            Self::Temp(n) => n.expr_data(),
            Self::Unary(n) => n.expr_data(),
        }
    }

    fn expr_data_mut(&mut self) -> &mut ExprData {
        match self {
            Self::Access(n) => n.expr_data_mut(),
            Self::And(n) => n.expr_data_mut(),
            Self::Arith(n) => n.expr_data_mut(),
            Self::Constant(n) => n.expr_data_mut(),
            Self::Id(n) => n.expr_data_mut(),
            Self::Not(n) => n.expr_data_mut(),
            Self::Or(n) => n.expr_data_mut(),
            Self::Rel(n) => n.expr_data_mut(),
            Self::Temp(n) => n.expr_data_mut(),
            Self::Unary(n) => n.expr_data_mut(),
        }
    }

    fn is_singleton(&self) -> bool {
        matches!(self, Self::Constant(_) | Self::Id(_) | Self::Temp(_))
    }

    fn is_constant(&self) -> bool {
        matches!(self, Self::Constant(_))
    }
}

impl<P, R> Walk<P, R> for Expr {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        match self {
            Self::Access(n) => n.walk(walker, arg),
            Self::And(n) => n.walk(walker, arg),
            Self::Arith(n) => n.walk(walker, arg),
            Self::Constant(n) => n.walk(walker, arg),
            Self::Id(n) => n.walk(walker, arg),
            Self::Not(n) => n.walk(walker, arg),
            Self::Or(n) => n.walk(walker, arg),
            Self::Rel(n) => n.walk(walker, arg),
            Self::Temp(n) => n.walk(walker, arg),
            Self::Unary(n) => n.walk(walker, arg),
        }
    }
}

impl fmt::Display for Expr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Temp(n) => fmt::Display::fmt(n, f),
            _ => fmt::Display::fmt(self.get_op(), f),
        }
    }
}
