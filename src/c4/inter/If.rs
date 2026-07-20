use super::{Expr, Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct If {
    data: StmtData,
    expr: Expr,
    stmt: Stmt,
}

impl If {
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

impl Node for If {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for If {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for If {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_if_node(self, arg)
    }
}
