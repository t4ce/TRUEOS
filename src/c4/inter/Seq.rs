use super::{Node, NodeData, StatementNode, Stmt, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Seq {
    stmt: StmtData,
    stmt1: Stmt,
    stmt2: Stmt,
}

impl Seq {
    pub fn new(stmt1: Stmt, stmt2: Stmt) -> Self {
        Self {
            stmt: StmtData::new(),
            stmt1,
            stmt2,
        }
    }

    pub fn get_stmt1(&self) -> &Stmt {
        &self.stmt1
    }
    pub fn get_stmt2(&self) -> &Stmt {
        &self.stmt2
    }
}

impl Node for Seq {
    fn node_data(&self) -> &NodeData {
        &self.stmt.node
    }
}

impl StatementNode for Seq {
    fn stmt_data(&self) -> &StmtData {
        &self.stmt
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.stmt
    }
}

impl<P, R> Walk<P, R> for Seq {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_seq_node(self, arg)
    }
}
