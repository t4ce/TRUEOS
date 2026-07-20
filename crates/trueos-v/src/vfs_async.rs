extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::future::{Future, poll_fn};
use core::task::{Context, Poll, Waker};

use crate::vcabi;

pub const ERR_BAD_UTF8: i32 = -1;
pub const ERR_IO: i32 = -2;
pub const ERR_BAD_PARAM: i32 = -4;
pub const ERR_NOT_FOUND: i32 = -8;

const READ_CHUNK_BYTES: usize = 64 * 1024;

struct Operation {
    id: u32,
}

impl Operation {
    fn from_start(value: i32) -> Result<Self, i32> {
        if value <= 0 {
            Err(if value == 0 { ERR_BAD_PARAM } else { value })
        } else {
            Ok(Self { id: value as u32 })
        }
    }

    async fn ready(&self) -> Result<(), i32> {
        poll_fn(|cx| {
            let status = unsafe { vcabi::trueos_cabi_async_fs_status(self.id) };
            match status {
                0 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                1 => Poll::Ready(Ok(())),
                error => Poll::Ready(Err(error)),
            }
        })
        .await
    }

    fn discard(&mut self) {
        if self.id != 0 {
            let _ = unsafe { vcabi::trueos_cabi_async_fs_discard(self.id) };
            self.id = 0;
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.discard();
    }
}

/// Run one cooperative future on the calling Blueprint lane.
///
/// Pending filesystem operations only poll lightweight operation state; this
/// yield lets the kernel's native async TRUEOSFS service make progress.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = core::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(output) => return output,
            Poll::Pending => crate::vsys::poll_once(),
        }
    }
}

/// Read a complete file without executing storage work on the Blueprint lane.
pub async fn read_file(path: &[u8]) -> Result<Vec<u8>, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_read_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;

    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len < 0 {
        return Err(len as i32);
    }
    let len = len as usize;
    let mut bytes = vec![0u8; len];
    let mut offset = 0usize;
    while offset < len {
        let end = core::cmp::min(offset.saturating_add(READ_CHUNK_BYTES), len);
        let got = unsafe {
            vcabi::trueos_cabi_async_fs_result_read(
                operation.id,
                offset,
                bytes[offset..end].as_mut_ptr(),
                end - offset,
            )
        };
        if got < 0 {
            return Err(got as i32);
        }
        if got == 0 {
            return Err(ERR_IO);
        }
        offset = offset.saturating_add(got as usize);
    }
    operation.discard();
    Ok(bytes)
}

pub async fn read_file_utf8(path: &[u8]) -> Result<String, i32> {
    String::from_utf8(read_file(path).await?).map_err(|_| ERR_BAD_UTF8)
}

/// Remove a file through the kernel's native async TRUEOSFS service.
pub async fn remove(path: &[u8]) -> Result<(), i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_remove_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;
    operation.discard();
    Ok(())
}
