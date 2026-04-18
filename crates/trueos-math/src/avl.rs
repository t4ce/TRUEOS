use core::cmp::Ordering;
use core::fmt::{self, Write};

use crate::bst_arena::{BstArena, BstNode, NodeId};

/// AVL extra data stored in each node: the subtree height.
type AvlExtra = i32;

/// A self-balancing AVL binary search tree.
///
/// Built on top of `BstArena` (shared with `BstMap`), adding:
/// - A height field per node (`BstNode.extra: i32`).
/// - Left/right rotations and the 4-case rebalance step.
/// - Rebalance calls after every structural mutation.
///
/// O(log n) insert, delete, search — guaranteed.
pub struct AvlTree<K, V> {
    arena: BstArena<K, V, AvlExtra>,
}

// ── Constructors & delegation (no Ord bound) ──

impl<K, V> AvlTree<K, V> {
    pub const fn new() -> Self {
        Self {
            arena: BstArena::new(),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.arena.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }

    pub fn clear(&mut self) {
        self.arena.clear();
    }

    pub fn min(&self) -> Option<(&K, &V)> {
        self.arena.min()
    }

    pub fn max(&self) -> Option<(&K, &V)> {
        self.arena.max()
    }

    pub fn iter(&self) -> crate::bst_arena::Iter<'_, K, V, AvlExtra> {
        crate::bst_arena::Iter::new(&self.arena)
    }

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
        self.arena.write_ascii_tree(out, max_nodes, |_id, n, w| {
            let label = if n.left.is_some() || n.right.is_some() {
                "N"
            } else {
                "L"
            };
            w.write_str(label)?;
            w.write_str(" h=")?;
            write!(w, "{} ", n.extra)?;
            render_key(&n.key, &n.value, w)
        })
    }

    // ── AVL-specific helpers ──

    fn height_of(&self, id: Option<NodeId>) -> i32 {
        match id {
            Some(id) => self.arena.node(id).extra,
            None => 0,
        }
    }

    fn update_height(&mut self, id: NodeId) {
        let lh = self.height_of(self.arena.node(id).left);
        let rh = self.height_of(self.arena.node(id).right);
        self.arena.node_mut(id).extra = 1 + lh.max(rh);
    }

    fn balance_factor(&self, id: NodeId) -> i32 {
        self.height_of(self.arena.node(id).left) - self.height_of(self.arena.node(id).right)
    }

    //        y                x
    //       / \              / \
    //      x   C    ->     A   y
    //     / \                 / \
    //    A   B               B   C
    fn rotate_right(&mut self, y: NodeId) -> NodeId {
        let x = self.arena.node(y).left.unwrap();
        let b = self.arena.node(x).right;
        self.arena.node_mut(x).right = Some(y);
        self.arena.node_mut(y).left = b;
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
        let y = self.arena.node(x).right.unwrap();
        let b = self.arena.node(y).left;
        self.arena.node_mut(y).left = Some(x);
        self.arena.node_mut(x).right = b;
        self.update_height(x);
        self.update_height(y);
        y
    }

    fn rebalance(&mut self, id: NodeId) -> NodeId {
        self.update_height(id);
        let bf = self.balance_factor(id);

        if bf > 1 {
            let left = self.arena.node(id).left.unwrap();
            if self.balance_factor(left) < 0 {
                let new_left = self.rotate_left(left);
                self.arena.node_mut(id).left = Some(new_left);
            }
            return self.rotate_right(id);
        }

        if bf < -1 {
            let right = self.arena.node(id).right.unwrap();
            if self.balance_factor(right) > 0 {
                let new_right = self.rotate_right(right);
                self.arena.node_mut(id).right = Some(new_right);
            }
            return self.rotate_left(id);
        }

        id
    }

    fn remove_min(&mut self, id: NodeId) -> (Option<NodeId>, K, V) {
        let left = self.arena.node(id).left;
        if left.is_none() {
            let right = self.arena.node(id).right;
            let n = self.arena.take(id);
            (right, n.key, n.value)
        } else {
            let (new_left, key, val) = self.remove_min(left.unwrap());
            self.arena.node_mut(id).left = new_left;
            (Some(self.rebalance(id)), key, val)
        }
    }
}

// ── Key-ordered operations ──

impl<K: Ord, V> AvlTree<K, V> {
    pub fn get(&self, key: &K) -> Option<&V> {
        self.arena.get(key)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.arena.get_mut(key)
    }

    pub fn contains(&self, key: &K) -> bool {
        self.arena.contains(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let (new_root, old) = self.insert_at(self.arena.root, key, value);
        self.arena.root = Some(new_root);
        if old.is_none() {
            self.arena.len += 1;
        }
        old
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (new_root, val) = self.remove_at(self.arena.root, key)?;
        self.arena.root = new_root;
        self.arena.len -= 1;
        Some(val)
    }

    // ── recursive insert/remove (with rebalance) ──

    fn insert_at(&mut self, node: Option<NodeId>, key: K, value: V) -> (NodeId, Option<V>) {
        let id = match node {
            None => return (self.arena.alloc(key, value, 1), None),
            Some(id) => id,
        };

        match key.cmp(&self.arena.node(id).key) {
            Ordering::Equal => {
                let old = core::mem::replace(&mut self.arena.node_mut(id).value, value);
                (id, Some(old))
            }
            Ordering::Less => {
                let left = self.arena.node(id).left;
                let (new_left, old) = self.insert_at(left, key, value);
                self.arena.node_mut(id).left = Some(new_left);
                if old.is_some() {
                    (id, old)
                } else {
                    (self.rebalance(id), old)
                }
            }
            Ordering::Greater => {
                let right = self.arena.node(id).right;
                let (new_right, old) = self.insert_at(right, key, value);
                self.arena.node_mut(id).right = Some(new_right);
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

        match key.cmp(&self.arena.node(id).key) {
            Ordering::Less => {
                let left = self.arena.node(id).left;
                let (new_left, val) = self.remove_at(left, key)?;
                self.arena.node_mut(id).left = new_left;
                Some((Some(self.rebalance(id)), val))
            }
            Ordering::Greater => {
                let right = self.arena.node(id).right;
                let (new_right, val) = self.remove_at(right, key)?;
                self.arena.node_mut(id).right = new_right;
                Some((Some(self.rebalance(id)), val))
            }
            Ordering::Equal => {
                let left = self.arena.node(id).left;
                let right = self.arena.node(id).right;

                match (left, right) {
                    (None, None) => {
                        let n = self.arena.take(id);
                        Some((None, n.value))
                    }
                    (Some(child), None) | (None, Some(child)) => {
                        let n = self.arena.take(id);
                        Some((Some(child), n.value))
                    }
                    (Some(_), Some(right_child)) => {
                        let (new_right, succ_key, succ_val) = self.remove_min(right_child);
                        let old_val =
                            core::mem::replace(&mut self.arena.node_mut(id).value, succ_val);
                        let _ = core::mem::replace(&mut self.arena.node_mut(id).key, succ_key);
                        self.arena.node_mut(id).right = new_right;
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

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::AvlTree;
    use alloc::string::String;
    use alloc::vec::Vec;

    /// Verify the AVL invariant recursively: correct heights, |balance| <= 1, BST ordering.
    fn verify<K: Ord, V>(tree: &AvlTree<K, V>) -> bool {
        verify_subtree(tree, tree.arena.root, None, None)
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

        let n = tree.arena.node(id);

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
        if n.extra != 1 + lh.max(rh) {
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
            alloc::vec![
                (1, 100),
                (2, 200),
                (3, 300),
                (4, 400),
                (5, 500),
                (6, 600),
                (7, 700),
                (8, 800)
            ]
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

        for i in 0..128u64 {
            t.insert(i, i);
        }
        assert_eq!(t.len(), 128);
        assert!(verify(&t));

        for i in (0..128u64).step_by(2) {
            assert_eq!(t.remove(&i), Some(i));
        }
        assert_eq!(t.len(), 64);
        assert!(verify(&t));

        for i in 0..128u64 {
            if i % 2 == 0 {
                assert!(!t.contains(&i));
            } else {
                assert_eq!(t.get(&i), Some(&i));
            }
        }

        for i in (0..128u64).step_by(2) {
            t.insert(i, i + 1000);
        }
        assert_eq!(t.len(), 128);
        assert!(verify(&t));

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
        let slots_before = t.arena.slots.len();

        for i in 0..10 {
            t.remove(&i);
        }
        assert!(t.is_empty());

        for i in 0..10 {
            t.insert(i, i);
        }
        assert_eq!(t.arena.slots.len(), slots_before);
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

        // AVL height <= 1.44 * log2(n+2). For n=1000, max ~15.
        let root = t.arena.root.unwrap();
        let h = t.arena.node(root).extra;
        assert!(h <= 15, "height {} too large for {} nodes", h, n);
    }
}
