use super::{Expr, ExprData, ExpressionNode, LogicalData, Node, NodeData, Token, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Not {
    logical: LogicalData,
}

impl Not {
    pub fn new(token: Token, expr: Expr) -> Self {
        Self {
            logical: LogicalData::new(token, expr.clone(), expr),
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

impl Node for Not {
    fn node_data(&self) -> &NodeData {
        &self.logical.expr.node
    }
}

impl ExpressionNode for Not {
    fn expr_data(&self) -> &ExprData {
        &self.logical.expr
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        &mut self.logical.expr
    }
}

impl<P, R> Walk<P, R> for Not {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_not_node(self, arg)
    }
}
