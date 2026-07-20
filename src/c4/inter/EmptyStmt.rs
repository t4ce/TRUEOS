use std::sync::LazyLock;

use super::{Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct EmptyStmt {
    stmt: StmtData,
}

impl EmptyStmt {
    pub fn new() -> Self {
        Self {
            stmt: StmtData::new(),
        }
    }
}

impl Default for EmptyStmt {
    fn default() -> Self {
        Self::new()
    }
}

pub static NULL: LazyLock<Stmt> = LazyLock::new(|| EmptyStmt::new().into());

impl Node for EmptyStmt {
    fn node_data(&self) -> &NodeData {
        &self.stmt.node
    }
}

impl StatementNode for EmptyStmt {
    fn stmt_data(&self) -> &StmtData {
        &self.stmt
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.stmt
    }
}

impl<P, R> Walk<P, R> for EmptyStmt {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_empty_stmt_node(self, arg)
    }
}
