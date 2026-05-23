#![cfg_attr(not(feature = "full"), allow(dead_code))]
// It doesn't make sense to enforce `unsafe_op_in_unsafe_fn` for this module because
//
// * The intrusive linked list naturally relies on unsafe operations.
// * Excessive `unsafe {}` blocks hurt readability significantly.
// TODO: replace with `#[expect(unsafe_op_in_unsafe_fn)]` after bumpping
// the MSRV to 1.81.0.
#![allow(unsafe_op_in_unsafe_fn)]

//! An intrusive double linked list of data.
//!
//! The data structure supports tracking pinned nodes. Most of the data
//! structure's APIs are `unsafe` as they require the caller to ensure the
//! specified node is actually contained by the list.

use core::cell::UnsafeCell;
use core::fmt;
use core::marker::{PhantomData, PhantomPinned};
use core::mem::ManuallyDrop;
use core::ptr::{self, NonNull};

/// An intrusive linked list.
///
/// Currently, the list is not emptied on drop. It is the caller's
/// responsibility to ensure the list is empty before dropping it.
pub(crate) struct LinkedList<L, T> {
    /// Linked list head
    head: Option<NonNull<T>>,

    /// Linked list tail
    tail: Option<NonNull<T>>,

    /// Node type marker.
    _marker: PhantomData<*const L>,
}

unsafe impl<L: Link> Send for LinkedList<L, L::Target> where L::Target: Send {}
unsafe impl<L: Link> Sync for LinkedList<L, L::Target> where L::Target: Sync {}

/// Defines how a type is tracked within a linked list.
///
/// In order to support storing a single type within multiple lists, accessing
/// the list pointers is decoupled from the entry type.
///
/// # Safety
///
/// Implementations must guarantee that `Target` types are pinned in memory. In
/// other words, when a node is inserted, the value will not be moved as long as
/// it is stored in the list.
pub(crate) unsafe trait Link {
    /// Handle to the list entry.
    ///
    /// This is usually a pointer-ish type.
    type Handle;

    /// Node type.
    type Target;

    /// Convert the handle to a raw pointer without consuming the handle.
    #[allow(clippy::wrong_self_convention)]
    fn as_raw(handle: &Self::Handle) -> NonNull<Self::Target>;

    /// Convert the raw pointer to a handle
    unsafe fn from_raw(ptr: NonNull<Self::Target>) -> Self::Handle;

    /// Return the pointers for a node
    ///
    /// # Safety
    ///
    /// The resulting pointer should have the same tag in the stacked-borrows
    /// stack as the argument. In particular, the method may not create an
    /// intermediate reference in the process of creating the resulting raw
    /// pointer.
    ///
    /// The `target` pointer must be valid.
    unsafe fn pointers(target: NonNull<Self::Target>) -> NonNull<Pointers<Self::Target>>;
}

/// Previous / next pointers.
pub(crate) struct Pointers<T> {
    inner: UnsafeCell<PointersInner<T>>,
}
/// We do not want the compiler to put the `noalias` attribute on mutable
/// references to this type, so the type has been made `!Unpin` with a
/// `PhantomPinned` field.
///
/// Additionally, we never access the `prev` or `next` fields directly, as any
/// such access would implicitly involve the creation of a reference to the
/// field, which we want to avoid since the fields are not `!Unpin`, and would
/// hence be given the `noalias` attribute if we were to do such an access. As
/// an alternative to accessing the fields directly, the `Pointers` type
/// provides getters and setters for the two fields, and those are implemented
/// using `ptr`-specific methods which avoids the creation of intermediate
/// references.
///
/// See this link for more information:
/// <https://github.com/rust-lang/rust/pull/82834>
struct PointersInner<T> {
    /// The previous node in the list. null if there is no previous node.
    prev: Option<NonNull<T>>,

    /// The next node in the list. null if there is no previous node.
    next: Option<NonNull<T>>,

    /// This type is !Unpin due to the heuristic from:
    /// <https://github.com/rust-lang/rust/pull/82834>
    _pin: PhantomPinned,
}

unsafe impl<T: Send> Send for Pointers<T> {}
unsafe impl<T: Sync> Sync for Pointers<T> {}

// ===== impl LinkedList =====

impl<L, T> LinkedList<L, T> {
    /// Creates an empty linked list.
    pub(crate) const fn new() -> LinkedList<L, T> {
        LinkedList {
            head: None,
            tail: None,
            _marker: PhantomData,
        }
    }
}

impl<L: Link> LinkedList<L, L::Target> {
    /// Adds an element first in the list.
    pub(crate) fn push_front(&mut self, val: L::Handle) {
        // The value should not be dropped, it is being inserted into the list
        let val = ManuallyDrop::new(val);
        let ptr = L::as_raw(&val);
        assert_ne!(self.head, Some(ptr));
        unsafe {
            L::pointers(ptr).as_mut().set_next(self.head);
            L::pointers(ptr).as_mut().set_prev(None);

            if let Some(head) = self.head {
                L::pointers(head).as_mut().set_prev(Some(ptr));
            }

            self.head = Some(ptr);

            if self.tail.is_none() {
                self.tail = Some(ptr);
            }
        }
    }

    /// Removes the first element from a list and returns it, or None if it is
    /// empty.
    pub(crate) fn pop_front(&mut self) -> Option<L::Handle> {
        unsafe {
            let head = self.head?;
            self.head = L::pointers(head).as_ref().get_next();

            if let Some(new_head) = L::pointers(head).as_ref().get_next() {
                L::pointers(new_head).as_mut().set_prev(None);
            } else {
                self.tail = None;
            }

            L::pointers(head).as_mut().set_prev(None);
            L::pointers(head).as_mut().set_next(None);

            Some(L::from_raw(head))
        }
    }

    /// Removes the last element from a list and returns it, or None if it is
    /// empty.
    pub(crate) fn pop_back(&mut self) -> Option<L::Handle> {
        unsafe {
            let last = self.tail?;
            self.tail = L::pointers(last).as_ref().get_prev();

            if let Some(prev) = L::pointers(last).as_ref().get_prev() {
                L::pointers(prev).as_mut().set_next(None);
            } else {
                self.head = None;
            }

            L::pointers(last).as_mut().set_prev(None);
            L::pointers(last).as_mut().set_next(None);

            Some(L::from_raw(last))
        }
    }

    /// Returns whether the linked list does not contain any node
    pub(crate) fn is_empty(&self) -> bool {
        if self.head.is_some() {
            return false;
        }

        assert!(self.tail.is_none());
        true
    }

    /// Removes the specified node from the list
    ///
    /// # Safety
    ///
    /// The caller **must** ensure that exactly one of the following is true:
    /// - `node` is currently contained by `self`,
    /// - `node` is not contained by any list,
    /// - `node` is currently contained by some other `GuardedLinkedList` **and**
    ///   the caller has an exclusive access to that list. This condition is
    ///   used by the linked list in `sync::Notify`.
    pub(crate) unsafe fn remove(&mut self, node: NonNull<L::Target>) -> Option<L::Handle> {
        if let Some(prev) = L::pointers(node).as_ref().get_prev() {
            debug_assert_eq!(L::pointers(prev).as_ref().get_next(), Some(node));
            L::pointers(prev)
                .as_mut()
                .set_next(L::pointers(node).as_ref().get_next());
        } else {
            if self.head != Some(node) {
                return None;
            }

            self.head = L::pointers(node).as_ref().get_next();
        }

        if let Some(next) = L::pointers(node).as_ref().get_next() {
            debug_assert_eq!(L::pointers(next).as_ref().get_prev(), Some(node));
            L::pointers(next)
                .as_mut()
                .set_prev(L::pointers(node).as_ref().get_prev());
        } else {
            // This might be the last item in the list
            if self.tail != Some(node) {
                return None;
            }

            self.tail = L::pointers(node).as_ref().get_prev();
        }

        L::pointers(node).as_mut().set_next(None);
        L::pointers(node).as_mut().set_prev(None);

        Some(L::from_raw(node))
    }
}

impl<L: Link> fmt::Debug for LinkedList<L, L::Target> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkedList")
            .field("head", &self.head)
            .field("tail", &self.tail)
            .finish()
    }
}

#[cfg(any(
    feature = "fs",
    feature = "rt",
    all(unix, feature = "process"),
    feature = "signal",
    feature = "sync",
))]
impl<L: Link> LinkedList<L, L::Target> {
    pub(crate) fn last(&self) -> Option<&L::Target> {
        let tail = self.tail.as_ref()?;
        unsafe { Some(&*tail.as_ptr()) }
    }
}

impl<L: Link> Default for LinkedList<L, L::Target> {
    fn default() -> Self {
        Self::new()
    }
}

// ===== impl DrainFilter =====

cfg_io_driver_impl! {
    pub(crate) struct DrainFilter<'a, T: Link, F> {
        list: &'a mut LinkedList<T, T::Target>,
        filter: F,
        curr: Option<NonNull<T::Target>>,
    }

    impl<T: Link> LinkedList<T, T::Target> {
        pub(crate) fn drain_filter<F>(&mut self, filter: F) -> DrainFilter<'_, T, F>
        where
            F: FnMut(&T::Target) -> bool,
        {
            let curr = self.head;
            DrainFilter {
                curr,
                filter,
                list: self,
            }
        }
    }

    impl<'a, T, F> Iterator for DrainFilter<'a, T, F>
    where
        T: Link,
        F: FnMut(&T::Target) -> bool,
    {
        type Item = T::Handle;

        fn next(&mut self) -> Option<Self::Item> {
            while let Some(curr) = self.curr {
                // safety: the pointer references data contained by the list
                self.curr = unsafe { T::pointers(curr).as_ref() }.get_next();

                // safety: the value is still owned by the linked list.
                if (self.filter)(unsafe { &mut *curr.as_ptr() }) {
                    return unsafe { self.list.remove(curr) };
                }
            }

            None
        }
    }
}

cfg_taskdump! {
    impl<T: Link> LinkedList<T, T::Target> {
        pub(crate) fn for_each<F>(&mut self, mut f: F)
        where
            F: FnMut(&T::Handle),
        {
            let mut next = self.head;

            while let Some(curr) = next {
                unsafe {
                    let handle = ManuallyDrop::new(T::from_raw(curr));
                    f(&handle);
                    next = T::pointers(curr).as_ref().get_next();
                }
            }
        }
    }
}

// ===== impl GuardedLinkedList =====

feature! {
    #![any(
        feature = "process",
        feature = "sync",
        feature = "rt",
        feature = "signal",
    )]

    /// An intrusive linked list, but instead of keeping pointers to the head
    /// and tail nodes, it uses a special guard node linked with those nodes.
    /// It means that the list is circular and every pointer of a node from
    /// the list is not `None`, including pointers from the guard node.
    ///
    /// If a list is empty, then both pointers of the guard node are pointing
    /// at the guard node itself.
    pub(crate) struct GuardedLinkedList<L, T> {
        /// Pointer to the guard node.
        guard: NonNull<T>,

        /// Node type marker.
        _marker: PhantomData<*const L>,
    }

    impl<L: Link> LinkedList<L, L::Target> {
        /// Turns a linked list into the guarded version by linking the guard node
        /// with the head and tail nodes. Like with other nodes, you should guarantee
        /// that the guard node is pinned in memory.
        pub(crate) fn into_guarded(self, guard_handle: L::Handle) -> GuardedLinkedList<L, L::Target> {
            // `guard_handle` is a NonNull pointer, we don't have to care about dropping it.
            let guard = L::as_raw(&guard_handle);

            unsafe {
                if let Some(head) = self.head {
                    debug_assert!(L::pointers(head).as_ref().get_prev().is_none());
                    L::pointers(head).as_mut().set_prev(Some(guard));
                    L::pointers(guard).as_mut().set_next(Some(head));

                    // The list is not empty, so the tail cannot be `None`.
                    let tail = self.tail.unwrap();
                    debug_assert!(L::pointers(tail).as_ref().get_next().is_none());
                    L::pointers(tail).as_mut().set_next(Some(guard));
                    L::pointers(guard).as_mut().set_prev(Some(tail));
                } else {
                    // The list is empty.
                    L::pointers(guard).as_mut().set_prev(Some(guard));
                    L::pointers(guard).as_mut().set_next(Some(guard));
                }
            }

            GuardedLinkedList { guard, _marker: PhantomData }
        }
    }

    impl<L: Link> GuardedLinkedList<L, L::Target> {
        fn tail(&self) -> Option<NonNull<L::Target>> {
            let tail_ptr = unsafe {
                L::pointers(self.guard).as_ref().get_prev().unwrap()
            };

            // Compare the tail pointer with the address of the guard node itself.
            // If the guard points at itself, then there are no other nodes and
            // the list is considered empty.
            if tail_ptr != self.guard {
                Some(tail_ptr)
            } else {
                None
            }
        }

        /// Removes the last element from a list and returns it, or None if it is
        /// empty.
        pub(crate) fn pop_back(&mut self) -> Option<L::Handle> {
            unsafe {
                let last = self.tail()?;
                let before_last = L::pointers(last).as_ref().get_prev().unwrap();

                L::pointers(self.guard).as_mut().set_prev(Some(before_last));
                L::pointers(before_last).as_mut().set_next(Some(self.guard));

                L::pointers(last).as_mut().set_prev(None);
                L::pointers(last).as_mut().set_next(None);

                Some(L::from_raw(last))
            }
        }
    }
}

// ===== impl Pointers =====

impl<T> Pointers<T> {
    /// Create a new set of empty pointers
    pub(crate) fn new() -> Pointers<T> {
        Pointers {
            inner: UnsafeCell::new(PointersInner {
                prev: None,
                next: None,
                _pin: PhantomPinned,
            }),
        }
    }

    pub(crate) fn get_prev(&self) -> Option<NonNull<T>> {
        // SAFETY: Field is accessed immutably through a reference.
        unsafe { ptr::addr_of!((*self.inner.get()).prev).read() }
    }
    pub(crate) fn get_next(&self) -> Option<NonNull<T>> {
        // SAFETY: Field is accessed immutably through a reference.
        unsafe { ptr::addr_of!((*self.inner.get()).next).read() }
    }

    fn set_prev(&mut self, value: Option<NonNull<T>>) {
        // SAFETY: Field is accessed mutably through a mutable reference.
        unsafe {
            ptr::addr_of_mut!((*self.inner.get()).prev).write(value);
        }
    }
    fn set_next(&mut self, value: Option<NonNull<T>>) {
        // SAFETY: Field is accessed mutably through a mutable reference.
        unsafe {
            ptr::addr_of_mut!((*self.inner.get()).next).write(value);
        }
    }
}

impl<T> fmt::Debug for Pointers<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prev = self.get_prev();
        let next = self.get_next();
        f.debug_struct("Pointers")
            .field("prev", &prev)
            .field("next", &next)
            .finish()
    }
}


    /// This is a fuzz test. You run it by entering `cargo fuzz run fuzz_linked_list` in CLI in `/tokio/` module.
    #[cfg(fuzzing)]
    pub fn fuzz_linked_list(ops: &[u8]) {
        enum Op {
            Push,
            Pop,
            Remove(usize),
        }
        use std::collections::VecDeque;

        let ops = ops
            .iter()
            .map(|i| match i % 3u8 {
                0 => Op::Push,
                1 => Op::Pop,
                2 => Op::Remove((i / 3u8) as usize),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>();

        let mut ll = LinkedList::<&Entry, <&Entry as Link>::Target>::new();
        let mut reference = VecDeque::new();

        let entries: Vec<_> = (0..ops.len()).map(|i| entry(i as i32)).collect();

        for (i, op) in ops.iter().enumerate() {
            match op {
                Op::Push => {
                    reference.push_front(i as i32);
                    assert_eq!(entries[i].val, i as i32);

                    ll.push_front(entries[i].as_ref());
                }
                Op::Pop => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        continue;
                    }

                    let v = reference.pop_back();
                    assert_eq!(v, ll.pop_back().map(|v| v.val));
                }
                Op::Remove(n) => {
                    if reference.is_empty() {
                        assert!(ll.is_empty());
                        continue;
                    }

                    let idx = n % reference.len();
                    let expect = reference.remove(idx).unwrap();

                    unsafe {
                        let entry = ll.remove(ptr(&entries[expect as usize])).unwrap();
                        assert_eq!(expect, entry.val);
                    }
                }
            }
        }
    }
}
