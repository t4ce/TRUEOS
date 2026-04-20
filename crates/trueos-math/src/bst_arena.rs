use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt::{self, Write};

pub type NodeId = usize;

/// A binary-search-tree node stored in an arena.
///
/// The `extra` field carries per-node metadata that differs between tree
/// flavours (e.g. `()` for a plain BST, `i32` height for an AVL tree).
pub struct BstNode<K, V, X> {
    pub key: K,
    pub value: V,
    pub left: Option<NodeId>,
    pub right: Option<NodeId>,
    pub extra: X,
}

/// Arena-backed storage for BST nodes with a free-list for slot reuse.
///
/// This is the shared foundation for both `BstMap` (unbalanced) and
/// `AvlTree` (self-balancing).  It owns the node vec, root pointer,
/// element count, and free-list — but knows nothing about balancing.
pub struct BstArena<K, V, X> {
    pub(crate) slots: Vec<Option<BstNode<K, V, X>>>,
    pub(crate) root: Option<NodeId>,
    pub(crate) len: usize,
    pub(crate) free: Vec<NodeId>,
}

// ── Constructors & bookkeeping (no Ord bound) ──

impl<K, V, X> BstArena<K, V, X> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            root: None,
            len: 0,
            free: Vec::new(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.len
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.root = None;
        self.len = 0;
        self.free.clear();
    }

    // ── node access ──

    #[inline]
    pub fn node(&self, id: NodeId) -> &BstNode<K, V, X> {
        self.slots[id].as_ref().unwrap()
    }

    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut BstNode<K, V, X> {
        self.slots[id].as_mut().unwrap()
    }

    #[inline]
    pub fn is_valid(&self, id: NodeId) -> bool {
        id < self.slots.len() && self.slots[id].is_some()
    }

    // ── allocation ──

    pub fn alloc(&mut self, key: K, value: V, extra: X) -> NodeId {
        let node = BstNode {
            key,
            value,
            left: None,
            right: None,
            extra,
        };
        if let Some(id) = self.free.pop() {
            self.slots[id] = Some(node);
            id
        } else {
            let id = self.slots.len();
            self.slots.push(Some(node));
            id
        }
    }

    pub fn take(&mut self, id: NodeId) -> BstNode<K, V, X> {
        let n = self.slots[id].take().unwrap();
        self.free.push(id);
        n
    }

    // ── structural queries (no Ord required) ──

    pub fn min(&self) -> Option<(&K, &V)> {
        let mut cur = self.root?;
        loop {
            match self.node(cur).left {
                Some(l) => cur = l,
                None => {
                    let n = self.node(cur);
                    return Some((&n.key, &n.value));
                }
            }
        }
    }

    pub fn max(&self) -> Option<(&K, &V)> {
        let mut cur = self.root?;
        loop {
            match self.node(cur).right {
                Some(r) => cur = r,
                None => {
                    let n = self.node(cur);
                    return Some((&n.key, &n.value));
                }
            }
        }
    }

    // ── search (requires Ord) ──

    pub fn get(&self, key: &K) -> Option<&V>
    where
        K: Ord,
    {
        let mut cur = self.root;
        while let Some(id) = cur {
            let n = self.node(id);
            match key.cmp(&n.key) {
                Ordering::Equal => return Some(&n.value),
                Ordering::Less => cur = n.left,
                Ordering::Greater => cur = n.right,
            }
        }
        None
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V>
    where
        K: Ord,
    {
        let mut cur = self.root;
        while let Some(id) = cur {
            let cmp = key.cmp(&self.node(id).key);
            match cmp {
                Ordering::Equal => return Some(&mut self.node_mut(id).value),
                Ordering::Less => cur = self.node(id).left,
                Ordering::Greater => cur = self.node(id).right,
            }
        }
        None
    }

    pub fn contains(&self, key: &K) -> bool
    where
        K: Ord,
    {
        self.get(key).is_some()
    }

    // ── ASCII rendering ──

    pub fn write_ascii_tree<W, F>(
        &self,
        out: &mut W,
        max_nodes: usize,
        mut render_node: F,
    ) -> fmt::Result
    where
        W: Write,
        F: FnMut(NodeId, &BstNode<K, V, X>, &mut W) -> fmt::Result,
    {
        use crate::ascii_tree::{AsciiBranches, AsciiStack, Frame, write_ascii_tree};

        if max_nodes == 0 {
            return Ok(());
        }
        let Some(root) = self.root else {
            return Ok(());
        };

        let mut stack: Vec<Frame<NodeId>> = Vec::new();
        let mut branches: Vec<bool> = Vec::new();
        stack.push(Frame {
            id: root,
            depth: 0,
            is_last: true,
        });

        write_ascii_tree(self, root, out, max_nodes, &mut stack, &mut branches, "nodes", |id, w| {
            let n = self.node(id);
            render_node(id, n, w)
        })
    }
}

// ── AsciiTreeTraversal for the arena itself ──

impl<K, V, X> crate::ascii_tree::AsciiTreeTraversal for BstArena<K, V, X> {
    type NodeId = NodeId;

    fn is_valid(&self, id: Self::NodeId) -> bool {
        BstArena::is_valid(self, id)
    }

    fn push_children_rev<
        S: crate::ascii_tree::AsciiStack<crate::ascii_tree::Frame<Self::NodeId>>,
    >(
        &self,
        parent: Self::NodeId,
        child_depth: usize,
        stack: &mut S,
    ) {
        let n = self.node(parent);
        let has_right = n.right.is_some();

        // Push right first (deeper in stack), then left (on top) so left prints first.
        if let Some(r) = n.right {
            let _ = stack.push(crate::ascii_tree::Frame {
                id: r,
                depth: child_depth,
                is_last: true,
            });
        }
        if let Some(l) = n.left {
            let _ = stack.push(crate::ascii_tree::Frame {
                id: l,
                depth: child_depth,
                is_last: !has_right,
            });
        }
    }
}

// ── In-order iterator ──

pub struct Iter<'a, K, V, X> {
    arena: &'a BstArena<K, V, X>,
    stack: Vec<NodeId>,
}

impl<'a, K, V, X> Iter<'a, K, V, X> {
    pub fn new(arena: &'a BstArena<K, V, X>) -> Self {
        let mut it = Self {
            arena,
            stack: Vec::new(),
        };
        it.push_left_spine(arena.root);
        it
    }

    fn push_left_spine(&mut self, mut node: Option<NodeId>) {
        while let Some(id) = node {
            self.stack.push(id);
            node = self.arena.node(id).left;
        }
    }
}

impl<'a, K, V, X> Iterator for Iter<'a, K, V, X> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let n = self.arena.node(id);
        self.push_left_spine(n.right);
        Some((&n.key, &n.value))
    }
}
