use super::{
    DreiAdrCode, Expr, ExprData, ExpressionNode, Id, Node, NodeData, OpData, Token, TreeWalker,
    Type, Walk,
};

/// Binary arithmetic expression.
#[derive(Clone, Debug, PartialEq)]
pub struct Arith {
    op: OpData,
    expr1: Expr,
    expr2: Expr,
}

impl Arith {
    pub fn new(token: Token, expr1: Expr, expr2: Expr) -> Self {
        Self {
            op: OpData::new(token),
            expr1,
            expr2,
        }
    }

    pub fn with_type(token: Token, expr1: Expr, expr2: Expr, ty: Type) -> Self {
        let mut arith = Self::new(token, expr1, expr2);
        arith.set_type(ty);
        arith
    }

    pub fn get_expr1(&self) -> &Expr {
        &self.expr1
    }
    pub fn get_expr2(&self) -> &Expr {
        &self.expr2
    }
    pub fn set_expr1(&mut self, expr: Expr) {
        self.expr1 = expr;
    }
    pub fn set_expr2(&mut self, expr: Expr) {
        self.expr2 = expr;
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        let left = self
            .expr1
            .as_singleton()
            .expect("left operand must be lowered to a Singleton");
        let right = self
            .expr2
            .as_singleton()
            .expect("right operand must be lowered to a Singleton");
        DreiAdrCode::Binary {
            target,
            left,
            operator: self.get_op().clone(),
            right,
        }
    }
}

impl Node for Arith {
    fn node_data(&self) -> &NodeData {
        &self.op.expr.node
    }
}

impl ExpressionNode for Arith {
    fn expr_data(&self) -> &ExprData {
        &self.op.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.op.expr
    }
}

impl<P, R> Walk<P, R> for Arith {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_arith_node(self, arg)
    }
}
