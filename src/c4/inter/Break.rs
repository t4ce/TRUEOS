use super::{Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Break {
    data: StmtData,
    stmt: Option<Stmt>,
}

impl Break {
    pub fn new() -> Self {
        Self {
            data: StmtData::new(),
            stmt: None,
        }
    }
    pub fn get_stmt(&self) -> Option<&Stmt> {
        self.stmt.as_ref()
    }
    pub fn set_stmt(&mut self, stmt: Stmt) {
        self.stmt = Some(stmt);
    }
}

impl Default for Break {
    fn default() -> Self {
        Self::new()
    }
}

impl Node for Break {
    fn node_data(&self) -> &NodeData {
        &self.data.node
    }
}

impl StatementNode for Break {
    fn stmt_data(&self) -> &StmtData {
        &self.data
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.data
    }
}

impl<P, R> Walk<P, R> for Break {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_break_node(self, arg)
    }
}
