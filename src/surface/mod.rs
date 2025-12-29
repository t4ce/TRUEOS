pub use ::core as Core;
pub use ::alloc as alloc;

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
surface_reexport!(ascii => ::core::ascii);
surface_reexport!(ffi => ::core::ffi);
surface_reexport!(arch => ::core::arch);
surface_reexport!(any => ::core::any);
surface_reexport!(error => ::core::error);
surface_reexport!(iter => ::core::iter);
surface_reexport!(num => ::core::num);
surface_reexport!(f16 => ::core::f16); // this would influence
surface_reexport!(f32 => ::core::f32); // threads
surface_reexport!(f64 => ::core::f64); // if we had preemtion (or threads)
surface_reexport!(f128 => ::core::f128);
surface_reexport!(future => ::core::future);
surface_reexport!(task => ::core::task);
surface_reexport!(option => ::core::option);
surface_reexport!(panic => ::core::panic);
surface_reexport!(backtrace => crate::backtrace);
surface_reexport!(result => ::core::result);
surface_reexport!(vec => ::alloc::vec);
surface_reexport!(string => ::alloc::string);
surface_reexport!(boxed => ::alloc::boxed);

pub mod env {
    use crate::surface::string::String;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum VarError {
        NotPresent,
        NotUnicode,
        Unsupported,
    }

    pub fn var(_key: &str) -> Result<String, VarError> {
        Err(VarError::Unsupported)
    }
}

pub mod prelude {
    pub use crate::surface::boxed::Box;
    pub use crate::surface::fmt::{Debug, Display};
    pub use crate::surface::option::Option;
    pub use crate::surface::result::Result;
    pub use crate::surface::string::{String, ToString};
    pub use crate::surface::vec::Vec;
}

pub mod sync {
    pub use core::sync::atomic::{
        compiler_fence, fence, AtomicBool, AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicIsize,
        AtomicPtr, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering,
    };
    pub use alloc::sync::{Arc, Weak};
    pub use spin::{Mutex, MutexGuard, Once, RwLock, RwLockReadGuard, RwLockWriteGuard};
}

pub mod io;

pub mod time {
    pub use crate::time::*;
}

pub mod random {
    pub use crate::rng::*;
}

pub use random as rand;

pub mod collections {
    pub use heapless::*;
    pub use ::alloc::collections::{BinaryHeap, BTreeMap, BTreeSet, LinkedList, VecDeque};
    pub use hashbrown::{HashMap, HashSet};
}

pub mod unicode {
    #[cfg(feature = "surface-unicode-segmentation")]
    pub use unicode_segmentation as segmentation;

    #[cfg(feature = "surface-unicode-normalization")]
    pub use unicode_normalization as normalization;
}
