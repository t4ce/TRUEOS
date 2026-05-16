#[cfg(not(target_os = "zkvm"))]
use crate::io::blocking::Blocking;
#[cfg(not(target_os = "zkvm"))]
use crate::io::stdio_common::SplitByUtf8BoundaryIfWindows;
use crate::io::AsyncWrite;

use std::io;
use core::pin::Pin;
use core::task::Context;
use core::task::Poll;

cfg_io_std! {
    /// A handle to the standard error stream of a process.
    ///
    /// Concurrent writes to stderr must be executed with care: Only individual
    /// writes to this [`AsyncWrite`] are guaranteed to be intact. In particular
    /// you should be aware that writes using [`write_all`] are not guaranteed
    /// to occur as a single write, so multiple threads writing data with
    /// [`write_all`] may result in interleaved output.
    ///
    /// Created by the [`stderr`] function.
    ///
    /// [`stderr`]: stderr()
    /// [`AsyncWrite`]: AsyncWrite
    /// [`write_all`]: crate::io::AsyncWriteExt::write_all()
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::io::{self, AsyncWriteExt};
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let mut stderr = io::stderr();
    ///     stderr.write_all(b"Print some error here.").await?;
    ///     Ok(())
    /// }
    /// ```
    #[derive(Debug)]
    #[cfg(not(target_os = "zkvm"))]
    pub struct Stderr {
        std: SplitByUtf8BoundaryIfWindows<Blocking<std::io::Stderr>>,
    }

    #[derive(Debug)]
    #[cfg(target_os = "zkvm")]
    pub struct Stderr {
        _priv: (),
    }

    /// Constructs a new handle to the standard error of the current process.
    ///
    /// The returned handle allows writing to standard error from the within the
    /// Tokio runtime.
    ///
    /// Concurrent writes to stderr must be executed with care: Only individual
    /// writes to this [`AsyncWrite`] are guaranteed to be intact. In particular
    /// you should be aware that writes using [`write_all`] are not guaranteed
    /// to occur as a single write, so multiple threads writing data with
    /// [`write_all`] may result in interleaved output.
    ///
    /// Note that unlike [`std::io::stderr`], each call to this `stderr()`
    /// produces a new writer, so for example, this program does **not** flush stderr:
    ///
    /// ```no_run
    /// # use tokio::io::AsyncWriteExt;
    /// # #[tokio::main]
    /// # async fn main() -> std::io::Result<()> {
    /// tokio::io::stderr().write_all(b"aa").await?;
    /// tokio::io::stderr().flush().await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// [`std::io::stderr`]: std::io::stderr
    /// [`AsyncWrite`]: AsyncWrite
    /// [`write_all`]: crate::io::AsyncWriteExt::write_all()
    ///
    /// # Examples
    ///
    /// ```
    /// use tokio::io::{self, AsyncWriteExt};
    ///
    /// #[tokio::main]
    /// async fn main() -> io::Result<()> {
    ///     let mut stderr = io::stderr();
    ///     stderr.write_all(b"Print some error here.").await?;
    ///     Ok(())
    /// }
    /// ```
    #[cfg(not(target_os = "zkvm"))]
    pub fn stderr() -> Stderr {
        let std = io::stderr();
        // SAFETY: The `Read` implementation of `std` does not read from the
        // buffer it is borrowing and correctly reports the length of the data
        // written into the buffer.
        let blocking = unsafe { Blocking::new(std) };
        Stderr {
            std: SplitByUtf8BoundaryIfWindows::new(blocking),
        }
    }

    #[cfg(target_os = "zkvm")]
    pub fn stderr() -> Stderr {
        Stderr { _priv: () }
    }
}

#[cfg(all(unix, not(target_os = "zkvm")))]
mod sys {
    use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

    use super::Stderr;

    impl AsRawFd for Stderr {
        fn as_raw_fd(&self) -> RawFd {
            std::io::stderr().as_raw_fd()
        }
    }

    impl AsFd for Stderr {
        fn as_fd(&self) -> BorrowedFd<'_> {
            unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
        }
    }
}

cfg_windows! {
    use crate::os::windows::io::{AsHandle, BorrowedHandle, AsRawHandle, RawHandle};

    impl AsRawHandle for Stderr {
        fn as_raw_handle(&self) -> RawHandle {
            std::io::stderr().as_raw_handle()
        }
    }

    impl AsHandle for Stderr {
        fn as_handle(&self) -> BorrowedHandle<'_> {
            unsafe { BorrowedHandle::borrow_raw(self.as_raw_handle()) }
        }
    }
}

impl AsyncWrite for Stderr {
    #[cfg_attr(target_os = "zkvm", allow(unused_mut))]
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        #[cfg(target_os = "zkvm")]
        {
            let _ = (self, cx);
            return Poll::Ready(Ok(buf.len()));
        }
        #[cfg(not(target_os = "zkvm"))]
        Pin::new(&mut self.std).poll_write(cx, buf)
    }

    #[cfg_attr(target_os = "zkvm", allow(unused_mut))]
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), io::Error>> {
        #[cfg(target_os = "zkvm")]
        {
            let _ = (self, cx);
            return Poll::Ready(Ok(()));
        }
        #[cfg(not(target_os = "zkvm"))]
        Pin::new(&mut self.std).poll_flush(cx)
    }

    #[cfg_attr(target_os = "zkvm", allow(unused_mut))]
    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), io::Error>> {
        #[cfg(target_os = "zkvm")]
        {
            let _ = (self, cx);
            return Poll::Ready(Ok(()));
        }
        #[cfg(not(target_os = "zkvm"))]
        Pin::new(&mut self.std).poll_shutdown(cx)
    }
}
