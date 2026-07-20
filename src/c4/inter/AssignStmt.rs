use super::{Assignment, Node, NodeData, StatementNode, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct AssignStmt {
    stmt: StmtData,
    assign: Assignment,
}

impl AssignStmt {
    pub fn new(assign: Assignment) -> Self {
        Self {
            stmt: StmtData::new(),
            assign,
        }
    }
    pub fn get_assign(&self) -> &Assignment {
        &self.assign
    }
}

impl Node for AssignStmt {
    fn node_data(&self) -> &NodeData {
        &self.stmt.node
    }
}

impl StatementNode for AssignStmt {
    fn stmt_data(&self) -> &StmtData {
        &self.stmt
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.stmt
    }
}

impl<P, R> Walk<P, R> for AssignStmt {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_assign_stmt_node(self, arg)
    }
}
