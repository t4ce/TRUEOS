use alloc::{boxed::Box, string::{String, ToString}, vec, vec::Vec};
use alloc::collections::BTreeSet;

use crate::disc::{block, partition};
use core::hint::spin_loop;
use core::sync::atomic::{AtomicU32, Ordering};
use sha2::{Digest, Sha256};
use spin::Mutex;
use trueos_math::Tree;

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TrueosFsPlacement {
    pub bootable: bool,
    pub super_lba: u64,
    pub data_lba: u64,
    pub data_end_lba_exclusive: Option<u64>,
}

/// TRUEOSFS per-disk root filetree cache.
///
/// This is intentionally limited in scope: it is *not* a page cache.
/// It is meant to memoize directory/file listings once we implement them.
const TRUEOSFS_TREE_CAP: usize = 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrueosFsTreeKind {
    Root,
    Dir,
    File,
    /// Represents a nested TRUEOSFS inside the tree (future mountpoint concept).
    TrueosFs,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrueosFsTreeEntry {
    pub kind: TrueosFsTreeKind,
    pub name: String,
}

pub type TrueosFsTree = Tree<TrueosFsTreeEntry, TRUEOSFS_TREE_CAP>;

struct RootMount {
    disk_id: block::DiscId,
    placement: TrueosFsPlacement,
    seq: u32,
    tree: Option<Box<TrueosFsTree>>,
}

static ROOT_SEQ: AtomicU32 = AtomicU32::new(0);
static ROOTS: Mutex<Vec<RootMount>> = Mutex::new(Vec::new());

// --- On-disk log format (simple, append-only) ---

const LOG_ENTRY_MAGIC: [u8; 8] = *b"TOSFLOG\0";
const LOG_ENTRY_VERSION: u16 = 1;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LogKind {
    Put = 1,
    Delete = 2,
}

#[derive(Clone, Copy, Debug)]
struct LogHeader {
    kind: LogKind,
    committed: bool,
    name_len: u16,
    data_len: u64,
    sha256: [u8; 32],
}

impl LogHeader {
    fn encode_into_block(&self, block: &mut [u8]) {
        // Header is stored in the first block of the entry. The remainder of the
        // block is left as-is (callers typically zero-fill it).
        if block.len() < 64 {
            return;
        }
        block[0..8].copy_from_slice(&LOG_ENTRY_MAGIC);
        block[8..10].copy_from_slice(&LOG_ENTRY_VERSION.to_le_bytes());
        block[10] = self.kind as u8;
        block[11] = if self.committed { 1 } else { 0 };
        block[12..14].copy_from_slice(&self.name_len.to_le_bytes());
        block[14..16].copy_from_slice(&0u16.to_le_bytes());
        block[16..24].copy_from_slice(&self.data_len.to_le_bytes());
        block[24..56].copy_from_slice(&self.sha256);
        // [56..64] reserved
        for b in block[56..64].iter_mut() {
            *b = 0;
        }
    }

    fn decode_from_block(block: &[u8]) -> Option<Self> {
        if block.len() < 64 {
            return None;
        }
        if &block[0..8] != &LOG_ENTRY_MAGIC {
            return None;
        }
        let ver = u16::from_le_bytes([block[8], block[9]]);
        if ver != LOG_ENTRY_VERSION {
            return None;
        }
        let kind = match block[10] {
            1 => LogKind::Put,
            2 => LogKind::Delete,
            _ => return None,
        };
        let committed = block[11] == 1;
        let name_len = u16::from_le_bytes([block[12], block[13]]);
        let data_len = u64::from_le_bytes([
            block[16], block[17], block[18], block[19], block[20], block[21], block[22], block[23],
        ]);
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&block[24..56]);
        Some(Self {
            kind,
            committed,
            name_len,
            data_len,
            sha256,
        })
    }
}

#[derive(Clone, Debug)]
struct FileRecord {
    entry_lba: u64,
    name_len: u16,
    data_len: u64,
    data_lba: u64,
    sha256: [u8; 32],
}

fn entry_blocks(block_size: usize, name_len: usize, data_len: usize) -> u64 {
    if block_size == 0 {
        return 0;
    }
    let name_blocks = (name_len + (block_size - 1)) / block_size;
    let data_blocks = (data_len + (block_size - 1)) / block_size;
    (1 + name_blocks + data_blocks) as u64
}

fn disk_data_end_lba_exclusive(disk: block::DeviceHandle, placement: &TrueosFsPlacement) -> u64 {
    placement
        .data_end_lba_exclusive
        .unwrap_or_else(|| disk.info().block_count)
}

fn read_one_block_aligned(handle: block::DeviceHandle, lba: u64) -> Result<Vec<u8>, block::Error> {
    read_blocks_aligned_retry(handle, lba, 1, 3)
}

fn write_blocks_aligned_chunked(handle: block::DeviceHandle, start_lba: u64, buf: &[u8]) -> Result<(), block::Error> {
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 || buf.is_empty() {
        return Err(block::Error::InvalidParam);
    }
    if buf.len() % bs != 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let align = info.dma_alignment.max(1) as usize;

    let mut lba = start_lba;
    let mut off = 0usize;
    while off < buf.len() {
        let remaining = buf.len() - off;
        let blocks_here = core::cmp::min(max_blocks, remaining / bs);
        let bytes_here = blocks_here * bs;

        let mut tmp = AlignedBuf::new(bytes_here, align).ok_or(block::Error::DmaUnavailable)?;
        tmp.as_mut_slice().copy_from_slice(&buf[off..off + bytes_here]);
        handle.write_blocks(lba, tmp.as_mut_slice())?;

        lba = lba.saturating_add(blocks_here as u64);
        off = off.saturating_add(bytes_here);

        // Keep the system responsive.
        crate::time::poll_executor();
    }

    Ok(())
}

fn write_stream_at_lba(
    handle: block::DeviceHandle,
    start_lba: u64,
    exact_bytes: usize,
    total_bytes_rounded: usize,
    mut source: impl FnMut(&mut [u8]) -> Result<usize, block::Error>,
) -> Result<(), block::Error> {
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    if total_bytes_rounded == 0 || total_bytes_rounded % bs != 0 {
        return Err(block::Error::InvalidParam);
    }
    if exact_bytes > total_bytes_rounded {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let align = info.dma_alignment.max(1) as usize;

    let mut lba = start_lba;
    let mut written = 0usize;
    let mut written_exact = 0usize;
    while written < total_bytes_rounded {
        let remaining = total_bytes_rounded - written;
        let blocks_here = core::cmp::min(max_blocks, remaining / bs);
        let bytes_here = blocks_here * bs;

        let mut tmp = AlignedBuf::new(bytes_here, align).ok_or(block::Error::DmaUnavailable)?;
        let chunk = tmp.as_mut_slice();
        chunk.fill(0);

        let mut filled = 0usize;
        while filled < bytes_here {
            if written_exact >= exact_bytes {
                break;
            }
            let remaining_exact = exact_bytes - written_exact;
            let want = core::cmp::min(bytes_here - filled, remaining_exact);
            let n = source(&mut chunk[filled..filled + want])?;
            if n == 0 {
                return Err(block::Error::Corrupted);
            }
            filled = filled.saturating_add(n);
            written_exact = written_exact.saturating_add(n);
        }

        handle.write_blocks(lba, chunk)?;
        lba = lba.saturating_add(blocks_here as u64);
        written = written.saturating_add(bytes_here);

        crate::time::poll_executor();
    }
    if written_exact != exact_bytes {
        return Err(block::Error::Corrupted);
    }
    Ok(())
}

fn read_exact_bytes(
    handle: block::DeviceHandle,
    start_lba: u64,
    start_byte_off: usize,
    out: &mut [u8],
) -> Result<(), block::Error> {
    if out.is_empty() {
        return Ok(());
    }
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    let align = info.dma_alignment.max(1) as usize;

    let mut tmp = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    let scratch = tmp.as_mut_slice();
    scratch.fill(0);

    let mut remaining = out;
    let mut abs_byte = start_byte_off;
    while !remaining.is_empty() {
        let lba = start_lba.saturating_add((abs_byte / bs) as u64);
        let off = abs_byte % bs;
        handle.read_blocks(lba, scratch)?;
        let take = core::cmp::min(bs - off, remaining.len());
        remaining[..take].copy_from_slice(&scratch[off..off + take]);
        remaining = &mut remaining[take..];
        abs_byte = abs_byte.saturating_add(take);
        crate::time::poll_executor();
    }
    Ok(())
}

fn compute_sha256_of_entry_data(
    handle: block::DeviceHandle,
    rec: &FileRecord,
) -> Result<[u8; 32], block::Error> {
    let mut hasher = Sha256::new();
    let mut remaining = rec.data_len as usize;
    let info = handle.info();
    let bs = info.block_size as usize;
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    let scratch = tmp.as_mut_slice();
    let mut pos = 0usize;
    while remaining > 0 {
        let lba = rec.data_lba.saturating_add((pos / bs) as u64);
        let off = pos % bs;
        handle.read_blocks(lba, scratch)?;
        let take = core::cmp::min(bs - off, remaining);
        hasher.update(&scratch[off..off + take]);
        remaining = remaining.saturating_sub(take);
        pos = pos.saturating_add(take);
        crate::time::poll_executor();
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..]);
    Ok(out)
}

fn find_latest_record(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    name: &str,
) -> Result<Option<FileRecord>, block::Error> {
    if name.is_empty() {
        return Ok(None);
    }
    let name_bytes = name.as_bytes();
    if name_bytes.len() > (u16::MAX as usize) {
        return Ok(None);
    }

    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let sb_block = read_one_block_aligned(disk, placement.super_lba)?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };
    let mut lba = placement.data_lba;
    let end_lba = placement.data_lba.saturating_add(sb.log_head_rel_blocks);
    let mut latest: Option<FileRecord> = None;

    while lba < end_lba {
        let hdr_block = read_one_block_aligned(disk, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        // Basic sanity caps.
        if name_len == 0 || name_len > 4096 {
            break;
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);
        let name_lba = lba.saturating_add(1);

        // Compare name (exact bytes only; padding is ignored).
        if name_len == name_bytes.len() {
            let mut tmp_name = vec![0u8; name_len];
            read_exact_bytes(disk, name_lba, 0, &mut tmp_name)?;
            if tmp_name == name_bytes {
                match hdr.kind {
                    LogKind::Put => {
                        let data_lba = lba.saturating_add(1).saturating_add(name_blocks as u64);
                        latest = Some(FileRecord {
                            entry_lba: lba,
                            name_len: hdr.name_len,
                            data_len: hdr.data_len,
                            data_lba,
                            sha256: hdr.sha256,
                        });
                    }
                    LogKind::Delete => {
                        latest = None;
                    }
                }
            }
        }

        lba = lba.saturating_add(blocks);
    }

    Ok(latest)
}

fn check_space_for_put(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    name_len: usize,
    data_len: usize,
) -> Result<Option<(trueos_fs::Superblock, u64, u64, usize)>, block::Error> {
    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let sb_block = read_one_block_aligned(disk, placement.super_lba)?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };

    // We store header in its own full block.
    let total_blocks = entry_blocks(bs, name_len, data_len);
    let entry_lba = placement.data_lba.saturating_add(sb.log_head_rel_blocks);
    let end_lba = disk_data_end_lba_exclusive(disk, placement);
    if entry_lba.saturating_add(total_blocks) > end_lba {
        return Ok(None);
    }

    let total_bytes_rounded = (total_blocks as usize).saturating_mul(bs);
    Ok(Some((sb, entry_lba, total_blocks, total_bytes_rounded)))
}

fn write_put_entry(
    disk: block::DeviceHandle,
    _placement: &TrueosFsPlacement,
    entry_lba: u64,
    total_blocks: u64,
    name: &str,
    data_source: &mut dyn FnMut(&mut [u8]) -> Result<usize, block::Error>,
    data_len: u64,
) -> Result<(), block::Error> {
    let info = disk.info();
    let bs = info.block_size as usize;
    let align = info.dma_alignment.max(1) as usize;

    let name_len = name.as_bytes().len();
    let name_blocks = (name_len + (bs - 1)) / bs;
    let name_bytes_rounded = name_blocks * bs;
    let data_blocks = ((data_len as usize) + (bs - 1)) / bs;
    let data_bytes_rounded = data_blocks * bs;
    let expected_blocks = 1u64
        .saturating_add(name_blocks as u64)
        .saturating_add(data_blocks as u64);
    if expected_blocks != total_blocks {
        return Err(block::Error::InvalidParam);
    }

    // 1) Write header block with committed=0 and sha=0.
    let mut hdr0 = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    hdr0.as_mut_slice().fill(0);
    LogHeader {
        kind: LogKind::Put,
        committed: false,
        name_len: name_len as u16,
        data_len,
        sha256: [0u8; 32],
    }
    .encode_into_block(hdr0.as_mut_slice());
    disk.write_blocks(entry_lba, hdr0.as_mut_slice())?;

    // 2) Write name blocks, padded with zeros.
    let mut name_buf = AlignedBuf::new(name_bytes_rounded, align).ok_or(block::Error::DmaUnavailable)?;
    name_buf.as_mut_slice().fill(0);
    name_buf.as_mut_slice()[..name_len].copy_from_slice(name.as_bytes());
    if name_bytes_rounded > 0 {
        write_blocks_aligned_chunked(disk, entry_lba.saturating_add(1), name_buf.as_mut_slice())?;
    }

    // 3) Stream data blocks, padded with zeros, while hashing.
    let mut hasher = Sha256::new();
    if data_bytes_rounded > 0 {
        let mut remaining = data_len as usize;
        let mut data_src = |dst: &mut [u8]| -> Result<usize, block::Error> {
            if remaining == 0 {
                return Ok(0);
            }
            let want = core::cmp::min(dst.len(), remaining);
            let got = data_source(&mut dst[..want])?;
            if got == 0 {
                return Err(block::Error::Corrupted);
            }
            hasher.update(&dst[..got]);
            remaining = remaining.saturating_sub(got);
            Ok(got)
        };

        write_stream_at_lba(
            disk,
            entry_lba
                .saturating_add(1)
                .saturating_add(name_blocks as u64),
            data_len as usize,
            data_bytes_rounded,
            &mut data_src,
        )?;
    }
    disk.flush()?;

    // 4) Rewrite header block with committed=1 and real sha.
    let digest = hasher.finalize();
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&digest[..]);
    let mut hdr1 = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    hdr1.as_mut_slice().fill(0);
    LogHeader {
        kind: LogKind::Put,
        committed: true,
        name_len: name_len as u16,
        data_len,
        sha256,
    }
    .encode_into_block(hdr1.as_mut_slice());
    disk.write_blocks(entry_lba, hdr1.as_mut_slice())?;
    disk.flush()?;

    Ok(())
}

fn write_delete_entry(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    entry_lba: u64,
    name: &str,
) -> Result<u64, block::Error> {
    let info = disk.info();
    let bs = info.block_size as usize;
    let align = info.dma_alignment.max(1) as usize;
    let name_len = name.len();

    let blocks = entry_blocks(bs, name_len, 0);

    // Header block committed immediately (no payload integrity needed).
    let mut hdr = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    hdr.as_mut_slice().fill(0);
    LogHeader {
        kind: LogKind::Delete,
        committed: true,
        name_len: name_len as u16,
        data_len: 0,
        sha256: [0u8; 32],
    }
    .encode_into_block(hdr.as_mut_slice());
    disk.write_blocks(entry_lba, hdr.as_mut_slice())?;

    // Write name bytes after header.
    let name_bytes = name.as_bytes();
    let name_blocks = (name_bytes.len() + (bs - 1)) / bs;
    let payload_bytes_rounded = name_blocks * bs;
    let mut off = 0usize;
    let mut src = |dst: &mut [u8]| -> Result<usize, block::Error> {
        if off >= name_bytes.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), name_bytes.len() - off);
        dst[..take].copy_from_slice(&name_bytes[off..off + take]);
        off = off.saturating_add(take);
        Ok(take)
    };
    if payload_bytes_rounded > 0 {
        write_stream_at_lba(
            disk,
            entry_lba.saturating_add(1),
            name_bytes.len(),
            payload_bytes_rounded,
            &mut src,
        )?;
    }
    disk.flush()?;

    let _ = placement;
    Ok(blocks)
}

fn advance_log_head(
    disk: block::DeviceHandle,
    placement: &TrueosFsPlacement,
    mut sb: trueos_fs::Superblock,
    delta_blocks: u64,
) -> Result<(), block::Error> {
    sb.log_head_rel_blocks = sb.log_head_rel_blocks.saturating_add(delta_blocks);
    let info = disk.info();
    let bs = info.block_size as usize;
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    tmp.as_mut_slice().fill(0);
    trueos_fs::write_superblock(tmp.as_mut_slice(), sb);
    disk.write_blocks(placement.super_lba, tmp.as_mut_slice())?;
    disk.flush()?;
    Ok(())
}

/// Ensure a single TRUEOSFS root exists for this *whole disk*.
///
/// Returns:
/// - `Ok(Some(disk_id))` if the disk contains TRUEOSFS and is now registered
/// - `Ok(None)` if the disk does not contain TRUEOSFS
/// - `Err(_)` on I/O or invalid param
pub fn mount_root(disk: block::DeviceHandle) -> Result<Option<block::DiscId>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }

    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };

    let disk_id = disk.id();

    let mut roots = ROOTS.lock();
    if roots.iter().any(|m| m.disk_id == disk_id) {
        return Ok(Some(disk_id));
    }

    // Seed an initial tree skeleton. Directory enumeration will fill this later.
    let mut tree = TrueosFsTree::new();
    let root = match tree.add_root(TrueosFsTreeEntry {
        kind: TrueosFsTreeKind::Root,
        name: String::from("trueosfs"),
    }) {
        Some(id) => id,
        None => {
            // Capacity too small; still register without a cache.
            roots.push(RootMount {
                disk_id,
                placement,
                seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
                tree: None,
            });
            return Ok(Some(disk_id));
        }
    };
    let _ = tree.add_child(
        root,
        TrueosFsTreeEntry {
            kind: TrueosFsTreeKind::Dir,
            name: String::from("/"),
        },
    );

    roots.push(RootMount {
        disk_id,
        placement,
        seq: ROOT_SEQ.fetch_add(1, Ordering::AcqRel).wrapping_add(1),
        tree: Some(Box::new(tree)),
    });

    Ok(Some(disk_id))
}

/// TRUEOSFS: write/replace a file.
///
/// Semantics:
/// - Always checks space before writing.
/// - Returns `Ok(false)` if no space or invalid name.
/// - Returns `Err(_)` on I/O errors.
pub fn file_in(disk: block::DeviceHandle, name: &str, bytes: &[u8]) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };
    if name.is_empty() || name.as_bytes().len() > (u16::MAX as usize) {
        return Ok(false);
    }

    let Some((sb, entry_lba, blocks, _total_bytes_rounded)) =
        check_space_for_put(disk, &placement, name.as_bytes().len(), bytes.len())?
    else {
        return Ok(false);
    };

    let mut off = 0usize;
    let mut src = |dst: &mut [u8]| -> Result<usize, block::Error> {
        if off >= bytes.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), bytes.len() - off);
        dst[..take].copy_from_slice(&bytes[off..off + take]);
        off = off.saturating_add(take);
        Ok(take)
    };

    write_put_entry(
        disk,
        &placement,
        entry_lba,
        blocks,
        name,
        &mut src,
        bytes.len() as u64,
    )?;
    advance_log_head(disk, &placement, sb, blocks)?;
    Ok(true)
}

/// TRUEOSFS: read a file.
///
/// Returns `Ok(None)` if missing or fails integrity check.
pub fn file_out(disk: block::DeviceHandle, name: &str) -> Result<Option<Vec<u8>>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };
    let Some(rec) = find_latest_record(disk, &placement, name)? else {
        return Ok(None);
    };

    let mut out = vec![0u8; rec.data_len as usize];
    read_exact_bytes(disk, rec.data_lba, 0, &mut out)?;

    // Integrity check: recompute sha.
    let mut hasher = Sha256::new();
    hasher.update(&out);
    let digest = hasher.finalize();
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&digest[..]);
    if sha != rec.sha256 {
        return Ok(None);
    }
    Ok(Some(out))
}

pub fn file_out_ok(disk: block::DeviceHandle, name: &str, out: &mut Vec<u8>) -> Result<bool, block::Error> {
    match file_out(disk, name)? {
        Some(v) => {
            out.clear();
            out.extend_from_slice(&v);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// TRUEOSFS: delete a file.
pub fn file_delete(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };
    if name.is_empty() || name.as_bytes().len() > (u16::MAX as usize) {
        return Ok(false);
    }
    // Must exist.
    if find_latest_record(disk, &placement, name)?.is_none() {
        return Ok(false);
    }

    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let sb_block = read_one_block_aligned(disk, placement.super_lba)?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };
    let entry_lba = placement.data_lba.saturating_add(sb.log_head_rel_blocks);

    // Space check.
    let blocks = entry_blocks(bs, name.as_bytes().len(), 0);
    let end_lba = disk_data_end_lba_exclusive(disk, &placement);
    if entry_lba.saturating_add(blocks) > end_lba {
        return Ok(false);
    }

    let written_blocks = write_delete_entry(disk, &placement, entry_lba, name)?;
    advance_log_head(disk, &placement, sb, written_blocks)?;
    Ok(true)
}

/// TRUEOSFS: validate a file by comparing stored SHA-256 with recomputation.
pub fn file_valid(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };
    let Some(rec) = find_latest_record(disk, &placement, name)? else {
        return Ok(false);
    };
    let sha = compute_sha256_of_entry_data(disk, &rec)?;
    Ok(sha == rec.sha256)
}

/// TRUEOSFS: check whether a file exists.
pub fn file_exists(disk: block::DeviceHandle, name: &str) -> Result<bool, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };
    Ok(find_latest_record(disk, &placement, name)?.is_some())
}

/// TRUEOSFS: list the immediate children of a directory.
///
/// This treats stored file names as `/`-separated paths (e.g. `qjs/cdn/abc.mjs`).
/// Output matches the USBMS/FAT listing format: newline-separated entry names.
///
/// Returns `Ok(None)` if the disk does not contain TRUEOSFS.
pub fn list_dir(disk: block::DeviceHandle, dir: &str) -> Result<Option<String>, block::Error> {
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(None);
    };

    // Normalize to a relative prefix; absolute input means “from root”.
    let Some(rel) = crate::path::normalize_rel_no_parent(dir) else {
        return Ok(Some(String::new()));
    };
    let prefix = if rel.is_empty() {
        String::new()
    } else {
        alloc::format!("{}/", rel)
    };

    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let sb_block = read_one_block_aligned(disk, placement.super_lba)?;
    let Some(sb) = trueos_fs::parse_superblock(&sb_block) else {
        return Err(block::Error::Corrupted);
    };
    let mut lba = placement.data_lba;
    let end_lba = placement.data_lba.saturating_add(sb.log_head_rel_blocks);

    // Track live keys.
    let mut live: BTreeSet<String> = BTreeSet::new();

    while lba < end_lba {
        let hdr_block = read_one_block_aligned(disk, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        if name_len == 0 || name_len > 4096 {
            break;
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);

        // Read name bytes (exact, without padding) and update live set.
        let name_lba = lba.saturating_add(1);
        let mut tmp_name = vec![0u8; name_len];
        read_exact_bytes(disk, name_lba, 0, &mut tmp_name)?;
        if let Ok(name) = core::str::from_utf8(&tmp_name) {
            // Store names normalized the same way as callers are expected to.
            match hdr.kind {
                LogKind::Put => {
                    live.insert(name.to_string());
                }
                LogKind::Delete => {
                    let _ = live.remove(name);
                }
            }
        }

        lba = lba.saturating_add(blocks);
    }

    // Extract immediate children under the requested prefix.
    let mut children: BTreeSet<String> = BTreeSet::new();
    for name in live.iter() {
        if !prefix.is_empty() {
            if !name.starts_with(prefix.as_str()) {
                continue;
            }
            let rest = &name[prefix.len()..];
            if rest.is_empty() {
                continue;
            }
            let seg = rest.split('/').next().unwrap_or("");
            if !seg.is_empty() {
                children.insert(seg.to_string());
            }
        } else {
            // Root: take first segment.
            let seg = name.split('/').next().unwrap_or("");
            if !seg.is_empty() {
                children.insert(seg.to_string());
            }
        }
    }

    // Match `list_usbms_dir` output: newline-separated names.
    const MAX_LISTING_BYTES: usize = 64 * 1024;
    let mut out = String::new();
    for entry in children.iter() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(entry);
        if out.len() > MAX_LISTING_BYTES {
            // Too large; truncate by returning an empty listing (best-effort).
            return Ok(Some(String::new()));
        }
    }

    Ok(Some(out))
}

/// TRUEOSFS: append bytes by performing a full new write.
///
/// - If file missing: forwards to `file_in`.
/// - If `append_bytes` is empty: returns `Ok(true)`.
pub fn file_append(disk: block::DeviceHandle, name: &str, append_bytes: &[u8]) -> Result<bool, block::Error> {
    if append_bytes.is_empty() {
        return Ok(true);
    }
    if disk.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    let Some(placement) = locate(disk)? else {
        return Ok(false);
    };

    let Some(base) = find_latest_record(disk, &placement, name)? else {
        return file_in(disk, name, append_bytes);
    };

    let bs = disk.info().block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    let base_name_blocks = ((base.name_len as usize) + (bs - 1)) / bs;
    let base_data_lba = base.entry_lba.saturating_add(1).saturating_add(base_name_blocks as u64);

    // Check space for the full new write.
    let new_len = (base.data_len as usize).saturating_add(append_bytes.len());
    let Some((sb, entry_lba, blocks, _total_bytes_rounded)) =
        check_space_for_put(disk, &placement, name.as_bytes().len(), new_len)?
    else {
        return Ok(false);
    };

    // Streaming source: first old bytes, then append bytes.
    let info = disk.info();
    let align = info.dma_alignment.max(1) as usize;
    let mut scratch = AlignedBuf::new(bs, align).ok_or(block::Error::DmaUnavailable)?;
    let scratch_buf = scratch.as_mut_slice();

    let mut base_remaining = base.data_len as usize;
    let mut base_pos = 0usize;
    let mut append_off = 0usize;

    let mut src = |dst: &mut [u8]| -> Result<usize, block::Error> {
        if dst.is_empty() {
            return Ok(0);
        }
        if base_remaining > 0 {
            let lba = base_data_lba.saturating_add((base_pos / bs) as u64);
            let off = base_pos % bs;
            disk.read_blocks(lba, scratch_buf)?;
            let take = core::cmp::min(core::cmp::min(bs - off, base_remaining), dst.len());
            let chunk = &scratch_buf[off..off + take];
            dst[..take].copy_from_slice(chunk);
            base_remaining = base_remaining.saturating_sub(take);
            base_pos = base_pos.saturating_add(take);
            return Ok(take);
        }
        if append_off < append_bytes.len() {
            let take = core::cmp::min(dst.len(), append_bytes.len() - append_off);
            let chunk = &append_bytes[append_off..append_off + take];
            dst[..take].copy_from_slice(chunk);
            append_off = append_off.saturating_add(take);
            return Ok(take);
        }
        Ok(0)
    };

    write_put_entry(
        disk,
        &placement,
        entry_lba,
        blocks,
        name,
        &mut src,
        new_len as u64,
    )?;
    advance_log_head(disk, &placement, sb, blocks)?;
    Ok(true)
}

pub fn roots_len() -> usize {
    ROOTS.lock().len()
}

pub fn root_seq(disk_id: block::DiscId) -> Option<u32> {
    let roots = ROOTS.lock();
    roots.iter().find(|m| m.disk_id == disk_id).map(|m| m.seq)
}

pub fn with_root_tree<R>(disk_id: block::DiscId, f: impl FnOnce(u32, &TrueosFsPlacement, &TrueosFsTree) -> R) -> Option<R> {
    let roots = ROOTS.lock();
    roots
        .iter()
        .find(|m| m.disk_id == disk_id)
        .and_then(|m| m.tree.as_deref().map(|t| f(m.seq, &m.placement, t)))
}

struct AlignedBuf {
    ptr: *mut u8,
    len: usize,
    layout: alloc::alloc::Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        let layout = alloc::alloc::Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc::alloc::alloc(layout) };
        if ptr.is_null() {
            return None;
        }
        Some(Self { ptr, len, layout })
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            unsafe { alloc::alloc::dealloc(self.ptr, self.layout) };
        }
    }
}

fn read_blocks_aligned(handle: block::DeviceHandle, lba: u64, blocks: usize) -> Result<Vec<u8>, block::Error> {
    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }
    let bytes = bs.saturating_mul(blocks);
    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    tmp.as_mut_slice().fill(0);
    handle.read_blocks(lba, tmp.as_mut_slice())?;
    Ok(tmp.as_mut_slice().to_vec())
}

fn looks_like_trueos_superblock(block0: &[u8]) -> bool {
    block0.len() >= 8 && &block0[0..8] == &trueos_fs::MAGIC
}

fn spin_wait_ms(ms: u64) {
    let hz = embassy_time_driver::TICK_HZ as u64;
    let start = embassy_time_driver::now();
    let delta_ticks = if hz == 0 {
        0
    } else {
        // Round up to at least one tick when ms>0.
        let ticks = (ms.saturating_mul(hz) + 999) / 1000;
        if ms > 0 { ticks.max(1) } else { 0 }
    };
    let deadline = start.saturating_add(delta_ticks);
    while embassy_time_driver::now() < deadline {
        crate::time::poll_executor();
        spin_loop();
    }
}

fn is_transient_io(e: block::Error) -> bool {
    matches!(e, block::Error::NotReady | block::Error::Timeout | block::Error::Io)
}

fn read_blocks_aligned_retry(
    handle: block::DeviceHandle,
    lba: u64,
    blocks: usize,
    attempts: u8,
) -> Result<Vec<u8>, block::Error> {
    let mut last: Option<block::Error> = None;
    let mut i = 0u8;
    while i < attempts {
        match read_blocks_aligned(handle, lba, blocks) {
            Ok(v) => return Ok(v),
            Err(e) if is_transient_io(e) => {
                last = Some(e);
                // Give USB storage some time to become ready after heavy writes.
                spin_wait_ms(10);
            }
            Err(e) => return Err(e),
        }
        i = i.wrapping_add(1);
    }
    Err(last.unwrap_or(block::Error::Io))
}

/// Find where TRUEOSFS lives on a whole disk.
///
/// - Bootable disks: TRUEOSFS is expected to live inside a GPT partition (ESP exists elsewhere).
/// - Data-only disks: TRUEOSFS may live at LBA0 (superfloppy-style).
pub fn locate(handle: block::DeviceHandle) -> Result<Option<TrueosFsPlacement>, block::Error> {
    if handle.parent().is_some() {
        return Ok(None);
    }

    // Prefer GPT-partitioned layouts (bootable-capable).
    {
        let mut tries = 0u8;
        while tries < 5 {
            match partition::read_gpt_partitions(handle) {
                Ok(parts) => {
                    let mut has_esp = false;
                    for p in parts.iter() {
                        if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                            has_esp = true;
                        }

                        // Our superblock is at the start of the TRUEOS data partition.
                        if let Ok(p0) = read_blocks_aligned_retry(handle, p.range.first_lba(), 1, 3) {
                            if looks_like_trueos_superblock(&p0) {
                                let super_lba = p.range.first_lba();
                                let end_lba_exclusive = p.range.last_lba().saturating_add(1);
                                return Ok(Some(TrueosFsPlacement {
                                    bootable: has_esp,
                                    super_lba,
                                    data_lba: trueos_fs::data_lba_from_super(super_lba),
                                    data_end_lba_exclusive: Some(end_lba_exclusive),
                                }));
                            }
                        }
                    }
                    break;
                }
                Err(e) if is_transient_io(e) => {
                    spin_wait_ms(10);
                }
                Err(_) => break,
            }
            tries = tries.wrapping_add(1);
        }
    }

    // Fallback: superblock at LBA0 (data-only images/disks).
    let bs0 = read_blocks_aligned_retry(handle, 0, 1, 3)?;
    if looks_like_trueos_superblock(&bs0) {
        return Ok(Some(TrueosFsPlacement {
            bootable: false,
            super_lba: 0,
            data_lba: trueos_fs::data_lba_from_super(0),
            data_end_lba_exclusive: None,
        }));
    }

    Ok(None)
}

pub fn format_blank(handle: block::DeviceHandle) -> Result<(), block::Error> {
    if handle.parent().is_some() {
        return Err(block::Error::InvalidParam);
    }
    if !handle.supports_write() {
        return Err(block::Error::NotSupported);
    }

    // If the disk is GPT-partitioned and has an ESP, do NOT clobber LBA0.
    // Only format an existing TRUEOS data partition (bootable layout).
    if let Ok(parts) = partition::read_gpt_partitions(handle) {
        let has_esp = parts
            .iter()
            .any(|p| p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES);

        if has_esp {
            if let Some(loc) = locate(handle)? {
                return format_blank_at(handle, loc.super_lba);
            }
            return Err(block::Error::NotSupported);
        }
    }

    // Data-only (superblock at LBA0).
    format_blank_at(handle, 0)
}

/// Format TRUEOSFS at the start of an already-created partition.
///
/// This is intended for installer code that first creates a GPT layout and then
/// formats the TRUEOS data partition without clobbering LBA0 of the whole disk.
pub fn format_blank_partition(partition: block::DeviceHandle) -> Result<(), block::Error> {
    if partition.parent().is_none() {
        return Err(block::Error::InvalidParam);
    }
    if !partition.supports_write() {
        return Err(block::Error::NotSupported);
    }
    format_blank_at(partition, 0)
}

fn format_blank_at(handle: block::DeviceHandle, super_lba: u64) -> Result<(), block::Error> {

    let info = handle.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        return Err(block::Error::InvalidParam);
    }

    let max_blocks = if info.max_transfer_bytes > 0 {
        (info.max_transfer_bytes as usize / bs).max(1)
    } else {
        1
    };
    let blocks = core::cmp::min(8usize, max_blocks);
    let bytes = bs.saturating_mul(blocks);

    let align = info.dma_alignment.max(1) as usize;
    let mut tmp = AlignedBuf::new(bytes, align).ok_or(block::Error::DmaUnavailable)?;
    let buf = tmp.as_mut_slice();
    buf.fill(0);

    trueos_fs::write_blank_superblock(&mut buf[..bs]);

    handle.write_blocks(super_lba, buf)?;
    handle.flush()?;

    // Best-effort end-to-end NVMe sanity check: write a tiny payload into the data region
    // and read it back. This keeps validation at the block device layer (no extra shell cmds)
    // and avoids clobbering partition tables/boot sectors.
    let info = handle.info();
    if info.kind == block::DeviceKind::Nvme {
        let data_lba = trueos_fs::data_lba_from_super(super_lba);
        // Need at least 2 blocks available in the data region.
        if data_lba.saturating_add(2) <= info.block_count {
            let blocks_verify = core::cmp::min(2usize, max_blocks);
            let bytes_verify = bs.saturating_mul(blocks_verify);
            let mut tmp2 = AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
            let w = tmp2.as_mut_slice();
            w.fill(0);
            let tag = b"TRUEOSFS-NVME-VERIFY";
            let n = core::cmp::min(tag.len(), bs);
            w[..n].copy_from_slice(&tag[..n]);
            // Put some changing bytes in the second block too.
            if blocks_verify > 1 {
                for (i, b) in w[bs..(2 * bs)].iter_mut().enumerate() {
                    *b = (i as u8).wrapping_mul(17).wrapping_add(0x5A);
                }
            }

            handle.write_blocks(data_lba, w)?;
            handle.flush()?;

            let mut rtmp = AlignedBuf::new(bytes_verify, align).ok_or(block::Error::DmaUnavailable)?;
            let r = rtmp.as_mut_slice();
            r.fill(0);
            handle.read_blocks(data_lba, r)?;
            if r != w {
                return Err(block::Error::Corrupted);
            }
        }
    }
    Ok(())
}
