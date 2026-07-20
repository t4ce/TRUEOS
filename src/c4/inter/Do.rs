use super::{Expr, Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Do {
    data: StmtData,
    expr: Expr,
    stmt: Stmt,
}

impl Do {
    pub fn new(stmt: Stmt, expr: Expr) -> Self {
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

impl Node for Do {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for Do {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for Do {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_do_node(self, arg)
    }
}
