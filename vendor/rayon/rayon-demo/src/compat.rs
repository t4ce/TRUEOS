#[cfg(not(target_os = "trueos"))]
pub mod env {
    pub use std::env::*;
}

#[cfg(target_os = "trueos")]
pub mod env {
    use alloc::{string::String, vec::Vec};

    pub fn args() -> alloc::vec::IntoIter<String> {
        Vec::new().into_iter()
    }
}

#[cfg(not(target_os = "trueos"))]
pub mod fs {
    pub use std::fs::*;
}

#[cfg(target_os = "trueos")]
pub mod fs {
    use crate::compat::{io, path::Path};

    pub struct File;

    impl File {
        pub fn open<P: AsRef<Path>>(_path: P) -> io::Result<Self> {
            Err(io::Error::new(
                io::ErrorKind::Uncategorized,
                "rayon-demo host file access unavailable on TRUEOS",
            ))
        }
    }

    impl io::Read for File {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::Uncategorized,
                "rayon-demo host file access unavailable on TRUEOS",
            ))
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub mod io {
    pub use std::io::*;
}

#[cfg(target_os = "trueos")]
pub mod io {
    pub use core3::io::*;

    pub mod prelude {
        pub use core3::io::{BufRead, Read, Seek, Write};
    }

    pub struct Stderr;
    pub struct Stdin;

    pub fn stderr() -> Stderr {
        Stderr
    }

    pub fn stdin() -> Stdin {
        Stdin
    }

    impl Write for Stderr {
        fn write(&mut self, buf: &[u8]) -> Result<usize> {
            Ok(buf.len())
        }

        fn flush(&mut self) -> Result<()> {
            Ok(())
        }
    }

    impl Stdin {
        pub fn read_line(&mut self, _buf: &mut alloc::string::String) -> Result<usize> {
            Err(Error::new(ErrorKind::Uncategorized, "rayon-demo stdin unavailable on TRUEOS"))
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub mod path {
    pub use std::path::*;
}

#[cfg(target_os = "trueos")]
pub mod path {
    use core::{fmt, ops::Deref};

    #[derive(Debug)]
    #[repr(transparent)]
    pub struct Path {
        inner: str,
    }

    impl Path {
        pub fn new<S: AsRef<str> + ?Sized>(path: &S) -> &Self {
            // SAFETY: Path is transparent over str.
            unsafe { &*(path.as_ref() as *const str as *const Self) }
        }

        pub fn display(&self) -> Display<'_> {
            Display(self)
        }

        pub fn as_str(&self) -> &str {
            &self.inner
        }
    }

    impl AsRef<Path> for Path {
        fn as_ref(&self) -> &Path {
            self
        }
    }

    impl Deref for Path {
        type Target = str;

        fn deref(&self) -> &str {
            self.as_str()
        }
    }

    pub struct Display<'a>(&'a Path);

    impl fmt::Display for Display<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0.as_str())
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub mod process {
    pub use std::process::*;
}

#[cfg(target_os = "trueos")]
pub mod process {
    pub fn exit(_code: i32) -> ! {
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(not(target_os = "trueos"))]
pub mod time {
    pub use std::time::*;
}

#[cfg(target_os = "trueos")]
pub mod time {
    use core::ops::{Add, AddAssign, Sub};

    pub use core::time::Duration;

    unsafe extern "Rust" {
        fn trueos_platform_monotonic_nanos() -> u64;
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
    pub struct Instant(Duration);

    impl Instant {
        pub fn now() -> Self {
            Self(Duration::from_nanos(unsafe { trueos_platform_monotonic_nanos() }))
        }

        pub fn elapsed(self) -> Duration {
            Self::now() - self
        }
    }

    impl Add<Duration> for Instant {
        type Output = Instant;

        fn add(self, rhs: Duration) -> Instant {
            Instant(self.0 + rhs)
        }
    }

    impl AddAssign<Duration> for Instant {
        fn add_assign(&mut self, rhs: Duration) {
            *self = *self + rhs;
        }
    }

    impl Sub<Instant> for Instant {
        type Output = Duration;

        fn sub(self, rhs: Instant) -> Duration {
            self.0
                .checked_sub(rhs.0)
                .unwrap_or_else(|| Duration::from_nanos(0))
        }
    }
}
