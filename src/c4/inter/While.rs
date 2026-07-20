use super::{Expr, Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct While {
    data: StmtData,
    expr: Expr,
    stmt: Stmt,
}

impl While {
    pub fn new(expr: Expr, stmt: Stmt) -> Self {
        Self {
            data: StmtData::new(),
            expr,
            stmt,
        }
    }
    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }
    pub fn get_stmt(&self) -> &Stmt {
        &self.stmt
    }
}

impl Node for While {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for While {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for While {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_while_node(self, arg)
    }
}
