pub(crate) struct SmallLinkedList<const N: usize> {
    nodes: [Node; N],
    head: Option<usize>,
    tail: Option<usize>,
    len: usize,
}

#[derive(Clone, Copy)]
struct Node {
    value: i64,
    prev: Option<usize>,
    next: Option<usize>,
    used: bool,
}

impl Node {
    const EMPTY: Self = Self {
        value: 0,
        prev: None,
        next: None,
        used: false,
    };
}

impl<const N: usize> SmallLinkedList<N> {
    pub(crate) const fn new() -> Self {
        Self {
            nodes: [Node::EMPTY; N],
            head: None,
            tail: None,
            len: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push_back(&mut self, value: i64) -> bool {
        let Some(index) = self.free_index() else {
            return false;
        };

        self.nodes[index] = Node {
            value,
            prev: self.tail,
            next: None,
            used: true,
        };

        match self.tail {
            Some(tail) => {
                self.nodes[tail].next = Some(index);
            }
            None => {
                self.head = Some(index);
            }
        }

        self.tail = Some(index);
        self.len += 1;
        true
    }

    pub(crate) fn pop_front(&mut self) -> Option<i64> {
        let index = self.head?;
        let node = self.nodes[index];

        self.head = node.next;
        match self.head {
            Some(head) => {
                self.nodes[head].prev = None;
            }
            None => {
                self.tail = None;
            }
        }

        self.nodes[index] = Node::EMPTY;
        self.len -= 1;
        Some(node.value)
    }

    fn free_index(&self) -> Option<usize> {
        let mut index = 0;
        while index < N {
            if !self.nodes[index].used {
                return Some(index);
            }
            index += 1;
        }
        None
    }
}
