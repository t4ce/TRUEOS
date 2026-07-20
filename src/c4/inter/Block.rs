use super::{Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    stmt: StmtData,
    stmts: Stmt,
}

impl Block {
    pub fn new(stmts: Stmt) -> Self {
        Self {
            stmt: StmtData::new(),
            stmts,
        }
    }
    pub fn get_stmts(&self) -> &Stmt {
        &self.stmts
    }
}

impl Node for Block {
    fn node_data(&self) -> &NodeData {
        &self.stmt.node
    }
}

impl StatementNode for Block {
    fn stmt_data(&self) -> &StmtData {
        &self.stmt
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.stmt
    }
}

impl<P, R> Walk<P, R> for Block {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_block_node(self, arg)
    }
}
