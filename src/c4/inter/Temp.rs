use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    DreiAdrCode, ExprData, ExpressionNode, Id, Node, NodeData, Singleton, Tag, Token, TreeWalker,
    Type, Walk,
};

static COUNT: AtomicUsize = AtomicUsize::new(0);

#[derive(Clone, Debug, PartialEq)]
pub struct Temp {
    id: Id,
    number: usize,
}

impl Temp {
    pub fn new(ty: Type) -> Self {
        let number = COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        Self {
            id: Id::new(Token::new("temp", Tag::Temp), ty, 0),
            number,
        }
    }

    pub fn get_number(&self) -> usize {
        self.number
    }
    pub fn get_offset(&self) -> i32 {
        self.id.get_offset()
    }
    pub fn as_id(&self) -> &Id {
        &self.id
    }

    pub fn code_for_value_to(&self, target: Id) -> DreiAdrCode {
        DreiAdrCode::Assign {
            target,
            value: Singleton::Temp(self.clone()),
        }
    }

    pub fn reset_counter() {
        COUNT.store(0, Ordering::Relaxed);
    }
}

impl Node for Temp {
    fn node_data(&self) -> &NodeData {
        self.id.node_data()
    }
}

impl ExpressionNode for Temp {
    fn expr_data(&self) -> &ExprData {
        self.id.expr_data()
    }
    fn expr_data_mut(&mut self) -> &mut ExprData {
        self.id.expr_data_mut()
    }
    fn is_singleton(&self) -> bool {
        true
    }
}

// Java Temp inherits Id.walk(), so visitor dispatch remains walk_id_node.
impl<P, R> Walk<P, R> for Temp {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        walker.walk_id_node(&self.id, arg)
    }
}

impl fmt::Display for Temp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "t{}", self.number)
    }
}
