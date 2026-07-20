use super::{Expr, Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Else {
    data: StmtData,
    expr: Expr,
    stmt1: Stmt,
    stmt2: Stmt,
}

impl Else {
    pub fn new(expr: Expr, stmt1: Stmt, stmt2: Stmt) -> Self {
        Self {
            data: StmtData::new(),
            expr,
            stmt1,
            stmt2,
        }
    }

    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }
    pub fn get_stmt1(&self) -> &Stmt {
        &self.stmt1
    }
    pub fn get_stmt2(&self) -> &Stmt {
        &self.stmt2
    }
}

impl Node for Else {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for Else {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for Else {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_else_node(self, arg)
    }
}
