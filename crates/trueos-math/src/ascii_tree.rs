use core::fmt::{self, Write};
use core::mem::MaybeUninit;

#[derive(Copy, Clone)]
pub struct Frame<Id> {
    pub id: Id,
    pub depth: usize,
    pub is_last: bool,
}

pub trait AsciiStack<T> {
    fn push(&mut self, v: T) -> bool;
    fn pop(&mut self) -> Option<T>;
}

pub struct ArrayStack<T, const N: usize> {
    buf: [MaybeUninit<T>; N],
    len: usize,
}

impl<T, const N: usize> ArrayStack<T, N> {
    pub fn new() -> Self {
        // SAFETY: An uninitialized [MaybeUninit<_>; N] is valid.
        let buf: [MaybeUninit<T>; N] = unsafe { MaybeUninit::uninit().assume_init() };
        Self { buf, len: 0 }
    }
}

impl<T, const N: usize> AsciiStack<T> for ArrayStack<T, N> {
    fn push(&mut self, v: T) -> bool {
        if self.len >= N {
            return false;
        }
        self.buf[self.len].write(v);
        self.len += 1;
        true
    }

    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: slot was initialized by push and is within bounds.
        Some(unsafe { self.buf[self.len].assume_init_read() })
    }
}

impl<T, const N: usize> Drop for ArrayStack<T, N> {
    fn drop(&mut self) {
        for i in 0..self.len {
            unsafe {
                self.buf[i].assume_init_drop();
            }
        }
    }
}

pub trait AsciiBranches {
    fn ensure_len(&mut self, len: usize);
    fn get(&self, idx: usize) -> bool;
    fn set(&mut self, idx: usize, val: bool);
}

impl<const N: usize> AsciiBranches for [bool; N] {
    fn ensure_len(&mut self, _len: usize) {
        // fixed-size; depth is naturally bounded for fixed-capacity trees.
    }

    fn get(&self, idx: usize) -> bool {
        if idx < N { self[idx] } else { false }
    }

    fn set(&mut self, idx: usize, val: bool) {
        if idx < N {
            self[idx] = val;
        }
    }
}

#[cfg(any(feature = "alloc", test))]
impl AsciiBranches for alloc::vec::Vec<bool> {
    fn ensure_len(&mut self, len: usize) {
        if self.len() < len {
            self.resize(len, false);
        }
    }

    fn get(&self, idx: usize) -> bool {
        self.as_slice().get(idx).copied().unwrap_or(false)
    }

    fn set(&mut self, idx: usize, val: bool) {
        if idx < self.len() {
            self[idx] = val;
        }
    }
}

#[cfg(any(feature = "alloc", test))]
impl<T> AsciiStack<T> for alloc::vec::Vec<T> {
    fn push(&mut self, v: T) -> bool {
        self.push(v);
        true
    }

    fn pop(&mut self) -> Option<T> {
        self.pop()
    }
}

pub trait AsciiTreeTraversal {
    type NodeId: Copy;

    fn is_valid(&self, id: Self::NodeId) -> bool;

    fn push_children_rev<S: AsciiStack<Frame<Self::NodeId>>>(
        &self,
        parent: Self::NodeId,
        child_depth: usize,
        stack: &mut S,
    );
}

pub fn write_ascii_tree<T, W, S, B, F>(
    tree: &T,
    root: T::NodeId,
    out: &mut W,
    max_items: usize,
    stack: &mut S,
    branches: &mut B,
    trunc_label: &'static str,
    mut fmt_node: F,
) -> fmt::Result
where
    T: AsciiTreeTraversal,
    W: Write,
    S: AsciiStack<Frame<T::NodeId>>,
    B: AsciiBranches,
    F: FnMut(T::NodeId, &mut W) -> fmt::Result,
{
    if max_items == 0 {
        return Ok(());
    }
    if !tree.is_valid(root) {
        return Ok(());
    }

    let mut printed = 0usize;

    while let Some(Frame { id, depth, is_last }) = stack.pop() {
        if printed >= max_items {
            writeln!(out, "... (max {} {})", max_items, trunc_label)?;
            break;
        }

        branches.ensure_len(depth + 1);

        if depth == 0 {
            fmt_node(id, out)?;
            out.write_char('\n')?;
        } else {
            for d in 0..(depth - 1) {
                if branches.get(d) {
                    out.write_str("|   ")?;
                } else {
                    out.write_str("    ")?;
                }
            }

            if is_last {
                out.write_str("`-- ")?;
            } else {
                out.write_str("|-- ")?;
            }

            fmt_node(id, out)?;
            out.write_char('\n')?;
        }

        printed += 1;
        branches.set(depth, !is_last);

        tree.push_children_rev(id, depth + 1, stack);
    }

    Ok(())
}
