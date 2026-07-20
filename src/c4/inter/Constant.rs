use std::fmt;
use std::sync::LazyLock;

use super::{
    DreiAdrCode, ExprData, ExpressionNode, Id, Node, NodeData, Singleton, SingletonData, Tag,
    Token, TreeWalker, Type, Walk,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Constant {
    singleton: SingletonData,
}

impl Constant {
    pub fn new(token: Token, ty: Type) -> Self {
        Self {
            singleton: SingletonData::new(token, ty),
        }
    }

    pub fn from_int(value: i32) -> Self {
        Self::new(Token::integer(value), Type::Int)
    }
    pub fn is_constant(&self) -> bool {
        true
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        DreiAdrCode::Assign {
            target,
            value: Singleton::Constant(self.clone()),
        }
    }
}

pub static TRUE: LazyLock<Constant> =
    LazyLock::new(|| Constant::new(Token::new("true", Tag::True), Type::Bool));
pub static FALSE: LazyLock<Constant> =
    LazyLock::new(|| Constant::new(Token::new("false", Tag::False), Type::Bool));

impl Node for Constant {
    fn node_data(&self) -> &NodeData {
        &self.singleton.expr.node
    }
}

impl ExpressionNode for Constant {
    fn expr_data(&self) -> &ExprData {
        &self.singleton.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.singleton.expr
    }
    fn is_singleton(&self) -> bool {
        true
    }
    fn is_constant(&self) -> bool {
        true
    }
}

impl<P, R> Walk<P, R> for Constant {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_constant_node(self, arg)
    }
}

impl fmt::Display for Constant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.get_op(), f)
    }
}
