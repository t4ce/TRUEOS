use std::sync::Mutex;

use super::{
    AssignStmt, Block, Break, Do, Else, EmptyStmt, For, If, Node, NodeData, Program, Seq,
    TreeWalker, Walk, While,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StmtData {
    pub(crate) node: NodeData,
    next: usize,
}

impl StmtData {
    pub fn new() -> Self {
        Self {
            node: NodeData::new(),
            next: 0,
        }
    }
}

impl Default for StmtData {
    fn default() -> Self {
        Self::new()
    }
}

pub trait StatementNode: Node {
    fn stmt_data(&self) -> &StmtData;
    fn stmt_data_mut(&mut self) -> &mut StmtData;

    fn get_next(&self) -> usize {
        self.stmt_data().next
    }
    fn set_next(&mut self, next: usize) {
        self.stmt_data_mut().next = next;
    }
}

/// Polymorphic statement value replacing Java references to abstract `Stmt`.
#[derive(Clone, Debug, PartialEq)]
pub enum Stmt {
    Assign(Box<AssignStmt>),
    Block(Box<Block>),
    Break(Box<Break>),
    Do(Box<Do>),
    Else(Box<Else>),
    Empty(Box<EmptyStmt>),
    For(Box<For>),
    If(Box<If>),
    Program(Box<Program>),
    Seq(Box<Seq>),
    While(Box<While>),
}

macro_rules! impl_from_stmt {
    ($variant:ident, $ty:ty) => {
        impl From<$ty> for Stmt {
            fn from(value: $ty) -> Self {
                Self::$variant(Box::new(value))
            }
        }
    };
}

impl_from_stmt!(Assign, AssignStmt);
impl_from_stmt!(Block, Block);
impl_from_stmt!(Break, Break);
impl_from_stmt!(Do, Do);
impl_from_stmt!(Else, Else);
impl_from_stmt!(Empty, EmptyStmt);
impl_from_stmt!(For, For);
impl_from_stmt!(If, If);
impl_from_stmt!(Program, Program);
impl_from_stmt!(Seq, Seq);
impl_from_stmt!(While, While);

static ENCLOSING: Mutex<Option<Stmt>> = Mutex::new(None);

impl Stmt {
    pub fn get_enclosing() -> Option<Stmt> {
        ENCLOSING
            .lock()
            .expect("enclosing statement mutex poisoned")
            .clone()
    }

    pub fn set_enclosing(enclosing: Option<Stmt>) {
        *ENCLOSING
            .lock()
            .expect("enclosing statement mutex poisoned") = enclosing;
    }
}

impl Node for Stmt {
    fn node_data(&self) -> &NodeData {
        match self {
            Self::Assign(n) => n.node_data(),
            Self::Block(n) => n.node_data(),
            Self::Break(n) => n.node_data(),
            Self::Do(n) => n.node_data(),
            Self::Else(n) => n.node_data(),
            Self::Empty(n) => n.node_data(),
            Self::For(n) => n.node_data(),
            Self::If(n) => n.node_data(),
            Self::Program(n) => n.node_data(),
            Self::Seq(n) => n.node_data(),
            Self::While(n) => n.node_data(),
        }
    }
}

impl StatementNode for Stmt {
    fn stmt_data(&self) -> &StmtData {
        match self {
            Self::Assign(n) => n.stmt_data(),
            Self::Block(n) => n.stmt_data(),
            Self::Break(n) => n.stmt_data(),
            Self::Do(n) => n.stmt_data(),
            Self::Else(n) => n.stmt_data(),
            Self::Empty(n) => n.stmt_data(),
            Self::For(n) => n.stmt_data(),
            Self::If(n) => n.stmt_data(),
            Self::Program(n) => n.stmt_data(),
            Self::Seq(n) => n.stmt_data(),
            Self::While(n) => n.stmt_data(),
        }
    }

    fn stmt_data_mut(&mut self) -> &mut StmtData {
        match self {
            Self::Assign(n) => n.stmt_data_mut(),
            Self::Block(n) => n.stmt_data_mut(),
            Self::Break(n) => n.stmt_data_mut(),
            Self::Do(n) => n.stmt_data_mut(),
            Self::Else(n) => n.stmt_data_mut(),
            Self::Empty(n) => n.stmt_data_mut(),
            Self::For(n) => n.stmt_data_mut(),
            Self::If(n) => n.stmt_data_mut(),
            Self::Program(n) => n.stmt_data_mut(),
            Self::Seq(n) => n.stmt_data_mut(),
            Self::While(n) => n.stmt_data_mut(),
        }
    }
}

impl<P, R> Walk<P, R> for Stmt {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R {
        match self {
            Self::Assign(n) => n.walk(walker, arg),
            Self::Block(n) => n.walk(walker, arg),
            Self::Break(n) => n.walk(walker, arg),
            Self::Do(n) => n.walk(walker, arg),
            Self::Else(n) => n.walk(walker, arg),
            Self::Empty(n) => n.walk(walker, arg),
            Self::For(n) => n.walk(walker, arg),
            Self::If(n) => n.walk(walker, arg),
            Self::Program(n) => n.walk(walker, arg),
            Self::Seq(n) => n.walk(walker, arg),
            Self::While(n) => n.walk(walker, arg),
        }
    }
}
