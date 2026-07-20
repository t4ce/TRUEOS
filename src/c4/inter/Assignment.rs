use super::{AssignElem, AssignId, Node, NodeData, TreeWalker, Walk};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssignmentData {
    pub(crate) node: NodeData,
}

impl AssignmentData {
    pub fn new() -> Self {
        Self {
            node: NodeData::new(),
        }
    }
}

impl Default for AssignmentData {
    fn default() -> Self {
        Self::new()
    }
}

pub trait AssignmentNode: Node {
    fn assignment_data(&self) -> &AssignmentData;
}

#[derive(Clone, Debug, PartialEq)]
pub enum Assignment {
    Elem(Box<AssignElem>),
    Id(Box<AssignId>),
}

impl From<AssignElem> for Assignment {
    fn from(value: AssignElem) -> Self {
        Self::Elem(Box::new(value))
    }
}

impl From<AssignId> for Assignment {
    fn from(value: AssignId) -> Self {
        Self::Id(Box::new(value))
    }
}

impl Node for Assignment {
    fn node_data(&self) -> &NodeData {
        match self {
            Self::Elem(node) => node.node_data(),
            Self::Id(node) => node.node_data(),
        }
    }
}

impl AssignmentNode for Assignment {
    fn assignment_data(&self) -> &AssignmentData {
        match self {
            Self::Elem(node) => node.assignment_data(),
            Self::Id(node) => node.assignment_data(),
        }
    }
}

impl<P, R> Walk<P, R> for Assignment {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        match self {
            Self::Elem(node) => node.walk(walker, arg),
            Self::Id(node) => node.walk(walker, arg),
        }
    }
}
