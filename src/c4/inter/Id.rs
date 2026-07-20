use std::fmt;

use super::{
    DreiAdrCode, ExprData, ExpressionNode, Node, NodeData, Singleton, SingletonData, Token,
    TreeWalker, Type, Walk,
};

#[derive(Clone, Debug, PartialEq)]
pub struct Id {
    singleton: SingletonData,
    offset: i32,
}

impl Id {
    pub fn new(word: Token, ty: Type, offset: i32) -> Self {
        Self {
            singleton: SingletonData::new(word, ty),
            offset,
        }
    }

    pub fn get_offset(&self) -> i32 {
        self.offset
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        DreiAdrCode::Assign {
            target,
            value: Singleton::Id(self.clone()),
        }
    }
}

impl Node for Id {
    fn node_data(&self) -> &NodeData {
        &self.singleton.expr.node
    }
}

impl ExpressionNode for Id {
    fn expr_data(&self) -> &ExprData {
        &self.singleton.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.singleton.expr
    }
    fn is_singleton(&self) -> bool {
        true
    }
}

impl<P, R> Walk<P, R> for Id {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_id_node(self, arg)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.get_op(), f)
    }
}
