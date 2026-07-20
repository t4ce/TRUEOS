use super::{Block, Node, NodeData, StatementNode, StmtData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    stmt: StmtData,
    block: Block,
}

impl Program {
    pub fn new(block: Block) -> Self {
        Self {
            stmt: StmtData::new(),
            block,
        }
    }
    pub fn get_block(&self) -> &Block {
        &self.block
    }
}

impl Node for Program {
    fn node_data(&self) -> &NodeData {
        &self.stmt.node
    }
}

impl StatementNode for Program {
    fn stmt_data(&self) -> &StmtData {
        &self.stmt
    }
    fn stmt_data_mut(&mut self) -> &mut StmtData {
        &mut self.stmt
    }
}

impl<P, R> Walk<P, R> for Program {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_program_node(self, arg)
    }
}
