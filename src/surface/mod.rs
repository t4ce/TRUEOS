pub use ::core as Core;
pub use ::alloc as Alloc;

macro_rules! surface_reexport {
    ($name:ident => $path:path) => {
        pub mod $name {
            pub use $path::*;
        }
    };
}

surface_reexport!(fmt => ::core::fmt);
surface_reexport!(mem => ::core::mem);
surface_reexport!(cmp => ::core::cmp);
surface_reexport!(ops => ::core::ops);
surface_reexport!(ptr => ::core::ptr);
surface_reexport!(slice => ::core::slice);
surface_reexport!(str => ::core::str);
surface_reexport!(borrow => ::core::borrow);
surface_reexport!(hash => ::core::hash);
surface_reexport!(marker => ::core::marker);
surface_reexport!(convert => ::core::convert);
surface_reexport!(default => ::core::default);
surface_reexport!(iter => ::core::iter);
surface_reexport!(num => ::core::num);
surface_reexport!(option => ::core::option);
surface_reexport!(result => ::core::result);
surface_reexport!(vec => ::alloc::vec);
surface_reexport!(string => ::alloc::string);
surface_reexport!(boxed => ::alloc::boxed);

pub mod io;

pub mod time {
    pub use crate::time::*;
}

pub mod rand {
    pub use crate::rng::*;
}

pub mod collections {
    pub use heapless::*;
    pub use ::alloc::collections::{BinaryHeap, BTreeMap, BTreeSet, LinkedList, VecDeque};
    pub use hashbrown::{HashMap, HashSet};
}
