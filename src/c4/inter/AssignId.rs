use super::{AssignmentData, AssignmentNode, Expr, Id, Node, NodeData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct AssignId {
    assignment: AssignmentData,
    ident: Id,
    expr: Expr,
}

impl AssignId {
    pub fn new(ident: Id, expr: Expr) -> Self {
        Self {
            assignment: AssignmentData::new(),
            ident,
            expr,
        }
    }

    pub fn get_ident(&self) -> &Id {
        &self.ident
    }
    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }
}

impl Node for AssignId {
    fn node_data(&self) -> &NodeData {
        &self.assignment.node
    }
}

impl AssignmentNode for AssignId {
    fn assignment_data(&self) -> &AssignmentData {
        &self.assignment
    }
}

impl<P, R> Walk<P, R> for AssignId {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_assign_id_node(self, arg)
    }
}
