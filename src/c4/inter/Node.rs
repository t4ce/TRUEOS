use std::sync::atomic::{AtomicUsize, Ordering};

use super::{
    Access, And, Arith, AssignElem, AssignId, AssignStmt, Block, Break, Constant, Do, Else,
    EmptyStmt, For, Id, If, Not, Or, Program, Rel, Seq, Unary, While,
};

static LEXER_LINE: AtomicUsize = AtomicUsize::new(1);

pub fn lexer_line() -> usize {
    LEXER_LINE.load(Ordering::Relaxed)
}

pub fn set_lexer_line(line: usize) {
    LEXER_LINE.store(line, Ordering::Relaxed);
}

/// State inherited by every Java `Node`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeData {
    lexline: usize,
}

impl NodeData {
    pub fn new() -> Self {
        Self {
            lexline: lexer_line(),
        }
    }

    pub fn at_line(lexline: usize) -> Self {
        Self { lexline }
    }
}

impl Default for NodeData {
    fn default() -> Self {
        Self::new()
    }
}

pub trait Node {
    fn node_data(&self) -> &NodeData;

    fn get_lexline(&self) -> usize {
        self.node_data().lexline
    }

    fn error(&self, message: &str) -> ! {
        panic!("near line {}: {message}", self.get_lexline())
    }
}

/// Generic visitor equivalent to `treewalker.TreeWalker<R, P>`.
pub trait TreeWalker<P, R> {
    fn walk<N: Walk<P, R>>(&mut self, node: &N, arg: P) -> R
    where
        Self: Sized,
    {
        node.walk(self, arg)
    }

    fn walk_access_node(&mut self, node: &Access, arg: P) -> R;
    fn walk_and_node(&mut self, node: &And, arg: P) -> R;
    fn walk_arith_node(&mut self, node: &Arith, arg: P) -> R;
    fn walk_assign_elem_node(&mut self, node: &AssignElem, arg: P) -> R;
    fn walk_assign_id_node(&mut self, node: &AssignId, arg: P) -> R;
    fn walk_assign_stmt_node(&mut self, node: &AssignStmt, arg: P) -> R;
    fn walk_block_node(&mut self, node: &Block, arg: P) -> R;
    fn walk_break_node(&mut self, node: &Break, arg: P) -> R;
    fn walk_constant_node(&mut self, node: &Constant, arg: P) -> R;
    fn walk_do_node(&mut self, node: &Do, arg: P) -> R;
    fn walk_else_node(&mut self, node: &Else, arg: P) -> R;
    fn walk_empty_stmt_node(&mut self, node: &EmptyStmt, arg: P) -> R;
    fn walk_for_node(&mut self, node: &For, arg: P) -> R;
    fn walk_id_node(&mut self, node: &Id, arg: P) -> R;
    fn walk_if_node(&mut self, node: &If, arg: P) -> R;
    fn walk_not_node(&mut self, node: &Not, arg: P) -> R;
    fn walk_or_node(&mut self, node: &Or, arg: P) -> R;
    fn walk_program_node(&mut self, node: &Program, arg: P) -> R;
    fn walk_rel_node(&mut self, node: &Rel, arg: P) -> R;
    fn walk_seq_node(&mut self, node: &Seq, arg: P) -> R;
    fn walk_unary_node(&mut self, node: &Unary, arg: P) -> R;
    fn walk_while_node(&mut self, node: &While, arg: P) -> R;
}

pub trait Walk<P, R> {
    fn walk<W: TreeWalker<P, R> + ?Sized>(&self, walker: &mut W, arg: P) -> R;
}
