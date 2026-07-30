use core::future::poll_fn;
use core::task::Poll;

use crate::bp_abi::TrueosArchiveReport;
use crate::vcabi;

pub const ERR_BAD_UTF8: i32 = -1;
pub const ERR_IO: i32 = -2;
pub const ERR_NO_SPACE: i32 = -3;
pub const ERR_BAD_PARAM: i32 = -4;
pub const ERR_BAD_PATH: i32 = -6;
pub const ERR_TOO_LARGE: i32 = -7;
pub const ERR_NOT_FOUND: i32 = -8;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub file_count: u32,
}

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

    async fn finish(&mut self) -> Result<Report, i32> {
        poll_fn(|cx| {
            let status = unsafe { vcabi::trueos_cabi_archive_status(self.id) };
            match status {
                0 => {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                1 => Poll::Ready(Ok(())),
                error => Poll::Ready(Err(error)),
            }
        })
        .await?;

        let mut raw = TrueosArchiveReport::default();
        let status =
            unsafe { vcabi::trueos_cabi_archive_report(self.id, core::ptr::addr_of_mut!(raw)) };
        if status != 0 {
            return Err(status);
        }
        self.discard();
        Ok(Report {
            input_bytes: raw.input_bytes,
            output_bytes: raw.output_bytes,
            file_count: raw.file_count,
        })
    }

    fn discard(&mut self) {
        if self.id != 0 {
            let _ = unsafe { vcabi::trueos_cabi_archive_discard(self.id) };
            self.id = 0;
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.discard();
    }
}

/// Pack a TRUEOSFS file or directory into a deterministic 7z archive.
///
/// The future resolves only after the archive has been written successfully.
pub async fn pack(source: &[u8], archive: &[u8]) -> Result<Report, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_archive_pack_start(
            source.as_ptr(),
            source.len(),
            archive.as_ptr(),
            archive.len(),
        )
    })?;
    operation.finish().await
}

/// Unpack a 7z archive into a TRUEOSFS destination directory.
///
/// The kernel validates archive and path resource caps before extracting. The
/// future resolves only after every output file has been written successfully.
pub async fn unpack(archive: &[u8], destination: &[u8]) -> Result<Report, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_archive_unpack_start(
            archive.as_ptr(),
            archive.len(),
            destination.as_ptr(),
            destination.len(),
        )
    })?;
    operation.finish().await
}
