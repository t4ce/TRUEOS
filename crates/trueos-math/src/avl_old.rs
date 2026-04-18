use alloc::vec::Vec;
use core::cmp::Ordering;
use core::fmt::{self, Write};

type NodeId = usize;

struct Node<K, V> {
    key: K,
    value: V,
    left: Option<NodeId>,
    right: Option<NodeId>,
    height: i32,
}

/// A self-balancing AVL binary search tree.
///
/// - Arena-backed (`Vec` slots + free-list for reuse after deletion).
/// - `K: Ord` keys, arbitrary `V` values.
/// - O(log n) insert, delete, search.
/// - In-order iteration via stack-based iterator.
pub struct AvlTree<K, V> {
    slots: Vec<Option<Node<K, V>>>,
    root: Option<NodeId>,
    len: usize,
    free: Vec<NodeId>,
}

// ── Constructors & structural helpers (no Ord bound) ──

impl<K, V> AvlTree<K, V> {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            root: None,
            len: 0,
            free: Vec::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.slots.clear();
        self.root = None;
        self.len = 0;
        self.free.clear();
    }

    /// Returns the minimum key-value pair (leftmost node).
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

    /// Returns the maximum key-value pair (rightmost node).
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

    /// In-order iterator over `(&K, &V)`.
    pub fn iter(&self) -> Iter<'_, K, V> {
        let mut it = Iter {
            tree: self,
            stack: Vec::new(),
        };
        it.push_left_spine(self.root);
        it
    }

    /// Writes a simple ASCII representation of the tree structure.
    ///
    /// - `max_nodes` caps how many nodes will be printed.
    /// - `render_key` controls how each node's key is displayed.
    pub fn write_ascii_tree<W, F>(
        &self,
        out: &mut W,
        max_nodes: usize,
        mut render_key: F,
    ) -> fmt::Result
    where
        W: Write,
        F: FnMut(&K, &V, &mut W) -> fmt::Result,
    {
        use crate::ascii_tree::{Frame, write_ascii_tree};

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

        write_ascii_tree(
            self,
            root,
            out,
            max_nodes,
            &mut stack,
            &mut branches,
            "nodes",
            |id, w| {
                let n = self.node(id);
                let label = if n.left.is_some() || n.right.is_some() {
                    "N"
                } else {
                    "L"
                };
                w.write_str(label)?;
                w.write_str(" h=")?;
                write!(w, "{} ", n.height)?;
                render_key(&n.key, &n.value, w)
            },
        )
    }

    // ── internal arena helpers ──

    fn node(&self, id: NodeId) -> &Node<K, V> {
        self.slots[id].as_ref().unwrap()
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node<K, V> {
        self.slots[id].as_mut().unwrap()
    }

    fn height_of(&self, id: Option<NodeId>) -> i32 {
        match id {
            Some(id) => self.node(id).height,
            None => 0,
        }
    }

    fn update_height(&mut self, id: NodeId) {
        let lh = self.height_of(self.node(id).left);
        let rh = self.height_of(self.node(id).right);
        self.node_mut(id).height = 1 + lh.max(rh);
    }

    fn balance_factor(&self, id: NodeId) -> i32 {
        self.height_of(self.node(id).left) - self.height_of(self.node(id).right)
    }

    //        y                x
    //       / \              / \
    //      x   C    ->     A   y
    //     / \                 / \
    //    A   B               B   C
    fn rotate_right(&mut self, y: NodeId) -> NodeId {
        let x = self.node(y).left.unwrap();
        let b = self.node(x).right;
        self.node_mut(x).right = Some(y);
        self.node_mut(y).left = b;
        self.update_height(y);
        self.update_height(x);
        x
    }

    //      x                y
    //     / \              / \
    //    A   y    ->      x   C
    //       / \          / \
    //      B   C        A   B
    fn rotate_left(&mut self, x: NodeId) -> NodeId {
        let y = self.node(x).right.unwrap();
        let b = self.node(y).left;
        self.node_mut(y).left = Some(x);
        self.node_mut(x).right = b;
        self.update_height(x);
        self.update_height(y);
        y
    }

    fn rebalance(&mut self, id: NodeId) -> NodeId {
        self.update_height(id);
        let bf = self.balance_factor(id);

        if bf > 1 {
            // Left-heavy.
            let left = self.node(id).left.unwrap();
            if self.balance_factor(left) < 0 {
                // LR case: rotate left child left first.
                let new_left = self.rotate_left(left);
                self.node_mut(id).left = Some(new_left);
            }
            return self.rotate_right(id);
        }

        if bf < -1 {
            // Right-heavy.
            let right = self.node(id).right.unwrap();
            if self.balance_factor(right) > 0 {
                // RL case: rotate right child right first.
                let new_right = self.rotate_right(right);
                self.node_mut(id).right = Some(new_right);
            }
            return self.rotate_left(id);
        }

        id
    }

    fn alloc_node(&mut self, key: K, value: V) -> NodeId {
        let node = Node {
            key,
            value,
            left: None,
            right: None,
            height: 1,
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

    fn take_node(&mut self, id: NodeId) -> Node<K, V> {
        let n = self.slots[id].take().unwrap();
        self.free.push(id);
        n
    }

    /// Removes the minimum node from the subtree rooted at `id`.
    /// Returns `(new_subtree_root, removed_key, removed_value)`.
    fn remove_min(&mut self, id: NodeId) -> (Option<NodeId>, K, V) {
        let left = self.node(id).left;
        if left.is_none() {
            let right = self.node(id).right;
            let n = self.take_node(id);
            (right, n.key, n.value)
        } else {
            let (new_left, key, val) = self.remove_min(left.unwrap());
            self.node_mut(id).left = new_left;
            (Some(self.rebalance(id)), key, val)
        }
    }
}

// ── Key-ordered operations ──

impl<K: Ord, V> AvlTree<K, V> {
    /// Returns a reference to the value for `key`, or `None`.
    pub fn get(&self, key: &K) -> Option<&V> {
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

    /// Returns a mutable reference to the value for `key`, or `None`.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
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

    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Inserts `key -> value`. Returns the previous value if the key existed.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let (new_root, old) = self.insert_at(self.root, key, value);
        self.root = Some(new_root);
        if old.is_none() {
            self.len += 1;
        }
        old
    }

    /// Removes the entry for `key`. Returns the value if it existed.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (new_root, val) = self.remove_at(self.root, key)?;
        self.root = new_root;
        self.len -= 1;
        Some(val)
    }

    // ── recursive insert/remove ──

    fn insert_at(&mut self, node: Option<NodeId>, key: K, value: V) -> (NodeId, Option<V>) {
        let id = match node {
            None => return (self.alloc_node(key, value), None),
            Some(id) => id,
        };

        match key.cmp(&self.node(id).key) {
            Ordering::Equal => {
                let old = core::mem::replace(&mut self.node_mut(id).value, value);
                (id, Some(old))
            }
            Ordering::Less => {
                let left = self.node(id).left;
                let (new_left, old) = self.insert_at(left, key, value);
                self.node_mut(id).left = Some(new_left);
                if old.is_some() {
                    (id, old)
                } else {
                    (self.rebalance(id), old)
                }
            }
            Ordering::Greater => {
                let right = self.node(id).right;
                let (new_right, old) = self.insert_at(right, key, value);
                self.node_mut(id).right = Some(new_right);
                if old.is_some() {
                    (id, old)
                } else {
                    (self.rebalance(id), old)
                }
            }
        }
    }

    fn remove_at(&mut self, node: Option<NodeId>, key: &K) -> Option<(Option<NodeId>, V)> {
        let id = node?;

        match key.cmp(&self.node(id).key) {
            Ordering::Less => {
                let left = self.node(id).left;
                let (new_left, val) = self.remove_at(left, key)?;
                self.node_mut(id).left = new_left;
                Some((Some(self.rebalance(id)), val))
            }
            Ordering::Greater => {
                let right = self.node(id).right;
                let (new_right, val) = self.remove_at(right, key)?;
                self.node_mut(id).right = new_right;
                Some((Some(self.rebalance(id)), val))
            }
            Ordering::Equal => {
                let left = self.node(id).left;
                let right = self.node(id).right;

                match (left, right) {
                    // Leaf node.
                    (None, None) => {
                        let n = self.take_node(id);
                        Some((None, n.value))
                    }
                    // Single child.
                    (Some(child), None) | (None, Some(child)) => {
                        let n = self.take_node(id);
                        Some((Some(child), n.value))
                    }
                    // Two children: replace with in-order successor.
                    (Some(_), Some(right_child)) => {
                        let (new_right, succ_key, succ_val) = self.remove_min(right_child);
                        let old_val = core::mem::replace(&mut self.node_mut(id).value, succ_val);
                        let _ = core::mem::replace(&mut self.node_mut(id).key, succ_key);
                        self.node_mut(id).right = new_right;
                        Some((Some(self.rebalance(id)), old_val))
                    }
                }
            }
        }
    }
}

// ── Default ──

impl<K, V> Default for AvlTree<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

// ── AsciiTreeTraversal ──

impl<K, V> crate::ascii_tree::AsciiTreeTraversal for AvlTree<K, V> {
    type NodeId = NodeId;

    fn is_valid(&self, id: Self::NodeId) -> bool {
        id < self.slots.len() && self.slots[id].is_some()
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
        let has_left = n.left.is_some();
        let has_right = n.right.is_some();

        // Push right first (deeper in stack), then left (on top) so left prints first.
        if let Some(r) = n.right {
            let _ = stack.push(crate::ascii_tree::Frame {
                id: r,
                depth: child_depth,
                is_last: true, // right is always the last child
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

// ── Iterator ──

pub struct Iter<'a, K, V> {
    tree: &'a AvlTree<K, V>,
    stack: Vec<NodeId>,
}

impl<'a, K, V> Iter<'a, K, V> {
    fn push_left_spine(&mut self, mut node: Option<NodeId>) {
        while let Some(id) = node {
            self.stack.push(id);
            node = self.tree.node(id).left;
        }
    }
}

impl<'a, K, V> Iterator for Iter<'a, K, V> {
    type Item = (&'a K, &'a V);

    fn next(&mut self) -> Option<Self::Item> {
        let id = self.stack.pop()?;
        let n = self.tree.node(id);
        self.push_left_spine(n.right);
        Some((&n.key, &n.value))
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::AvlTree;
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Verify the AVL invariant recursively: correct heights, |balance| <= 1, BST ordering.
    fn verify<K: Ord, V>(tree: &AvlTree<K, V>) -> bool {
        verify_subtree(tree, tree.root, None, None)
    }

    fn verify_subtree<K: Ord, V>(
        tree: &AvlTree<K, V>,
        node: Option<usize>,
        min: Option<&K>,
        max: Option<&K>,
    ) -> bool {
        let id = match node {
            None => return true,
            Some(id) => id,
        };

        let n = tree.node(id);

        // BST bounds.
        if let Some(lo) = min {
            if n.key <= *lo {
                return false;
            }
        }
        if let Some(hi) = max {
            if n.key >= *hi {
                return false;
            }
        }

        // Height correctness.
        let lh = tree.height_of(n.left);
        let rh = tree.height_of(n.right);
        if n.height != 1 + lh.max(rh) {
            return false;
        }

        // Balance factor in [-1, 1].
        if (lh - rh).abs() > 1 {
            return false;
        }

        verify_subtree(tree, n.left, min, Some(&n.key))
            && verify_subtree(tree, n.right, Some(&n.key), max)
    }

    #[test]
    fn empty_tree() {
        let t: AvlTree<u64, u64> = AvlTree::new();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.get(&0), None);
        assert_eq!(t.min(), None);
        assert_eq!(t.max(), None);
        assert_eq!(t.iter().count(), 0);
        assert!(verify(&t));
    }

    #[test]
    fn insert_single() {
        let mut t = AvlTree::new();
        assert_eq!(t.insert(42, "hello"), None);
        assert_eq!(t.len(), 1);
        assert_eq!(t.get(&42), Some(&"hello"));
        assert!(t.contains(&42));
        assert!(!t.contains(&0));
        assert!(verify(&t));
    }

    #[test]
    fn insert_replace() {
        let mut t = AvlTree::new();
        assert_eq!(t.insert(1, 10u32), None);
        assert_eq!(t.insert(1, 20), Some(10));
        assert_eq!(t.get(&1), Some(&20));
        assert_eq!(t.len(), 1);
        assert!(verify(&t));
    }

    #[test]
    fn ascending_inserts_trigger_rotations() {
        let mut t = AvlTree::new();
        for i in 0..32u64 {
            t.insert(i, i * 10);
        }
        assert_eq!(t.len(), 32);
        for i in 0..32u64 {
            assert_eq!(t.get(&i), Some(&(i * 10)));
        }
        assert!(verify(&t));
    }

    #[test]
    fn descending_inserts_trigger_rotations() {
        let mut t = AvlTree::new();
        for i in (0..32u64).rev() {
            t.insert(i, i);
        }
        assert_eq!(t.len(), 32);
        assert!(verify(&t));
    }

    #[test]
    fn zigzag_inserts() {
        let mut t = AvlTree::new();
        let keys = [10u64, 30, 20, 40, 25, 5, 15, 35, 50, 1];
        for &k in &keys {
            t.insert(k, k);
        }
        assert_eq!(t.len(), keys.len());
        for &k in &keys {
            assert!(t.contains(&k));
        }
        assert!(verify(&t));
    }

    #[test]
    fn iter_in_order() {
        let mut t = AvlTree::new();
        let keys = [5u64, 3, 7, 1, 4, 6, 8, 2];
        for &k in &keys {
            t.insert(k, k * 100);
        }
        let collected: Vec<(u64, u64)> = t.iter().map(|(&k, &v)| (k, v)).collect();
        assert_eq!(
            collected,
            alloc::vec![(1, 100), (2, 200), (3, 300), (4, 400), (5, 500), (6, 600), (7, 700), (8, 800)]
        );
    }

    #[test]
    fn min_max() {
        let mut t = AvlTree::new();
        for k in [20u64, 10, 30, 5, 15, 25, 35] {
            t.insert(k, k);
        }
        assert_eq!(t.min(), Some((&5, &5)));
        assert_eq!(t.max(), Some((&35, &35)));
    }

    #[test]
    fn remove_leaf() {
        let mut t = AvlTree::new();
        t.insert(10u64, "a");
        t.insert(5, "b");
        t.insert(15, "c");

        assert_eq!(t.remove(&5), Some("b"));
        assert_eq!(t.len(), 2);
        assert!(!t.contains(&5));
        assert!(verify(&t));
    }

    #[test]
    fn remove_node_with_one_child() {
        let mut t = AvlTree::new();
        t.insert(10u64, 10);
        t.insert(5, 5);
        t.insert(15, 15);
        t.insert(3, 3);

        assert_eq!(t.remove(&5), Some(5));
        assert_eq!(t.len(), 3);
        assert!(t.contains(&3));
        assert!(verify(&t));
    }

    #[test]
    fn remove_node_with_two_children() {
        let mut t = AvlTree::new();
        for k in [10u64, 5, 15, 3, 7, 12, 20] {
            t.insert(k, k);
        }

        assert_eq!(t.remove(&10), Some(10));
        assert_eq!(t.len(), 6);
        assert!(!t.contains(&10));
        for &k in &[5, 15, 3, 7, 12, 20] {
            assert!(t.contains(&k));
        }
        assert!(verify(&t));
    }

    #[test]
    fn remove_root() {
        let mut t = AvlTree::new();
        t.insert(1u64, "only");
        assert_eq!(t.remove(&1), Some("only"));
        assert!(t.is_empty());
        assert!(verify(&t));
    }

    #[test]
    fn remove_nonexistent() {
        let mut t = AvlTree::new();
        t.insert(1u64, 1);
        assert_eq!(t.remove(&999), None);
        assert_eq!(t.len(), 1);
    }

    #[test]
    fn insert_remove_stress() {
        let mut t = AvlTree::new();

        // Insert 0..128.
        for i in 0..128u64 {
            t.insert(i, i);
        }
        assert_eq!(t.len(), 128);
        assert!(verify(&t));

        // Remove even keys.
        for i in (0..128u64).step_by(2) {
            assert_eq!(t.remove(&i), Some(i));
        }
        assert_eq!(t.len(), 64);
        assert!(verify(&t));

        // Verify remaining.
        for i in 0..128u64 {
            if i % 2 == 0 {
                assert!(!t.contains(&i));
            } else {
                assert_eq!(t.get(&i), Some(&i));
            }
        }

        // Re-insert evens.
        for i in (0..128u64).step_by(2) {
            t.insert(i, i + 1000);
        }
        assert_eq!(t.len(), 128);
        assert!(verify(&t));

        // Verify iteration order.
        let keys: Vec<u64> = t.iter().map(|(&k, _)| k).collect();
        let expected: Vec<u64> = (0..128).collect();
        assert_eq!(keys, expected);
    }

    #[test]
    fn slot_reuse() {
        let mut t: AvlTree<u64, u64> = AvlTree::new();
        for i in 0..10 {
            t.insert(i, i);
        }
        let slots_before = t.slots.len();

        // Remove all.
        for i in 0..10 {
            t.remove(&i);
        }
        assert!(t.is_empty());

        // Re-insert: should reuse freed slots.
        for i in 0..10 {
            t.insert(i, i);
        }
        assert_eq!(t.slots.len(), slots_before);
        assert!(verify(&t));
    }

    #[test]
    fn get_mut_updates_value() {
        let mut t = AvlTree::new();
        t.insert(5u64, 100u64);
        if let Some(v) = t.get_mut(&5) {
            *v = 200;
        }
        assert_eq!(t.get(&5), Some(&200));
    }

    #[test]
    fn clear_resets() {
        let mut t = AvlTree::new();
        for i in 0..50u64 {
            t.insert(i, i);
        }
        t.clear();
        assert!(t.is_empty());
        assert_eq!(t.len(), 0);
        assert_eq!(t.iter().count(), 0);
    }

    #[test]
    fn write_ascii_tree_smoke() {
        let mut t = AvlTree::new();
        for k in [10u64, 5, 15, 3, 7, 12, 20] {
            t.insert(k, k);
        }

        let mut out = String::new();
        t.write_ascii_tree(&mut out, 32, |k, _v, w| write!(w, "{}", k))
            .unwrap();

        assert!(out.lines().count() >= 1);
        assert!(out.contains("10") || out.contains("5") || out.contains("15"));
    }

    #[test]
    fn large_sequential_insert_height() {
        let mut t = AvlTree::new();
        let n = 1000u64;
        for i in 0..n {
            t.insert(i, ());
        }
        assert_eq!(t.len(), n as usize);
        assert!(verify(&t));

        // AVL height ≤ 1.44 * log2(n+2). For n=1000, max ~15.
        let root = t.root.unwrap();
        let h = t.node(root).height;
        assert!(h <= 15, "height {} too large for {} nodes", h, n);
    }
}
