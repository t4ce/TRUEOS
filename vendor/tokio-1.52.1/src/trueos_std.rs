extern crate alloc;
extern crate self as std;

pub mod any {
    pub use core::any::*;
}

pub mod array {
    pub use core::array::*;
}

pub mod borrow {
    pub use alloc::borrow::*;
}

pub mod boxed {
    pub use alloc::boxed::*;
}

pub mod cell {
    pub use core::cell::*;
}

pub mod cmp {
    pub use core::cmp::*;
}

pub mod collections {
    pub use alloc::collections::*;
}

pub mod convert {
    pub use core::convert::*;
}

pub mod default {
    pub use core::default::*;
}

pub mod error {
    pub use core::error::*;
}

pub mod ffi {
    pub use core::ffi::*;

    pub type OsStr = str;
    pub type OsString = alloc::string::String;
}

pub mod fmt {
    pub use core::fmt::*;
}

pub mod hash {
    pub use core::hash::*;
}

pub mod hint {
    pub use core::hint::*;
}

pub mod iter {
    pub use core::iter::*;
}

pub mod marker {
    pub use core::marker::*;
}

pub mod mem {
    pub use core::mem::*;
}

pub mod num {
    pub use core::num::*;
}

pub mod ops {
    pub use core::ops::*;
}

pub mod option {
    pub use core::option::*;
}

pub mod os {
    pub mod raw {
        pub use core::ffi::{
            c_char, c_double, c_float, c_int, c_long, c_longlong, c_schar, c_short, c_uchar,
            c_uint, c_ulong, c_ulonglong, c_ushort, c_void,
        };
    }
}

pub mod path {
    extern crate alloc;

    use alloc::borrow::ToOwned;
    use alloc::string::{String, ToString};
    use core::borrow::Borrow;
    use core::fmt;
    use core::ops::Deref;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum Component<'a> {
        Prefix(&'a str),
        RootDir,
        CurDir,
        ParentDir,
        Normal(&'a str),
    }

    #[repr(transparent)]
    pub struct Path {
        inner: str,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq, Ord, PartialOrd, Hash)]
    pub struct PathBuf {
        inner: String,
    }

    pub struct Components<'a> {
        path: &'a str,
        pos: usize,
        yielded_root: bool,
    }

    impl Path {
        pub fn new<S: AsRef<str> + ?Sized>(s: &S) -> &Path {
            unsafe { &*(s.as_ref() as *const str as *const Path) }
        }

        pub fn to_str(&self) -> Option<&str> {
            Some(&self.inner)
        }

        pub fn as_os_str(&self) -> &str {
            &self.inner
        }

        pub fn components(&self) -> Components<'_> {
            Components { path: &self.inner, pos: 0, yielded_root: false }
        }

        pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
            let p = path.as_ref();
            if p.inner.starts_with('/') {
                return PathBuf::from(p.inner.to_string());
            }
            let mut out = PathBuf::from(self.inner.to_string());
            out.push(p);
            out
        }
    }

    impl fmt::Debug for Path {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            fmt::Debug::fmt(&&self.inner, f)
        }
    }

    impl PathBuf {
        pub fn new() -> Self {
            Self { inner: String::new() }
        }

        pub fn push<P: AsRef<Path>>(&mut self, path: P) {
            let p = path.as_ref();
            if p.inner.starts_with('/') {
                self.inner.clear();
                self.inner.push_str(&p.inner);
                return;
            }
            if !self.inner.is_empty() && !self.inner.ends_with('/') {
                self.inner.push('/');
            }
            self.inner.push_str(&p.inner);
        }

        pub fn pop(&mut self) -> bool {
            let trimmed = self.inner.trim_end_matches('/');
            if trimmed.is_empty() {
                self.inner.clear();
                return false;
            }
            match trimmed.rfind('/') {
                Some(0) => self.inner.truncate(1),
                Some(pos) => self.inner.truncate(pos),
                None => self.inner.clear(),
            }
            true
        }

        pub fn as_path(&self) -> &Path {
            Path::new(self.inner.as_str())
        }

        pub fn as_os_str(&self) -> &str {
            self.inner.as_str()
        }
    }

    impl From<String> for PathBuf {
        fn from(inner: String) -> Self {
            Self { inner }
        }
    }

    impl From<&str> for PathBuf {
        fn from(value: &str) -> Self {
            Self { inner: value.to_string() }
        }
    }

    impl AsRef<Path> for Path {
        fn as_ref(&self) -> &Path {
            self
        }
    }

    impl AsRef<Path> for PathBuf {
        fn as_ref(&self) -> &Path {
            self.as_path()
        }
    }

    impl AsRef<Path> for str {
        fn as_ref(&self) -> &Path {
            Path::new(self)
        }
    }

    impl AsRef<Path> for String {
        fn as_ref(&self) -> &Path {
            Path::new(self.as_str())
        }
    }

    impl Deref for PathBuf {
        type Target = Path;

        fn deref(&self) -> &Path {
            self.as_path()
        }
    }

    impl Borrow<Path> for PathBuf {
        fn borrow(&self) -> &Path {
            self.as_path()
        }
    }

    impl ToOwned for Path {
        type Owned = PathBuf;

        fn to_owned(&self) -> PathBuf {
            PathBuf::from(self.inner.to_string())
        }
    }

    impl<'a> Iterator for Components<'a> {
        type Item = Component<'a>;

        fn next(&mut self) -> Option<Self::Item> {
            if !self.yielded_root && self.path.starts_with('/') {
                self.yielded_root = true;
                while self.pos < self.path.len() && self.path.as_bytes()[self.pos] == b'/' {
                    self.pos += 1;
                }
                return Some(Component::RootDir);
            }

            while self.pos < self.path.len() {
                while self.pos < self.path.len() && self.path.as_bytes()[self.pos] == b'/' {
                    self.pos += 1;
                }
                if self.pos >= self.path.len() {
                    return None;
                }
                let start = self.pos;
                while self.pos < self.path.len() && self.path.as_bytes()[self.pos] != b'/' {
                    self.pos += 1;
                }
                let seg = &self.path[start..self.pos];
                return Some(match seg {
                    "." => Component::CurDir,
                    ".." => Component::ParentDir,
                    _ => Component::Normal(seg),
                });
            }
            None
        }

        fn size_hint(&self) -> (usize, Option<usize>) {
            (0, Some(self.path.len().saturating_sub(self.pos).saturating_add(1)))
        }
    }

    impl From<PathBuf> for String {
        fn from(value: PathBuf) -> String {
            value.inner
        }
    }

    impl From<&Path> for PathBuf {
        fn from(value: &Path) -> Self {
            value.to_owned()
        }
    }

    impl From<&PathBuf> for PathBuf {
        fn from(value: &PathBuf) -> Self {
            value.clone()
        }
    }
}

pub mod panic {
    pub use core::panic::{AssertUnwindSafe, Location, RefUnwindSafe, UnwindSafe};

    pub fn resume_unwind(_: alloc::boxed::Box<dyn core::any::Any + Send>) -> ! {
        panic!("panic resume is not available on TRUEOS")
    }

    pub fn catch_unwind<F: FnOnce() -> R, R>(f: F) -> Result<R, alloc::boxed::Box<dyn core::any::Any + Send>> {
        Ok(f())
    }
}

pub mod pin {
    pub use core::pin::*;
}

pub mod prelude {
    pub mod rust_2021 {
        pub use alloc::{
            borrow::ToOwned,
            boxed::Box,
            format,
            string::{String, ToString},
            vec,
            vec::Vec,
        };
        pub use core::prelude::rust_2021::*;
    }
}

pub mod ptr {
    pub use core::ptr::*;
}

pub mod rc {
    pub use alloc::rc::*;
}

pub mod result {
    pub use core::result::*;
}

pub mod slice {
    pub use core::slice::*;
}

pub mod str {
    pub use core::str::*;
}

pub mod string {
    pub use alloc::string::*;
}

pub mod thread {
    extern crate alloc;

    use core::fmt;
    use core::time::Duration;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct AccessError;

    #[derive(Clone, Copy, Eq, PartialEq)]
    pub struct ThreadId(usize);

    impl fmt::Debug for ThreadId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_tuple("ThreadId").field(&self.0).finish()
        }
    }

    pub struct Thread {
        id: ThreadId,
    }

    pub type Result<T> = core::result::Result<T, alloc::boxed::Box<dyn core::any::Any + Send>>;

    pub struct Builder;

    pub struct JoinHandle<T> {
        value: Option<T>,
    }

    pub struct LocalKey<T> {
        _marker: core::marker::PhantomData<T>,
    }

    impl Thread {
        pub fn id(&self) -> ThreadId {
            self.id
        }
    }

    pub fn current() -> Thread {
        Thread { id: ThreadId(0) }
    }

    pub fn panicking() -> bool {
        false
    }

    pub fn park() {}

    pub fn park_timeout(_: Duration) {}

    pub fn sleep(_: Duration) {}

    pub fn spawn<F, T>(f: F) -> JoinHandle<T>
    where
        F: FnOnce() -> T,
    {
        JoinHandle { value: Some(f()) }
    }

    impl Builder {
        pub fn new() -> Self {
            Self
        }

        pub fn name(self, _: alloc::string::String) -> Self {
            self
        }

        pub fn spawn<F, T>(self, f: F) -> Result<JoinHandle<T>>
        where
            F: FnOnce() -> T,
        {
            Ok(spawn(f))
        }
    }

    impl<T> JoinHandle<T> {
        pub fn join(mut self) -> Result<T> {
            Ok(self.value.take().expect("thread result already taken"))
        }

        pub fn thread(&self) -> Thread {
            current()
        }
    }
}

pub mod vec {
    pub use alloc::vec::*;
}

#[macro_export]
macro_rules! eprintln {
    ($($tt:tt)*) => {{}};
}
