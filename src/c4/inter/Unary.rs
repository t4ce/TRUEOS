use super::{
    DreiAdrCode, Expr, ExprData, ExpressionNode, Id, Node, NodeData, OpData, Token, TreeWalker,
    Walk,
};

/// Unary arithmetic expression. Logical negation is represented by `Not`.
#[derive(Clone, Debug, PartialEq)]
pub struct Unary {
    op: OpData,
    expr: Expr,
}

impl Unary {
    pub fn new(token: Token, expr: Expr) -> Self {
        Self {
            op: OpData::new(token),
            expr,
        }
    }

    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        let value = self
            .expr
            .as_singleton()
            .expect("unary operand must be lowered to a Singleton");
        DreiAdrCode::Unary {
            target,
            operator: self.get_op().clone(),
            value,
        }
    }
}

impl Node for Unary {
    fn node_data(&self) -> &NodeData {
        &self.op.expr.node
    }
}

impl ExpressionNode for Unary {
    fn expr_data(&self) -> &ExprData {
        &self.op.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.op.expr
    }
}

impl<P, R> Walk<P, R> for Unary {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_unary_node(self, arg)
    }
}
