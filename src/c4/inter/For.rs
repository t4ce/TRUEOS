use super::{Assignment, Expr, Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct For {
    data: StmtData,
    expr: Expr,
    init_ass: Assignment,
    iter_ass: Assignment,
    stmt: Stmt,
}

impl For {
    pub fn new(init_ass: Assignment, expr: Expr, iter_ass: Assignment, stmt: Stmt) -> Self {
        Self {
            data: StmtData::new(),
            expr,
            init_ass,
            iter_ass,
            stmt,
        }
    }

    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }
    pub fn get_init_ass(&self) -> &Assignment {
        &self.init_ass
    }
    pub fn get_iter_ass(&self) -> &Assignment {
        &self.iter_ass
    }
    pub fn get_stmt(&self) -> &Stmt {
        &self.stmt
    }
}

impl Node for For {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for For {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for For {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_for_node(self, arg)
    }
}
