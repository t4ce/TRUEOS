use super::{
    DreiAdrCode, Expr, ExprData, ExpressionNode, Id, Node, NodeData, OpData, Tag, Token,
    TreeWalker, Type, Walk,
};

/// Array access expression (`array[index]`).
#[derive(Clone, Debug, PartialEq)]
pub struct Access {
    op: OpData,
    array: Expr,
    index: Expr,
}

impl Access {
    pub fn new(array: Expr, index: Expr) -> Self {
        Self {
            op: OpData::new(Token::new("[]", Tag::Index)),
            array,
            index,
        }
    }

    pub fn with_type(array: Expr, index: Expr, ty: Type) -> Self {
        let mut access = Self::new(array, index);
        access.set_type(ty);
        access
    }

    pub fn get_array(&self) -> &Expr {
        &self.array
    }
    pub fn get_index(&self) -> &Expr {
        &self.index
    }
    pub fn set_index(&mut self, index: Expr) {
        self.index = index;
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        let array = self
            .array
            .as_id()
            .expect("array access must be lowered to an Id");
        let index = self
            .index
            .as_singleton()
            .expect("array index must be lowered to a Singleton");
        DreiAdrCode::ArrayRef {
            target,
            array,
            index,
        }
    }
}

impl Node for Access {
    fn node_data(&self) -> &NodeData {
        &self.op.expr.node
    }
}

impl ExpressionNode for Access {
    fn expr_data(&self) -> &ExprData {
        &self.op.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.op.expr
    }
}

impl<P, R> Walk<P, R> for Access {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_access_node(self, arg)
    }
}
