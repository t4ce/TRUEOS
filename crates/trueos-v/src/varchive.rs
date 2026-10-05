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
        let _ = crate::vsys::log_record(3, "apps", &alloc::format!("archive-api: phase=start-return value={value}"));
        if value <= 0 {
            Err(if value == 0 { ERR_BAD_PARAM } else { value })
        } else {
            Ok(Self { id: value as u32 })
        }
    }

    async fn finish(&mut self) -> Result<Report, i32> {
        self.finish_with_progress(|_| {}).await
    }

    async fn finish_with_progress(&mut self, mut progress: impl FnMut(u32)) -> Result<Report, i32> {
        self.ready_with_progress(&mut progress).await?;

        let mut raw = TrueosArchiveReport::default();
        let status =
            unsafe { vcabi::trueos_cabi_archive_report(self.id, core::ptr::addr_of_mut!(raw)) };
        let _ = crate::vsys::log_record(3, "apps", &alloc::format!("archive-api: phase=report operation={} status={status} files={} input_bytes={} output_bytes={}", self.id, raw.file_count, raw.input_bytes, raw.output_bytes));
        if status != 0 {
            return Err(status);
        }
        progress(100);
        self.discard();
        Ok(Report {
            input_bytes: raw.input_bytes,
            output_bytes: raw.output_bytes,
            file_count: raw.file_count,
        })
    }

    async fn ready_with_progress(&mut self, mut progress: impl FnMut(u32)) -> Result<(), i32> {
        poll_fn(|cx| {
            let status = unsafe { vcabi::trueos_cabi_archive_status(self.id) };
            match status {
                0 => {
                    let mut raw = TrueosArchiveReport::default();
                    if unsafe { vcabi::trueos_cabi_archive_report(self.id, &mut raw) } == 0 {
                        progress(raw.reserved.min(99));
                    }
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
                1 => Poll::Ready(Ok(())),
                error => {
                    let _ = crate::vsys::log_record(3, "apps", &alloc::format!("archive-api: phase=status-error operation={} code={error}", self.id));
                    Poll::Ready(Err(error))
                }
            }
        })
        .await
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

/// Pack a TRUEOSFS file or directory into a deterministic archive.
/// A destination ending in `.lz4` or `.mz4` selects POSIX tar in a standard
/// LZ4 frame (use `.tar.lz4` or `.tar.mz4`); other destinations retain the 7z encoding. LZ4 uses the
/// kernel GPU block service when available, otherwise the bounded CPU pool.
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

/// Pack several regular TRUEOSFS files into one deterministic archive.
/// Destination format selection is the same as [`pack`].
///
/// `sources` must be non-empty UTF-8 paths without NUL bytes. The kernel
/// performs all source reads before it commits the destination archive.
pub async fn pack_many(sources: &[&[u8]], archive: &[u8]) -> Result<Report, i32> {
    if sources.is_empty()
        || sources
            .iter()
            .any(|source| source.is_empty() || source.contains(&0))
    {
        return Err(ERR_BAD_PARAM);
    }
    let source_bytes = sources
        .iter()
        .enumerate()
        .try_fold(0usize, |total, (index, source)| {
            total
                .checked_add(usize::from(index != 0))
                .and_then(|total| total.checked_add(source.len()))
                .ok_or(ERR_TOO_LARGE)
        })?;
    let total = source_bytes
        .checked_add(archive.len())
        .ok_or(ERR_TOO_LARGE)?;
    let mut encoded = alloc::vec::Vec::new();
    encoded.try_reserve_exact(total).map_err(|_| ERR_NO_SPACE)?;
    for (index, source) in sources.iter().enumerate() {
        if index != 0 {
            encoded.push(0);
        }
        encoded.extend_from_slice(source);
    }
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_archive_pack_many_start(
            encoded.as_ptr(),
            encoded.len(),
            archive.as_ptr(),
            archive.len(),
        )
    })?;
    operation.finish().await
}

/// Unpack a 7z or tar/LZ4 archive into a TRUEOSFS destination directory.
/// The kernel detects the encoding from the file magic, independent of its name.
///
/// The kernel validates archive and path resource caps before extracting. The
/// future resolves only after every output file has been written successfully.
pub async fn unpack(archive: &[u8], destination: &[u8]) -> Result<Report, i32> {
    unpack_with_progress(archive, destination, |_| {}).await
}

/// Report owner-scoped unpack progress on each poll, from 0 to 100.
/// Reading and decoding occupy 0–20%; committed bytes and files occupy 20–99%.
/// 100% is emitted only after successful completion. The caller controls polling cadence.
pub async fn unpack_with_progress(
    archive: &[u8], destination: &[u8], progress: impl FnMut(u32),
) -> Result<Report, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_archive_unpack_start(
            archive.as_ptr(),
            archive.len(),
            destination.as_ptr(),
            destination.len(),
        )
    })?;
    operation.finish_with_progress(progress).await
}

/// Progress for the kernel decode and the subsequent copy into caller RAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryProgress {
    Decoding { percent: u32 },
    Copying { copied: usize, total: usize },
}

/// Ask the kernel codec service to decode one standard LZ4 frame into RAM.
/// No output file or directory is created. GPU/CPU selection and checksum
/// validation are identical to filesystem extraction. The returned buffer is
/// caller-owned, and the kernel operation is discarded on success/error/drop.
/// `max_bytes` bounds the caller allocation; the kernel also enforces its
/// archive limits. This returns the TAR bytes, without parsing the container.
pub async fn decode_lz4_to_memory(archive: &[u8], max_bytes: usize) -> Result<alloc::vec::Vec<u8>, i32> {
    decode_lz4_to_memory_with_progress(archive, max_bytes, |_| {}).await
}

/// Decode progress is sampled on each poll. Copy progress is reported at the
/// start, approximately every MiB, and after the last successful read. The
/// future yields between copy batches so cancellation still releases results.
pub async fn decode_lz4_to_memory_with_progress(
    archive: &[u8], max_bytes: usize, mut progress: impl FnMut(MemoryProgress),
) -> Result<alloc::vec::Vec<u8>, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_archive_lz4_decode_start_v1(archive.as_ptr(), archive.len())
    })?;
    operation.ready_with_progress(|percent| progress(MemoryProgress::Decoding { percent })).await?;
    progress(MemoryProgress::Decoding { percent: 100 });
    let len = unsafe { vcabi::trueos_cabi_archive_result_len_v1(operation.id) };
    if len < 0 { return Err(len as i32); }
    let len = len as usize;
    if len > max_bytes { return Err(ERR_TOO_LARGE); }
    progress(MemoryProgress::Copying { copied: 0, total: len });
    let mut bytes = alloc::vec::Vec::new();
    bytes.try_reserve_exact(len).map_err(|_| ERR_NO_SPACE)?;
    bytes.resize(len, 0);
    let mut offset = 0;
    let mut reported = 0;
    while offset < len {
        let want = (len - offset).min(64 * 1024);
        let got = unsafe {
            vcabi::trueos_cabi_archive_result_read_v1(operation.id, offset, bytes[offset..].as_mut_ptr(), want)
        };
        if got < 0 { return Err(got as i32); }
        if got == 0 || got as usize > want { return Err(ERR_IO); }
        offset += got as usize;
        if offset - reported >= 1024 * 1024 || offset == len {
            progress(MemoryProgress::Copying { copied: offset, total: len });
            reported = offset;
            if offset < len {
                let mut yielded = false;
                poll_fn(|cx| {
                    if yielded { Poll::Ready(()) } else {
                        yielded = true;
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                }).await;
            }
        }
    }
    operation.discard();
    Ok(bytes)
}
