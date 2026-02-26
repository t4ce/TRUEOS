use core::mem::MaybeUninit;
use core::{fmt, fmt::Write};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(pub usize);

struct Node<T> {
    value: T,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    next_sibling: Option<NodeId>,
}

pub struct Tree<T, const N: usize> {
    nodes: [MaybeUninit<Node<T>>; N],
    used: [bool; N],
    len: usize,
    root: Option<NodeId>,
}

impl<T, const N: usize> Tree<T, N> {
    pub fn new() -> Self {
        // SAFETY: An uninitialized [MaybeUninit<_>; N] is valid.
        let nodes: [MaybeUninit<Node<T>>; N] = unsafe { MaybeUninit::uninit().assume_init() };
        Self {
            nodes,
            used: [false; N],
            len: 0,
            root: None,
        }
    }

    pub fn with_root(value: T) -> Option<Self> {
        let mut tree = Self::new();
        if tree.add_root(value).is_some() {
            Some(tree)
        } else {
            None
        }
    }

    pub fn capacity(&self) -> usize {
        N
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    pub fn add_root(&mut self, value: T) -> Option<NodeId> {
        if self.root.is_some() {
            return None;
        }
        let id = self.alloc_node(value, None)?;
        self.root = Some(id);
        Some(id)
    }

    pub fn add_child(&mut self, parent: NodeId, value: T) -> Option<NodeId> {
        if !self.is_used(parent) {
            return None;
        }
        let child = self.alloc_node(value, Some(parent))?;
        let parent_node = self.node_mut(parent);
        match parent_node.first_child {
            None => {
                parent_node.first_child = Some(child);
            }
            Some(mut cur) => loop {
                let next = self.node(cur).next_sibling;
                if let Some(n) = next {
                    cur = n;
                } else {
                    self.node_mut(cur).next_sibling = Some(child);
                    break;
                }
            },
        }
        Some(child)
    }

    /// Moves an existing node (and its subtree) under a new parent.
    ///
    /// This is a pure metadata operation: it rewires parent/child pointers.
    ///
    /// Returns `true` if the node was moved.
    pub fn move_node(&mut self, id: NodeId, new_parent: NodeId) -> bool {
        if !self.is_used(id) || !self.is_used(new_parent) {
            return false;
        }
        if self.root == Some(id) {
            return false;
        }
        if id == new_parent {
            return false;
        }

        // Prevent cycles: new_parent must not be within id's subtree.
        if self.is_descendant_of(new_parent, id) {
            return false;
        }

        let old_parent = match self.node(id).parent {
            Some(p) => p,
            None => return false,
        };

        // Unlink id from old parent's child list.
        let mut prev: Option<NodeId> = None;
        let mut cur = self.node(old_parent).first_child;
        while let Some(c) = cur {
            if c == id {
                let next = self.node(c).next_sibling;
                match prev {
                    None => self.node_mut(old_parent).first_child = next,
                    Some(p) => self.node_mut(p).next_sibling = next,
                }
                break;
            }
            prev = cur;
            cur = self.node(c).next_sibling;
        }

        // If we didn't find it in the old parent list, tree is inconsistent.
        if cur.is_none() {
            return false;
        }

        // Attach to new parent at the end of its child list.
        {
            let node = self.node_mut(id);
            node.parent = Some(new_parent);
            node.next_sibling = None;
        }

        let first = self.node(new_parent).first_child;
        match first {
            None => {
                self.node_mut(new_parent).first_child = Some(id);
            }
            Some(mut last) => loop {
                let next = self.node(last).next_sibling;
                if let Some(n) = next {
                    last = n;
                } else {
                    self.node_mut(last).next_sibling = Some(id);
                    break;
                }
            },
        }

        true
    }

    pub fn get(&self, id: NodeId) -> Option<&T> {
        if self.is_used(id) {
            Some(&self.node(id).value)
        } else {
            None
        }
    }

    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut T> {
        if self.is_used(id) {
            Some(&mut self.node_mut(id).value)
        } else {
            None
        }
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        if self.is_used(id) {
            self.node(id).parent
        } else {
            None
        }
    }

    pub fn children(&self, parent: NodeId) -> Children<'_, T, N> {
        let first = if self.is_used(parent) {
            self.node(parent).first_child
        } else {
            None
        };
        Children {
            tree: self,
            next: first,
        }
    }

    pub fn traverse_dfs_preorder<F>(&self, start: NodeId, mut f: F)
    where
        F: FnMut(NodeId, &T),
    {
        if !self.is_used(start) {
            return;
        }

        let mut stack: [NodeId; N] = [NodeId(usize::MAX); N];
        let mut sp = 0usize;
        stack[sp] = start;
        sp += 1;

        while sp > 0 {
            sp -= 1;
            let id = stack[sp];
            let node = self.node(id);
            f(id, &node.value);

            // Push children onto stack (depth-first). This will traverse siblings in reverse
            // insertion order, which is acceptable for a basic helper.
            let mut child = node.first_child;
            while let Some(ch) = child {
                if sp < N {
                    stack[sp] = ch;
                    sp += 1;
                }
                child = self.node(ch).next_sibling;
            }
        }
    }

    pub fn traverse_bfs<F>(&self, start: NodeId, mut f: F)
    where
        F: FnMut(NodeId, &T),
    {
        if !self.is_used(start) {
            return;
        }

        let mut queue: [NodeId; N] = [NodeId(usize::MAX); N];
        let mut head = 0usize;
        let mut tail = 0usize;

        queue[tail] = start;
        tail = (tail + 1) % N;

        while head != tail {
            let id = queue[head];
            head = (head + 1) % N;

            let node = self.node(id);
            f(id, &node.value);

            let mut child = node.first_child;
            while let Some(ch) = child {
                let next_tail = (tail + 1) % N;
                if next_tail == head {
                    break;
                }
                queue[tail] = ch;
                tail = next_tail;
                child = self.node(ch).next_sibling;
            }
        }
    }

    fn alloc_node(&mut self, value: T, parent: Option<NodeId>) -> Option<NodeId> {
        for i in 0..N {
            if !self.used[i] {
                let id = NodeId(i);
                let node = Node {
                    value,
                    parent,
                    first_child: None,
                    next_sibling: None,
                };
                self.nodes[i].write(node);
                self.used[i] = true;
                self.len += 1;
                return Some(id);
            }
        }
        None
    }

    fn is_used(&self, id: NodeId) -> bool {
        id.0 < N && self.used[id.0]
    }

    fn is_descendant_of(&self, mut node: NodeId, ancestor: NodeId) -> bool {
        while let Some(p) = self.parent(node) {
            if p == ancestor {
                return true;
            }
            node = p;
        }
        false
    }

    fn node(&self, id: NodeId) -> &Node<T> {
        unsafe { self.nodes[id.0].assume_init_ref() }
    }

    fn node_mut(&mut self, id: NodeId) -> &mut Node<T> {
        unsafe { self.nodes[id.0].assume_init_mut() }
    }

    /// Writes a simple ASCII tree to `out`.
    ///
    /// This is intended for kernel/shell diagnostics and avoids allocation.
    ///
    /// Notes:
    /// - The traversal is depth-first.
    /// - Sibling order is reverse insertion order (stack-based).
    /// - `max_entries` limits the number of printed nodes (including `start`).
    pub fn write_ascii_tree<W, F>(
        &self,
        start: NodeId,
        out: &mut W,
        max_entries: usize,
        mut render: F,
    ) -> fmt::Result
    where
        W: Write,
        F: FnMut(&T, &mut W) -> fmt::Result,
    {
        use crate::ascii_tree::{ArrayStack, AsciiStack, Frame, write_ascii_tree};

        if max_entries == 0 || !self.is_used(start) {
            return Ok(());
        }

        let mut stack: ArrayStack<Frame<NodeId>, N> = ArrayStack::new();
        let mut branches: [bool; N] = [false; N];
        let _ = stack.push(Frame {
            id: start,
            depth: 0,
            is_last: true,
        });

        write_ascii_tree(
            self,
            start,
            out,
            max_entries,
            &mut stack,
            &mut branches,
            "entries",
            |id, w| {
                let v = &self.node(id).value;
                render(v, w)
            },
        )
    }

    /// Returns an HTML `<ul>/<li>` representation of the tree.
    ///
    /// - Uses a single outer `<ul>`, with nested `<ul>` for children.
    /// - Only `<ul>` and `<li>` tags are emitted.
    /// - Node text is HTML-escaped.
    /// - Caps traversal at 1000 nodes (then inserts a truncation `<li>`).
    #[cfg(any(feature = "alloc", test))]
    pub fn html_tree_string<F>(&self, start: NodeId, mut render: F) -> alloc::string::String
    where
        F: FnMut(&T, &mut alloc::string::String),
    {
        use crate::html_tree::{DEFAULT_MAX_ITEMS, tree_to_html_string};

        tree_to_html_string(self, start, DEFAULT_MAX_ITEMS, |id, s| {
            let v = &self.node(id).value;
            render(v, s)
        })
    }
}

impl<T, const N: usize> crate::ascii_tree::AsciiTreeTraversal for Tree<T, N> {
    type NodeId = NodeId;

    fn is_valid(&self, id: Self::NodeId) -> bool {
        self.is_used(id)
    }

    fn push_children_rev<
        S: crate::ascii_tree::AsciiStack<crate::ascii_tree::Frame<Self::NodeId>>,
    >(
        &self,
        parent: Self::NodeId,
        child_depth: usize,
        stack: &mut S,
    ) {
        // Push in sibling order; stack pop yields reverse insertion order.
        let mut child = self.node(parent).first_child;
        while let Some(ch) = child {
            let child_is_last = self.node(ch).next_sibling.is_none();
            let _ = stack.push(crate::ascii_tree::Frame {
                id: ch,
                depth: child_depth,
                is_last: child_is_last,
            });
            child = self.node(ch).next_sibling;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_node_basic() {
        let mut t: Tree<&'static str, 16> = Tree::new();
        let root = t.add_root("root").unwrap();
        let a = t.add_child(root, "a").unwrap();
        let b = t.add_child(root, "b").unwrap();
        let c = t.add_child(a, "c").unwrap();

        assert_eq!(t.parent(c), Some(a));
        assert!(t.move_node(c, b));
        assert_eq!(t.parent(c), Some(b));
        assert!(!t.move_node(root, a), "should not move root");
    }

    #[test]
    fn move_node_prevents_cycles() {
        let mut t: Tree<&'static str, 16> = Tree::new();
        let root = t.add_root("root").unwrap();
        let a = t.add_child(root, "a").unwrap();
        let b = t.add_child(a, "b").unwrap();

        assert!(
            !t.move_node(a, b),
            "should not allow moving a under its descendant"
        );
        assert_eq!(t.parent(a), Some(root));
    }

    #[test]
    fn write_ascii_tree_smoke() {
        let mut t: Tree<&'static str, 16> = Tree::new();
        let root = t.add_root("root").unwrap();
        let a = t.add_child(root, "a").unwrap();
        let _b = t.add_child(root, "b").unwrap();
        let _c = t.add_child(a, "c").unwrap();

        let mut out = String::new();
        t.write_ascii_tree(root, &mut out, 64, |v, w| w.write_str(v))
            .unwrap();

        assert!(out.starts_with("root\n"));
        assert!(out.contains("|-- ") || out.contains("`-- "));
    }

    #[test]
    fn write_ascii_tree_respects_max_entries() {
        let mut t: Tree<&'static str, 32> = Tree::new();
        let root = t.add_root("root").unwrap();
        let a = t.add_child(root, "a").unwrap();
        let b = t.add_child(root, "b").unwrap();
        let _c = t.add_child(a, "c").unwrap();
        let _d = t.add_child(b, "d").unwrap();

        let mut out = String::new();
        t.write_ascii_tree(root, &mut out, 2, |v, w| w.write_str(v))
            .unwrap();

        // 2 entries printed, then truncation line.
        assert!(out.lines().count() >= 3);
        assert!(out.contains("... (max 2 entries)"));
    }

    #[test]
    fn html_tree_smoke() {
        let mut t: Tree<&'static str, 16> = Tree::new();
        let root = t.add_root("root").unwrap();
        let a = t.add_child(root, "a").unwrap();
        let _b = t.add_child(root, "b").unwrap();
        let _c = t.add_child(a, "c").unwrap();

        let html = t.html_tree_string(root, |v, s| s.push_str(v));

        assert!(html.starts_with("<ul><li>root"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>a"));
        assert!(html.contains("<li>b"));
        assert!(html.contains("<li>c"));
        assert!(html.ends_with("</ul>"));
    }

    #[test]
    fn html_tree_escapes_text() {
        let mut t: Tree<&'static str, 4> = Tree::new();
        let root = t.add_root("<&>\"'").unwrap();
        let html = t.html_tree_string(root, |v, s| s.push_str(v));

        assert!(html.contains("&lt;&amp;&gt;&quot;&#39;"));
    }
}

impl<T, const N: usize> Default for Tree<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Drop for Tree<T, N> {
    fn drop(&mut self) {
        for i in 0..N {
            if self.used[i] {
                unsafe {
                    self.nodes[i].assume_init_drop();
                }
            }
        }
    }
}

pub struct Children<'a, T, const N: usize> {
    tree: &'a Tree<T, N>,
    next: Option<NodeId>,
}

impl<'a, T, const N: usize> Iterator for Children<'a, T, N> {
    type Item = NodeId;

    fn next(&mut self) -> Option<Self::Item> {
        let cur = self.next?;
        self.next = self.tree.node(cur).next_sibling;
        Some(cur)
    }
}
