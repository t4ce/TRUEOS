use super::{Expr, ExprData, ExpressionNode, LogicalData, Node, NodeData, Token, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Or {
    logical: LogicalData,
}

impl Or {
    pub fn new(token: Token, expr1: Expr, expr2: Expr) -> Self {
        Self {
            logical: LogicalData::new(token, expr1, expr2),
        }
    }

    pub fn get_expr1(&self) -> &Expr {
        self.logical.get_expr1()
    }
    pub fn get_expr2(&self) -> &Expr {
        self.logical.get_expr2()
    }
    pub fn set_expr1(&mut self, expr: Expr) {
        self.logical.set_expr1(expr);
    }
    pub fn set_expr2(&mut self, expr: Expr) {
        self.logical.set_expr2(expr);
    }
}

impl Node for Or {
    fn node_data(&self) -> &NodeData {
        &self.logical.expr.node
    }
}

impl ExpressionNode for Or {
    fn expr_data(&self) -> &ExprData {
        &self.logical.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.logical.expr
    }
}

impl<P, R> Walk<P, R> for Or {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_or_node(self, arg)
    }
}
