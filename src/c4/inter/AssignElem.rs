use super::{Access, AssignmentData, AssignmentNode, Expr, Node, NodeData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq)]
pub struct AssignElem {
    assignment: AssignmentData,
    access: Access,
    expr: Expr,
}

impl AssignElem {
    pub fn new(access: Access, expr: Expr) -> Self {
        Self {
            assignment: AssignmentData::new(),
            access,
            expr,
        }
    }

    pub fn get_acc(&self) -> &Access {
        &self.access
    }
    pub fn set_acc(&mut self, access: Access) {
        self.access = access;
    }
    pub fn get_expr(&self) -> &Expr {
        &self.expr
    }
    pub fn set_expr(&mut self, expr: Expr) {
        self.expr = expr;
    }
}

impl Node for AssignElem {
    fn node_data(&self) -> &NodeData {
        &self.assignment.node
    }
}

impl AssignmentNode for AssignElem {
    fn assignment_data(&self) -> &AssignmentData {
        &self.assignment
    }
}

impl<P, R> Walk<P, R> for AssignElem {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_assign_elem_node(self, arg)
    }
}
