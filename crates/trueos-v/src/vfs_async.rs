extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::future::{Future, poll_fn};
use core::task::{Context, Poll, Waker};

use crate::vcabi;

pub use trueos_fs::ContentTypeId;

pub const ERR_BAD_UTF8: i32 = -1;
pub const ERR_IO: i32 = -2;
pub const ERR_BAD_PARAM: i32 = -4;
pub const ERR_NOT_FOUND: i32 = -8;
pub const ERR_ALREADY_EXISTS: i32 = -9;
pub const ERR_TYPE_REQUIRED: i32 = -15;
pub const ERR_TYPE_MISMATCH: i32 = -16;

const READ_CHUNK_BYTES: usize = 64 * 1024;
const WRITE_CHUNK_BYTES: usize = 64 * 1024;
const DIR_LIST_MAGIC: [u8; 4] = *b"TDL1";
const DIR_LIST_HEADER_BYTES: usize = 12;
const DIR_LIST_ENTRY_HEADER_BYTES: usize = 4;
const TYPED_DIR_LIST_MAGIC: [u8; 4] = *b"TDL2";

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    File = 1,
    Directory = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Metadata {
    pub kind: NodeKind,
    pub len: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirEntry {
    pub name: String,
    pub kind: NodeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypedMetadata {
    pub kind: NodeKind,
    pub len: u64,
    pub content_type: ContentTypeId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedDirEntry {
    pub name: String,
    pub kind: NodeKind,
    pub content_type: ContentTypeId,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TypedDirListing {
    pub entries: Vec<TypedDirEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DirListing {
    pub entries: Vec<DirEntry>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountInfo {
    /// Selector accepted by every async filesystem operation, for example
    /// `trueosfs:disc3` for the root and `trueosfs:disc3/apps` below it.
    pub selector: String,
    pub label: String,
    pub primary: bool,
    pub read_only: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecordKey {
    Ffa,
    Key {
        provider: [u8; 16],
        handle: [u8; 32],
    },
}

impl Metadata {
    pub const fn is_file(self) -> bool {
        matches!(self.kind, NodeKind::File)
    }

    pub const fn is_dir(self) -> bool {
        matches!(self.kind, NodeKind::Directory)
    }
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

/// List the immediate typed children of a directory.
pub async fn list_dir(path: &[u8]) -> Result<DirListing, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_list_dir_start(path.as_ptr(), path.len())
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
    decode_dir_listing(bytes.as_slice())
}

fn decode_dir_listing(bytes: &[u8]) -> Result<DirListing, i32> {
    if bytes.len() < DIR_LIST_HEADER_BYTES || bytes[..4] != DIR_LIST_MAGIC {
        return Err(ERR_IO);
    }
    if bytes[6..8].iter().any(|byte| *byte != 0) {
        return Err(ERR_IO);
    }
    let flags = bytes[4];
    if flags & !1 != 0 || bytes[5] != 0 {
        return Err(ERR_IO);
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| ERR_IO)?) as usize;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(|_| ERR_IO)?;
    let mut offset = DIR_LIST_HEADER_BYTES;
    for _ in 0..count {
        if bytes.len().saturating_sub(offset) < DIR_LIST_ENTRY_HEADER_BYTES {
            return Err(ERR_IO);
        }
        let kind = match bytes[offset] {
            1 => NodeKind::File,
            2 => NodeKind::Directory,
            _ => return Err(ERR_IO),
        };
        if bytes[offset + 1] != 0 {
            return Err(ERR_IO);
        }
        let name_len = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        offset = offset.saturating_add(DIR_LIST_ENTRY_HEADER_BYTES);
        if name_len == 0 || bytes.len().saturating_sub(offset) < name_len {
            return Err(ERR_IO);
        }
        let name =
            core::str::from_utf8(&bytes[offset..offset + name_len]).map_err(|_| ERR_BAD_UTF8)?;
        if name.contains('/') || name == "." || name == ".." {
            return Err(ERR_IO);
        }
        entries.push(DirEntry {
            name: String::from(name),
            kind,
        });
        offset = offset.saturating_add(name_len);
    }
    if offset != bytes.len() {
        return Err(ERR_IO);
    }
    Ok(DirListing {
        entries,
        truncated: flags & 1 != 0,
    })
}

pub(crate) fn decode_typed_dir_listing(bytes: &[u8]) -> Result<TypedDirListing, i32> {
    if bytes.len() < DIR_LIST_HEADER_BYTES || bytes[..4] != TYPED_DIR_LIST_MAGIC {
        return Err(ERR_IO);
    }
    if bytes[6..8].iter().any(|byte| *byte != 0) || bytes[5] != 0 || bytes[4] & !1 != 0 {
        return Err(ERR_IO);
    }
    let count = u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| ERR_IO)?) as usize;
    let mut entries = Vec::new();
    entries.try_reserve_exact(count).map_err(|_| ERR_IO)?;
    let mut offset = DIR_LIST_HEADER_BYTES;
    for _ in 0..count {
        if bytes.len().saturating_sub(offset) < 8 {
            return Err(ERR_IO);
        }
        let kind = match bytes[offset] {
            1 => NodeKind::File,
            2 => NodeKind::Directory,
            _ => return Err(ERR_IO),
        };
        if bytes[offset + 1] != 0 {
            return Err(ERR_IO);
        }
        let name_len = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]) as usize;
        let content_type = ContentTypeId::from_raw(u32::from_le_bytes(
            bytes[offset + 4..offset + 8]
                .try_into()
                .map_err(|_| ERR_IO)?,
        ));
        offset += 8;
        if name_len == 0 || bytes.len().saturating_sub(offset) < name_len {
            return Err(ERR_IO);
        }
        let name =
            core::str::from_utf8(&bytes[offset..offset + name_len]).map_err(|_| ERR_BAD_UTF8)?;
        if name.contains('/') || name == "." || name == ".." {
            return Err(ERR_IO);
        }
        entries.push(TypedDirEntry {
            name: String::from(name),
            kind,
            content_type,
        });
        offset += name_len;
    }
    if offset != bytes.len() {
        return Err(ERR_IO);
    }
    Ok(TypedDirListing {
        entries,
        truncated: bytes[4] & 1 != 0,
    })
}

/// Enumerate host-granted TRUEOSFS root mounts.
///
/// Standard `async_fs` paths remain mapped to the app and common roots. The
/// host launcher must explicitly grant `fs-scope trueosfs` for this call and
/// for selectors returned by it.
pub async fn list_mounts() -> Result<Vec<MountInfo>, i32> {
    let mut operation =
        Operation::from_start(unsafe { vcabi::trueos_cabi_async_fs_list_mounts_start() })?;
    operation.ready().await?;
    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len < 0 {
        return Err(len as i32);
    }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_async_fs_result_read(operation.id, 0, bytes.as_mut_ptr(), bytes.len())
    };
    if got != bytes.len() as isize {
        return Err(if got < 0 { got as i32 } else { ERR_IO });
    }
    operation.discard();
    let text = String::from_utf8(bytes).map_err(|_| ERR_BAD_UTF8)?;
    parse_mounts(text.as_str())
}

fn parse_mounts(text: &str) -> Result<Vec<MountInfo>, i32> {
    let mut mounts = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        let mut fields = line.split('\t');
        let (Some(selector), Some(label), Some(primary), Some(read_only)) =
            (fields.next(), fields.next(), fields.next(), fields.next())
        else {
            return Err(ERR_IO);
        };
        if fields.next().is_some()
            || !selector.starts_with("trueosfs:disc")
            || !matches!(primary, "0" | "1")
            || !matches!(read_only, "0" | "1")
        {
            return Err(ERR_IO);
        }
        mounts.push(MountInfo {
            selector: String::from(selector),
            label: String::from(label),
            primary: primary == "1",
            read_only: read_only == "1",
        });
    }
    Ok(mounts)
}

#[cfg(test)]
mod mount_tests {
    use super::*;

    #[test]
    fn parses_mount_records() {
        let mounts =
            parse_mounts("trueosfs:disc2\tSystem\t1\t0\ntrueosfs:disc7\tArchive\t0\t1").unwrap();
        assert_eq!(mounts.len(), 2);
        assert!(mounts[0].primary);
        assert!(mounts[1].read_only);
    }

    #[test]
    fn rejects_malformed_mount_records() {
        assert_eq!(parse_mounts("trueosfs:disc2\tSystem\tmaybe\t0"), Err(ERR_IO));
        assert_eq!(parse_mounts("disc2\tSystem\t1\t0"), Err(ERR_IO));
    }

    #[test]
    fn decodes_typed_directory_listing() {
        let mut bytes = Vec::from(*b"TDL1\x01\x00\x00\x00\x02\x00\x00\x00");
        bytes.extend_from_slice(&[2, 0, 4, 0]);
        bytes.extend_from_slice(b"apps");
        bytes.extend_from_slice(&[1, 0, 8, 0]);
        bytes.extend_from_slice(b"boot.log");
        let listing = decode_dir_listing(bytes.as_slice()).unwrap();
        assert!(listing.truncated);
        assert_eq!(listing.entries.len(), 2);
        assert_eq!(listing.entries[0].name, "apps");
        assert_eq!(listing.entries[0].kind, NodeKind::Directory);
        assert_eq!(listing.entries[1].kind, NodeKind::File);
    }

    #[test]
    fn rejects_malformed_directory_listing() {
        assert_eq!(decode_dir_listing(b"apps"), Err(ERR_IO));
        assert_eq!(decode_dir_listing(b"TDL1\0\0\0\0\x01\0\0\0\x02\0\x04\0bad"), Err(ERR_IO));
    }

    #[test]
    fn decodes_tdl2_type_after_name_length() {
        let mut bytes = Vec::from(*b"TDL2\0\0\0\0\x01\0\0\0");
        bytes.extend_from_slice(&[1, 0, 4, 0]);
        bytes.extend_from_slice(&ContentTypeId::BLOB.raw().to_le_bytes());
        bytes.extend_from_slice(b"data");
        let listing = decode_typed_dir_listing(&bytes).unwrap();
        assert_eq!(listing.entries[0].name, "data");
        assert_eq!(listing.entries[0].content_type, ContentTypeId::BLOB);
    }
}

/// Replace a complete file without executing storage work on the Blueprint lane.
pub async fn write_file(path: &[u8], bytes: &[u8]) -> Result<(), i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_write_begin(path.as_ptr(), path.len(), bytes.len())
    })?;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(WRITE_CHUNK_BYTES), bytes.len());
        let status = unsafe {
            vcabi::trueos_cabi_async_fs_write_chunk(
                operation.id,
                offset,
                bytes[offset..end].as_ptr(),
                end - offset,
            )
        };
        if status != 0 {
            return Err(status);
        }
        offset = end;
    }
    let status = unsafe { vcabi::trueos_cabi_async_fs_write_commit(operation.id) };
    if status != 0 {
        return Err(status);
    }
    operation.ready().await?;
    operation.discard();
    Ok(())
}

/// Replace a file while declaring its native content identity.
pub async fn write_file_typed(
    path: &[u8],
    bytes: &[u8],
    content_type: ContentTypeId,
) -> Result<(), i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_typed_write_begin(
            path.as_ptr(),
            path.len(),
            bytes.len(),
            content_type.raw(),
        )
    })?;
    let mut offset = 0usize;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(WRITE_CHUNK_BYTES), bytes.len());
        let status = unsafe {
            vcabi::trueos_cabi_async_fs_write_chunk(
                operation.id,
                offset,
                bytes[offset..end].as_ptr(),
                end - offset,
            )
        };
        if status != 0 {
            return Err(status);
        }
        offset = end;
    }
    let status = unsafe { vcabi::trueos_cabi_async_fs_write_commit(operation.id) };
    if status != 0 {
        return Err(status);
    }
    operation.ready().await?;
    operation.discard();
    Ok(())
}

/// Materialize a directory and every missing parent directory.
pub async fn create_dir_all(path: &[u8]) -> Result<(), i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_create_dir_all_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;
    operation.discard();
    Ok(())
}

/// Read file or directory metadata through the kernel's async TRUEOSFS service.
pub async fn metadata(path: &[u8]) -> Result<Metadata, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_stat_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;

    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len < 0 {
        return Err(len as i32);
    }
    if len != 12 {
        return Err(ERR_IO);
    }
    let mut result = [0u8; 12];
    let got = unsafe {
        vcabi::trueos_cabi_async_fs_result_read(operation.id, 0, result.as_mut_ptr(), result.len())
    };
    if got != result.len() as isize {
        return Err(if got < 0 { got as i32 } else { ERR_IO });
    }
    operation.discard();
    let kind = match u32::from_le_bytes(result[..4].try_into().map_err(|_| ERR_IO)?) {
        1 => NodeKind::File,
        2 => NodeKind::Directory,
        _ => return Err(ERR_IO),
    };
    Ok(Metadata {
        kind,
        len: u64::from_le_bytes(result[4..].try_into().map_err(|_| ERR_IO)?),
    })
}

/// Read metadata including the native on-disk content identity.
pub async fn typed_metadata(path: &[u8]) -> Result<TypedMetadata, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_typed_stat_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;
    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len != 16 {
        return Err(if len < 0 { len as i32 } else { ERR_IO });
    }
    let mut result = [0u8; 16];
    let got = unsafe {
        vcabi::trueos_cabi_async_fs_result_read(operation.id, 0, result.as_mut_ptr(), 16)
    };
    operation.discard();
    if got != 16 {
        return Err(if got < 0 { got as i32 } else { ERR_IO });
    }
    let kind = match u32::from_le_bytes(result[..4].try_into().map_err(|_| ERR_IO)?) {
        1 => NodeKind::File,
        2 => NodeKind::Directory,
        _ => return Err(ERR_IO),
    };
    Ok(TypedMetadata {
        kind,
        len: u64::from_le_bytes(result[4..12].try_into().map_err(|_| ERR_IO)?),
        content_type: ContentTypeId::from_raw(u32::from_le_bytes(
            result[12..16].try_into().map_err(|_| ERR_IO)?,
        )),
    })
}

pub async fn list_dir_typed(path: &[u8]) -> Result<TypedDirListing, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_typed_list_dir_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;
    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len < 0 {
        return Err(len as i32);
    }
    let mut bytes = vec![0u8; len as usize];
    let got = unsafe {
        vcabi::trueos_cabi_async_fs_result_read(operation.id, 0, bytes.as_mut_ptr(), bytes.len())
    };
    operation.discard();
    if got != bytes.len() as isize {
        return Err(if got < 0 { got as i32 } else { ERR_IO });
    }
    decode_typed_dir_listing(&bytes)
}

pub async fn typed_list_dir(path: &[u8]) -> Result<TypedDirListing, i32> {
    list_dir_typed(path).await
}

/// Read the access key persisted in a TRUEOSFS file's on-disk record header.
pub async fn record_key(path: &[u8]) -> Result<RecordKey, i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_record_key_start(path.as_ptr(), path.len())
    })?;
    operation.ready().await?;

    let len = unsafe { vcabi::trueos_cabi_async_fs_result_len(operation.id) };
    if len < 0 {
        return Err(len as i32);
    }
    if len != 56 {
        return Err(ERR_IO);
    }
    let mut result = [0u8; 56];
    let got = unsafe {
        vcabi::trueos_cabi_async_fs_result_read(operation.id, 0, result.as_mut_ptr(), result.len())
    };
    if got != result.len() as isize {
        return Err(if got < 0 { got as i32 } else { ERR_IO });
    }
    operation.discard();
    match result[0] {
        0 if result[1..].iter().all(|byte| *byte == 0) => Ok(RecordKey::Ffa),
        1 if result[1..8].iter().all(|byte| *byte == 0) => {
            let mut provider = [0u8; 16];
            provider.copy_from_slice(&result[8..24]);
            let mut handle = [0u8; 32];
            handle.copy_from_slice(&result[24..56]);
            Ok(RecordKey::Key { provider, handle })
        }
        _ => Err(ERR_IO),
    }
}

pub async fn exists(path: &[u8]) -> Result<bool, i32> {
    match metadata(path).await {
        Ok(_) => Ok(true),
        Err(ERR_NOT_FOUND) => Ok(false),
        Err(error) => Err(error),
    }
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

/// Rename a file or a directory tree through the kernel's native TRUEOSFS service.
///
/// Source and destination must resolve to the same mounted TRUEOSFS root. The
/// operation never overwrites an existing destination.
pub async fn rename(source: &[u8], destination: &[u8]) -> Result<(), i32> {
    let mut operation = Operation::from_start(unsafe {
        vcabi::trueos_cabi_async_fs_rename_start(
            source.as_ptr(),
            source.len(),
            destination.as_ptr(),
            destination.len(),
        )
    })?;
    operation.ready().await?;
    operation.discard();
    Ok(())
}
