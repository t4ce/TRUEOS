#![no_std]

extern crate alloc;

use alloc::{collections::BTreeSet, string::String, vec, vec::Vec};
use sha2::{Digest, Sha256};

pub const MAGIC: [u8; 8] = *b"TRUEOSFS";

// Superblock layout (little-endian):
// [0..8]   MAGIC
// [8..16]  LOG_HEAD_REL_BLOCKS: u64 (relative to data_lba)
// [16..24] CHECKPOINT_REL_BLOCKS: u64 (relative to data_lba; 0 = none)

pub const SUPERBLOCK_MIN_BYTES: usize = 16;
pub const SUPERBLOCK_WITH_CKPT_MIN_BYTES: usize = 24;

pub const SUPERBLOCK_LOG_HEAD_REL_OFF: usize = 8;
pub const SUPERBLOCK_CHECKPOINT_REL_OFF: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Superblock {
    /// Next free block in the data region (relative to `data_lba_from_super(super_lba)`).
    pub log_head_rel_blocks: u64,

    /// Relative block offset (from `data_lba`) of the latest index checkpoint entry.
    ///
    /// `0` means no checkpoint is recorded.
    pub checkpoint_rel_blocks: u64,
}

pub fn parse_superblock(block0: &[u8]) -> Option<Superblock> {
    if block0.len() < SUPERBLOCK_MIN_BYTES {
        return None;
    }
    if &block0[0..8] != &MAGIC {
        return None;
    }
    let log_head_rel_blocks = u64::from_le_bytes([
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 1],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 2],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 3],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 4],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 5],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 6],
        block0[SUPERBLOCK_LOG_HEAD_REL_OFF + 7],
    ]);

    let checkpoint_rel_blocks = if block0.len() >= SUPERBLOCK_WITH_CKPT_MIN_BYTES {
        u64::from_le_bytes([
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 1],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 2],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 3],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 4],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 5],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 6],
            block0[SUPERBLOCK_CHECKPOINT_REL_OFF + 7],
        ])
    } else {
        0
    };

    Some(Superblock {
        log_head_rel_blocks,
        checkpoint_rel_blocks,
    })
}

pub fn write_superblock(block0: &mut [u8], sb: Superblock) {
    if block0.len() < SUPERBLOCK_MIN_BYTES {
        return;
    }
    // Keep any extra bytes beyond our known fields zeroed for now.
    for b in block0.iter_mut() {
        *b = 0;
    }
    block0[0..8].copy_from_slice(&MAGIC);
    block0[SUPERBLOCK_LOG_HEAD_REL_OFF..SUPERBLOCK_LOG_HEAD_REL_OFF + 8]
        .copy_from_slice(&sb.log_head_rel_blocks.to_le_bytes());

    if block0.len() >= SUPERBLOCK_WITH_CKPT_MIN_BYTES {
        block0[SUPERBLOCK_CHECKPOINT_REL_OFF..SUPERBLOCK_CHECKPOINT_REL_OFF + 8]
            .copy_from_slice(&sb.checkpoint_rel_blocks.to_le_bytes());
    }
}

/// Relative LBA (from the superblock) where the payload/data region starts.
///
/// Keeping this fixed means higher-level logic can treat the filesystem as
/// "starting at super_lba", regardless of whether the disk is data-only
/// (superblock at LBA0) or bootable (superblock inside a GPT partition).
pub const DATA_START_LBA_REL: u64 = 8;

#[inline]
pub const fn data_lba_from_super(super_lba: u64) -> u64 {
    super_lba + DATA_START_LBA_REL
}

pub fn write_blank_superblock(block0: &mut [u8]) {
    if block0.len() < SUPERBLOCK_MIN_BYTES {
        return;
    }

    write_superblock(
        block0,
        Superblock {
            log_head_rel_blocks: 0,
            checkpoint_rel_blocks: 0,
        },
    );
}

// --- TRUEOSFS generic engine (backend-agnostic) ---

/// Generic block I/O interface for the `trueos-fs` engine.
///
/// This is intentionally tiny: callers decide how to satisfy alignment/DMA
/// requirements. Kernel backends typically stage through aligned buffers.
pub trait BlockIo {
    type Error;

    fn block_size(&self) -> usize;
    fn block_count(&self) -> u64;

    /// Maximum transfer size (best-effort hint). Implementations may ignore.
    fn max_transfer_bytes(&self) -> usize {
        256 * 1024
    }

    /// Read `blocks` blocks at `lba`.
    ///
    /// The returned Vec is `blocks * block_size()` bytes.
    fn read_blocks(&self, lba: u64, blocks: usize) -> Result<Vec<u8>, Self::Error>;

    /// Write blocks starting at `lba`.
    ///
    /// `buf.len()` must be a multiple of `block_size()`.
    fn write_blocks(&self, lba: u64, buf: &[u8]) -> Result<(), Self::Error>;

    fn flush(&self) -> Result<(), Self::Error>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FsParams {
    pub super_lba: u64,
    pub data_lba: u64,
    pub data_end_lba_exclusive: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FsError<E> {
    Device(E),
    InvalidParam,
    Corrupted,
}

impl<E> From<E> for FsError<E> {
    fn from(value: E) -> Self {
        FsError::Device(value)
    }
}

// --- On-disk log format (simple, append-only) ---

pub const LOG_ENTRY_MAGIC: [u8; 8] = *b"TOSFLOG\0";

/// Delete records store the referenced (deleted) entry LBA as their data payload.
pub const DELETE_REF_BYTES: usize = 8;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogKind {
    Put = 1,
    Delete = 2,
    IndexCheckpoint = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LogHeader {
    pub kind: LogKind,
    pub committed: bool,
    pub name_len: u16,
    pub data_len: u64,
    pub sha256: [u8; 32],
}

/// Decoded index checkpoint payload.
///
/// This is meant to let mount code load an in-memory index snapshot and then
/// replay only the tail of the log from `replay_from_rel_blocks`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IndexCheckpoint {
    pub replay_from_rel_blocks: u64,
    pub entries: Vec<(Vec<u8>, LogKind, u64)>,
}

/// Encode the checkpoint payload as:
/// - `replay_from_rel_blocks: u64` (LE)
/// - repeated entries:
///   - `key_len: u16` (LE)
///   - `reserved: u16` (stores `kind` in low byte; 0 means "assume Put" for backward compat)
///   - `value_lba: u64` (LE) (the log entry LBA for the latest record)
///   - `key_bytes: [u8; key_len]`
pub fn encode_index_checkpoint_payload<'a>(
    replay_from_rel_blocks: u64,
    entries: impl Iterator<Item = (&'a [u8], LogKind, u64)>,
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&replay_from_rel_blocks.to_le_bytes());
    for (key, kind, entry_lba) in entries {
        let key_len = core::cmp::min(key.len(), u16::MAX as usize) as u16;
        out.extend_from_slice(&key_len.to_le_bytes());
        let reserved = (kind as u8) as u16;
        out.extend_from_slice(&reserved.to_le_bytes());
        out.extend_from_slice(&entry_lba.to_le_bytes());
        out.extend_from_slice(&key[..key_len as usize]);
    }
    out
}

pub fn decode_index_checkpoint_payload(payload: &[u8]) -> Option<IndexCheckpoint> {
    if payload.len() < 8 {
        return None;
    }
    let replay_from_rel_blocks = u64::from_le_bytes(payload[0..8].try_into().ok()?);
    let mut off = 8usize;
    let mut entries: Vec<(Vec<u8>, LogKind, u64)> = Vec::new();
    while off < payload.len() {
        if payload.len().saturating_sub(off) < 12 {
            return None;
        }
        let key_len = u16::from_le_bytes(payload[off..off + 2].try_into().ok()?) as usize;
        let reserved = u16::from_le_bytes(payload[off + 2..off + 4].try_into().ok()?);
        let entry_lba = u64::from_le_bytes(payload[off + 4..off + 12].try_into().ok()?);
        off = off.saturating_add(12);

        let kind_byte = (reserved & 0x00FF) as u8;
        let kind = match kind_byte {
            1 => LogKind::Put,
            2 => LogKind::Delete,
            // Backward compat: older payloads had reserved==0.
            _ => LogKind::Put,
        };

        if payload.len().saturating_sub(off) < key_len {
            return None;
        }
        let mut key = vec![0u8; key_len];
        key.copy_from_slice(&payload[off..off + key_len]);
        off = off.saturating_add(key_len);

        entries.push((key, kind, entry_lba));
    }

    Some(IndexCheckpoint {
        replay_from_rel_blocks,
        entries,
    })
}

impl LogHeader {
    pub fn encode_into_block(&self, block: &mut [u8]) {
        const MIN: usize = 52;
        if block.len() < MIN {
            return;
        }
        block[0..8].copy_from_slice(&LOG_ENTRY_MAGIC);
        block[8] = self.kind as u8;
        block[9] = if self.committed { 1 } else { 0 };
        block[10..12].copy_from_slice(&self.name_len.to_le_bytes());
        block[12..20].copy_from_slice(&self.data_len.to_le_bytes());
        block[20..52].copy_from_slice(&self.sha256);
        for b in block[52..].iter_mut() {
            *b = 0;
        }
    }

    pub fn decode_from_block(block: &[u8]) -> Option<Self> {
        if block.len() < 52 {
            return None;
        }
        if &block[0..8] != &LOG_ENTRY_MAGIC {
            return None;
        }
        let kind = match block[8] {
            1 => LogKind::Put,
            2 => LogKind::Delete,
            3 => LogKind::IndexCheckpoint,
            _ => return None,
        };
        let committed = block[9] == 1;
        let name_len = u16::from_le_bytes([block[10], block[11]]);
        let data_len = u64::from_le_bytes([
            block[12], block[13], block[14], block[15], block[16], block[17], block[18], block[19],
        ]);
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&block[20..52]);
        Some(Self {
            kind,
            committed,
            name_len,
            data_len,
            sha256,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct FileRecord {
    entry_lba: u64,
    name_len: u16,
    data_len: u64,
    data_lba: u64,
    sha256: [u8; 32],
}

fn disk_data_end_lba_exclusive<D: BlockIo>(dev: &D, params: &FsParams) -> u64 {
    params
        .data_end_lba_exclusive
        .unwrap_or_else(|| dev.block_count())
}

fn entry_blocks(block_size: usize, name_len: usize, data_len: usize) -> u64 {
    if block_size == 0 {
        return 0;
    }
    let name_blocks = (name_len + (block_size - 1)) / block_size;
    let data_blocks = (data_len + (block_size - 1)) / block_size;
    (1 + name_blocks + data_blocks) as u64
}

fn read_one_block<D: BlockIo>(dev: &D, lba: u64) -> Result<Vec<u8>, FsError<D::Error>> {
    dev.read_blocks(lba, 1).map_err(FsError::Device)
}

fn read_exact_bytes<D: BlockIo>(
    dev: &D,
    start_lba: u64,
    start_byte_off: usize,
    out: &mut [u8],
) -> Result<(), FsError<D::Error>> {
    if out.is_empty() {
        return Ok(());
    }
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    let mut remaining = out;
    let mut abs_byte = start_byte_off;
    while !remaining.is_empty() {
        let lba = start_lba.saturating_add((abs_byte / bs) as u64);
        let off = abs_byte % bs;
        let scratch = read_one_block(dev, lba)?;
        if scratch.len() < bs {
            return Err(FsError::Corrupted);
        }
        let take = core::cmp::min(bs - off, remaining.len());
        remaining[..take].copy_from_slice(&scratch[off..off + take]);
        remaining = &mut remaining[take..];
        abs_byte = abs_byte.saturating_add(take);
    }
    Ok(())
}

fn compute_sha256_of_entry_data<D: BlockIo>(
    dev: &D,
    rec: &FileRecord,
) -> Result<[u8; 32], FsError<D::Error>> {
    let mut hasher = Sha256::new();
    let mut remaining = rec.data_len as usize;
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let mut pos = 0usize;
    while remaining > 0 {
        let lba = rec.data_lba.saturating_add((pos / bs) as u64);
        let off = pos % bs;
        let scratch = read_one_block(dev, lba)?;
        if scratch.len() < bs {
            return Err(FsError::Corrupted);
        }
        let take = core::cmp::min(bs - off, remaining);
        hasher.update(&scratch[off..off + take]);
        remaining = remaining.saturating_sub(take);
        pos = pos.saturating_add(take);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest[..]);
    Ok(out)
}

fn check_space_for_put<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name_len: usize,
    data_len: usize,
) -> Result<Option<(Superblock, u64, u64)>, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };

    let total_blocks = entry_blocks(bs, name_len, data_len);
    let entry_lba = params.data_lba.saturating_add(sb.log_head_rel_blocks);
    let end_lba = disk_data_end_lba_exclusive(dev, params);
    if entry_lba.saturating_add(total_blocks) > end_lba {
        return Ok(None);
    }
    Ok(Some((sb, entry_lba, total_blocks)))
}

fn advance_log_head<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    mut sb: Superblock,
    delta_blocks: u64,
) -> Result<(), FsError<D::Error>> {
    sb.log_head_rel_blocks = sb.log_head_rel_blocks.saturating_add(delta_blocks);
    write_superblock_to_disk(dev, params, sb)
}

fn write_superblock_to_disk<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    sb: Superblock,
) -> Result<(), FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let mut tmp = vec![0u8; bs];
    write_superblock(&mut tmp, sb);
    dev.write_blocks(params.super_lba, &tmp).map_err(FsError::Device)?;
    dev.flush().map_err(FsError::Device)?;
    Ok(())
}

/// Read the latest index checkpoint pointed to by the superblock.
///
/// Returns `Ok(None)` if no checkpoint is recorded or if the referenced record
/// is invalid (best-effort robustness for torn superblock updates).
pub fn read_index_checkpoint<D: BlockIo>(
    dev: &D,
    params: &FsParams,
) -> Result<Option<IndexCheckpoint>, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };
    if sb.checkpoint_rel_blocks == 0 {
        return Ok(None);
    }

    let entry_lba = params.data_lba.saturating_add(sb.checkpoint_rel_blocks);
    let hdr_block = read_one_block(dev, entry_lba)?;
    let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
        return Ok(None);
    };
    if !hdr.committed || hdr.kind != LogKind::IndexCheckpoint {
        return Ok(None);
    }
    if hdr.name_len != 0 {
        return Ok(None);
    }
    if hdr.data_len < 8 {
        return Ok(None);
    }

    let payload_len = hdr.data_len as usize;
    let payload_lba = entry_lba.saturating_add(1);
    let mut payload = vec![0u8; payload_len];
    read_exact_bytes(dev, payload_lba, 0, &mut payload)?;

    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let digest = hasher.finalize();
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&digest[..]);
    if sha != hdr.sha256 {
        return Ok(None);
    }

    Ok(decode_index_checkpoint_payload(&payload))
}

/// Write a new index checkpoint record and update the superblock's
/// `checkpoint_rel_blocks` pointer to it.
///
/// Returns `Ok(false)` if there's no space.
pub fn write_index_checkpoint<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    replay_from_rel_blocks: u64,
    entries: impl Iterator<Item = (Vec<u8>, LogKind, u64)>,
) -> Result<bool, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    // Encode payload as (replay_from u64) + repeated entries.
    let payload = {
        let mut buf = Vec::new();
        buf.extend_from_slice(&replay_from_rel_blocks.to_le_bytes());
        for (k, kind, entry_lba) in entries {
            let key_len = core::cmp::min(k.len(), u16::MAX as usize) as u16;
            buf.extend_from_slice(&key_len.to_le_bytes());
            let reserved = (kind as u8) as u16;
            buf.extend_from_slice(&reserved.to_le_bytes());
            buf.extend_from_slice(&entry_lba.to_le_bytes());
            buf.extend_from_slice(&k[..key_len as usize]);
        }
        buf
    };

    let Some((mut sb, entry_lba, blocks)) = check_space_for_put(dev, params, 0, payload.len())? else {
        return Ok(false);
    };

    // 1) Header committed=0.
    let mut hdr0 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::IndexCheckpoint,
        committed: false,
        name_len: 0,
        data_len: payload.len() as u64,
        sha256: [0u8; 32],
    }
    .encode_into_block(&mut hdr0);
    dev.write_blocks(entry_lba, &hdr0).map_err(FsError::Device)?;

    // 2) Payload blocks.
    let payload_blocks = (payload.len() + (bs - 1)) / bs;
    let payload_bytes_rounded = payload_blocks * bs;
    let mut off = 0usize;
    let mut src = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> {
        if off >= payload.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), payload.len() - off);
        dst[..take].copy_from_slice(&payload[off..off + take]);
        off = off.saturating_add(take);
        Ok(take)
    };
    if payload_bytes_rounded > 0 {
        write_stream_at_lba(dev, entry_lba.saturating_add(1), payload.len(), payload_bytes_rounded, &mut src)?;
    }
    dev.flush().map_err(FsError::Device)?;

    // 3) Rewrite header committed=1 with sha of payload.
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let digest = hasher.finalize();
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&digest[..]);

    let mut hdr1 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::IndexCheckpoint,
        committed: true,
        name_len: 0,
        data_len: payload.len() as u64,
        sha256,
    }
    .encode_into_block(&mut hdr1);
    dev.write_blocks(entry_lba, &hdr1).map_err(FsError::Device)?;
    dev.flush().map_err(FsError::Device)?;

    // 4) Update superblock: advance log head and point checkpoint pointer here.
    let ckpt_rel = entry_lba.saturating_sub(params.data_lba);
    sb.log_head_rel_blocks = sb.log_head_rel_blocks.saturating_add(blocks);
    sb.checkpoint_rel_blocks = ckpt_rel;
    write_superblock_to_disk(dev, params, sb)?;
    Ok(true)
}

/// Replay the log range `[start_rel_blocks, end_rel_blocks)` and call `apply`
/// for each committed Put/Delete record.
///
/// Checkpoint records are skipped.
pub fn replay_log_range<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    start_rel_blocks: u64,
    end_rel_blocks: u64,
    mut apply: impl FnMut(LogKind, Vec<u8>, u64),
) -> Result<(), FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    let mut lba = params.data_lba.saturating_add(start_rel_blocks);
    let end_lba = params.data_lba.saturating_add(end_rel_blocks);

    while lba < end_lba {
        let hdr_block = read_one_block(dev, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        match hdr.kind {
            LogKind::Put | LogKind::Delete => {
                if name_len == 0 || name_len > 4096 {
                    break;
                }
                if hdr.kind == LogKind::Delete && data_len != DELETE_REF_BYTES {
                    break;
                }
            }
            LogKind::IndexCheckpoint => {
                if name_len != 0 || data_len < 8 {
                    break;
                }
            }
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);

        if hdr.kind == LogKind::IndexCheckpoint {
            lba = lba.saturating_add(blocks);
            continue;
        }

        let name_lba = lba.saturating_add(1);
        let mut name_bytes = vec![0u8; name_len];
        read_exact_bytes(dev, name_lba, 0, &mut name_bytes)?;
        apply(hdr.kind, name_bytes, lba);

        lba = lba.saturating_add(blocks);
    }

    Ok(())
}

fn write_stream_at_lba<D: BlockIo>(
    dev: &D,
    start_lba: u64,
    exact_bytes: usize,
    total_bytes_rounded: usize,
    mut source: impl FnMut(&mut [u8]) -> Result<usize, FsError<D::Error>>,
) -> Result<(), FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    if total_bytes_rounded == 0 || total_bytes_rounded % bs != 0 {
        return Err(FsError::InvalidParam);
    }
    if exact_bytes > total_bytes_rounded {
        return Err(FsError::InvalidParam);
    }

    let max_blocks = {
        let mtb = dev.max_transfer_bytes();
        if mtb > 0 {
            core::cmp::max(1, mtb / bs)
        } else {
            1
        }
    };

    let mut lba = start_lba;
    let mut written = 0usize;
    let mut written_exact = 0usize;
    while written < total_bytes_rounded {
        let remaining = total_bytes_rounded - written;
        let blocks_here = core::cmp::min(max_blocks, remaining / bs);
        let bytes_here = blocks_here * bs;

        let mut chunk = vec![0u8; bytes_here];
        let mut filled = 0usize;
        while filled < bytes_here {
            if written_exact >= exact_bytes {
                break;
            }
            let remaining_exact = exact_bytes - written_exact;
            let want = core::cmp::min(bytes_here - filled, remaining_exact);
            let n = source(&mut chunk[filled..filled + want])?;
            if n == 0 {
                return Err(FsError::Corrupted);
            }
            filled = filled.saturating_add(n);
            written_exact = written_exact.saturating_add(n);
        }

        dev.write_blocks(lba, &chunk).map_err(FsError::Device)?;
        lba = lba.saturating_add(blocks_here as u64);
        written = written.saturating_add(bytes_here);
    }
    if written_exact != exact_bytes {
        return Err(FsError::Corrupted);
    }
    Ok(())
}

fn write_put_entry<D: BlockIo>(
    dev: &D,
    entry_lba: u64,
    total_blocks: u64,
    name: &str,
    data_source: &mut dyn FnMut(&mut [u8]) -> Result<usize, FsError<D::Error>>,
    data_len: u64,
) -> Result<(), FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    let name_len = name.as_bytes().len();
    let name_blocks = (name_len + (bs - 1)) / bs;
    let name_bytes_rounded = name_blocks * bs;
    let data_blocks = ((data_len as usize) + (bs - 1)) / bs;
    let data_bytes_rounded = data_blocks * bs;

    let expected_blocks = 1u64
        .saturating_add(name_blocks as u64)
        .saturating_add(data_blocks as u64);
    if expected_blocks != total_blocks {
        return Err(FsError::InvalidParam);
    }

    // 1) Header committed=0.
    let mut hdr0 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::Put,
        committed: false,
        name_len: name_len as u16,
        data_len,
        sha256: [0u8; 32],
    }
    .encode_into_block(&mut hdr0);
    dev.write_blocks(entry_lba, &hdr0).map_err(FsError::Device)?;

    // 2) Name blocks (padded).
    if name_bytes_rounded > 0 {
        let mut name_buf = vec![0u8; name_bytes_rounded];
        name_buf[..name_len].copy_from_slice(name.as_bytes());
        dev.write_blocks(entry_lba.saturating_add(1), &name_buf)
            .map_err(FsError::Device)?;
    }

    // 3) Data stream while hashing.
    let mut hasher = Sha256::new();
    if data_bytes_rounded > 0 {
        let mut remaining = data_len as usize;
        let mut data_src = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> {
            if remaining == 0 {
                return Ok(0);
            }
            let want = core::cmp::min(dst.len(), remaining);
            let got = data_source(&mut dst[..want])?;
            if got == 0 {
                return Err(FsError::Corrupted);
            }
            hasher.update(&dst[..got]);
            remaining = remaining.saturating_sub(got);
            Ok(got)
        };

        write_stream_at_lba(
            dev,
            entry_lba
                .saturating_add(1)
                .saturating_add(name_blocks as u64),
            data_len as usize,
            data_bytes_rounded,
            &mut data_src,
        )?;
    }
    dev.flush().map_err(FsError::Device)?;

    // 4) Rewrite header committed=1 with sha.
    let digest = hasher.finalize();
    let mut sha256 = [0u8; 32];
    sha256.copy_from_slice(&digest[..]);
    let mut hdr1 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::Put,
        committed: true,
        name_len: name_len as u16,
        data_len,
        sha256,
    }
    .encode_into_block(&mut hdr1);
    dev.write_blocks(entry_lba, &hdr1).map_err(FsError::Device)?;
    dev.flush().map_err(FsError::Device)?;

    Ok(())
}

fn write_delete_entry<D: BlockIo>(
    dev: &D,
    entry_lba: u64,
    name: &str,
    deleted_entry_lba: u64,
    deleted_sha256: [u8; 32],
) -> Result<u64, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let name_len = name.len();
    let blocks = entry_blocks(bs, name_len, DELETE_REF_BYTES);

    // 1) Header committed=0.
    let mut hdr0 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::Delete,
        committed: false,
        name_len: name_len as u16,
        data_len: DELETE_REF_BYTES as u64,
        sha256: deleted_sha256,
    }
    .encode_into_block(&mut hdr0);
    dev.write_blocks(entry_lba, &hdr0).map_err(FsError::Device)?;

    let name_bytes = name.as_bytes();
    let name_blocks = (name_bytes.len() + (bs - 1)) / bs;
    let payload_bytes_rounded = name_blocks * bs;
    let mut off = 0usize;
    let mut src = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> {
        if off >= name_bytes.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), name_bytes.len() - off);
        dst[..take].copy_from_slice(&name_bytes[off..off + take]);
        off = off.saturating_add(take);
        Ok(take)
    };
    if payload_bytes_rounded > 0 {
        write_stream_at_lba(dev, entry_lba.saturating_add(1), name_bytes.len(), payload_bytes_rounded, &mut src)?;
    }

    // Data payload: referenced entry LBA (little-endian u64).
    let data_blocks = (DELETE_REF_BYTES + (bs - 1)) / bs;
    let data_bytes_rounded = data_blocks * bs;
    let ref_bytes = deleted_entry_lba.to_le_bytes();
    let mut ref_off = 0usize;
    let mut ref_src = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> {
        if ref_off >= ref_bytes.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), ref_bytes.len() - ref_off);
        dst[..take].copy_from_slice(&ref_bytes[ref_off..ref_off + take]);
        ref_off = ref_off.saturating_add(take);
        Ok(take)
    };
    if data_bytes_rounded > 0 {
        write_stream_at_lba(
            dev,
            entry_lba
                .saturating_add(1)
                .saturating_add(name_blocks as u64),
            DELETE_REF_BYTES,
            data_bytes_rounded,
            &mut ref_src,
        )?;
    }
    dev.flush().map_err(FsError::Device)?;

    // 2) Rewrite header committed=1 (after payload is durable).
    let mut hdr1 = vec![0u8; bs];
    LogHeader {
        kind: LogKind::Delete,
        committed: true,
        name_len: name_len as u16,
        data_len: DELETE_REF_BYTES as u64,
        sha256: deleted_sha256,
    }
    .encode_into_block(&mut hdr1);
    dev.write_blocks(entry_lba, &hdr1).map_err(FsError::Device)?;
    dev.flush().map_err(FsError::Device)?;
    Ok(blocks)
}

fn read_put_entry_data<D: BlockIo>(
    dev: &D,
    entry_lba: u64,
) -> Result<Option<(Vec<u8>, [u8; 32])>, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    let hdr_block = read_one_block(dev, entry_lba)?;
    let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
        return Ok(None);
    };
    if !hdr.committed || hdr.kind != LogKind::Put {
        return Ok(None);
    }

    let name_len = hdr.name_len as usize;
    let data_len = hdr.data_len as usize;
    if name_len == 0 || name_len > 4096 {
        return Ok(None);
    }

    let name_blocks = (name_len + (bs - 1)) / bs;
    let data_lba = entry_lba.saturating_add(1).saturating_add(name_blocks as u64);

    let mut out = vec![0u8; data_len];
    read_exact_bytes(dev, data_lba, 0, &mut out)?;

    let mut hasher = Sha256::new();
    hasher.update(&out);
    let digest = hasher.finalize();
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&digest[..]);
    if sha != hdr.sha256 {
        return Ok(None);
    }

    Ok(Some((out, sha)))
}

fn find_latest_delete_ref<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<Option<(u64, [u8; 32])>, FsError<D::Error>> {
    if name.is_empty() {
        return Ok(None);
    }
    let name_bytes = name.as_bytes();
    if name_bytes.len() > (u16::MAX as usize) {
        return Ok(None);
    }

    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };

    let mut lba = params.data_lba;
    let end_lba = params.data_lba.saturating_add(sb.log_head_rel_blocks);
    let mut latest_delete: Option<(u64, [u8; 32])> = None;

    while lba < end_lba {
        let hdr_block = read_one_block(dev, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        match hdr.kind {
            LogKind::Put | LogKind::Delete => {
                if name_len == 0 || name_len > 4096 {
                    break;
                }
                if hdr.kind == LogKind::Delete && data_len != DELETE_REF_BYTES {
                    break;
                }
            }
            LogKind::IndexCheckpoint => {
                // Index checkpoints intentionally have no name payload.
                if name_len != 0 {
                    break;
                }
                // Require at least the replay-from u64.
                if data_len < 8 {
                    break;
                }
            }
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);

        if hdr.kind != LogKind::IndexCheckpoint && name_len == name_bytes.len() {
            let name_lba = lba.saturating_add(1);
            let mut tmp_name = vec![0u8; name_len];
            read_exact_bytes(dev, name_lba, 0, &mut tmp_name)?;
            if tmp_name == name_bytes {
                match hdr.kind {
                    LogKind::Put => {
                        latest_delete = None;
                    }
                    LogKind::Delete => {
                        let ref_lba = lba
                            .saturating_add(1)
                            .saturating_add(name_blocks as u64);
                        let mut ref_bytes = [0u8; DELETE_REF_BYTES];
                        read_exact_bytes(dev, ref_lba, 0, &mut ref_bytes)?;
                        let deleted_entry_lba = u64::from_le_bytes(ref_bytes);
                        latest_delete = Some((deleted_entry_lba, hdr.sha256));
                    }
                    LogKind::IndexCheckpoint => {}
                }
            }
        }

        lba = lba.saturating_add(blocks);
    }

    Ok(latest_delete)
}

/// Attempts to restore the most recently deleted version of `name`.
///
/// Returns `true` if a restore was performed.
pub fn undelete_file<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<bool, FsError<D::Error>> {
    let Some((deleted_entry_lba, expected_sha)) = find_latest_delete_ref(dev, params, name)? else {
        return Ok(false);
    };

    // Restore from the referenced Put entry.
    let Some((data, sha)) = read_put_entry_data(dev, deleted_entry_lba)? else {
        return Ok(false);
    };
    if sha != expected_sha {
        return Ok(false);
    }

    write_file(dev, params, name, &data)
}

fn find_latest_record<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<Option<FileRecord>, FsError<D::Error>> {
    if name.is_empty() {
        return Ok(None);
    }
    let name_bytes = name.as_bytes();
    if name_bytes.len() > (u16::MAX as usize) {
        return Ok(None);
    }

    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };

    let mut lba = params.data_lba;
    let end_lba = params.data_lba.saturating_add(sb.log_head_rel_blocks);
    let mut latest: Option<FileRecord> = None;

    while lba < end_lba {
        let hdr_block = read_one_block(dev, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        match hdr.kind {
            LogKind::Put | LogKind::Delete => {
                if name_len == 0 || name_len > 4096 {
                    break;
                }
            }
            LogKind::IndexCheckpoint => {
                if name_len != 0 {
                    break;
                }
                if data_len < 8 {
                    break;
                }
            }
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);
        let name_lba = lba.saturating_add(1);

        if hdr.kind != LogKind::IndexCheckpoint && name_len == name_bytes.len() {
            let mut tmp_name = vec![0u8; name_len];
            read_exact_bytes(dev, name_lba, 0, &mut tmp_name)?;
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
                    LogKind::IndexCheckpoint => {}
                }
            }
        }

        lba = lba.saturating_add(blocks);
    }

    Ok(latest)
}

pub fn write_file<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
    bytes: &[u8],
) -> Result<bool, FsError<D::Error>> {
    if name.is_empty() || name.as_bytes().len() > (u16::MAX as usize) {
        return Ok(false);
    }

    let Some((sb, entry_lba, blocks)) = check_space_for_put(dev, params, name.as_bytes().len(), bytes.len())? else {
        return Ok(false);
    };

    let mut off = 0usize;
    let mut src = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> {
        if off >= bytes.len() {
            return Ok(0);
        }
        let take = core::cmp::min(dst.len(), bytes.len() - off);
        dst[..take].copy_from_slice(&bytes[off..off + take]);
        off = off.saturating_add(take);
        Ok(take)
    };

    write_put_entry(dev, entry_lba, blocks, name, &mut src, bytes.len() as u64)?;
    advance_log_head(dev, params, sb, blocks)?;
    Ok(true)
}

pub fn read_file<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<Option<Vec<u8>>, FsError<D::Error>> {
    let Some(rec) = find_latest_record(dev, params, name)? else {
        return Ok(None);
    };

    let mut out = vec![0u8; rec.data_len as usize];
    read_exact_bytes(dev, rec.data_lba, 0, &mut out)?;

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

/// Read a file's data from a specific log entry LBA.
///
/// This is intended for callers that maintain an in-memory `name -> entry_lba`
/// index (e.g. via index checkpoints + tail replay).
///
/// Returns `Ok(None)` if the entry is invalid, not committed, or not a Put.
pub fn read_file_at<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    entry_lba: u64,
) -> Result<Option<Vec<u8>>, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }

    // Basic bounds check (best-effort).
    if entry_lba < params.data_lba {
        return Ok(None);
    }
    let end_lba = disk_data_end_lba_exclusive(dev, params);
    if entry_lba >= end_lba {
        return Ok(None);
    }

    let hdr_block = read_one_block(dev, entry_lba)?;
    let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
        return Ok(None);
    };
    if !hdr.committed || hdr.kind != LogKind::Put {
        return Ok(None);
    }

    let name_len = hdr.name_len as usize;
    let data_len = hdr.data_len as usize;
    if name_len == 0 || name_len > 4096 {
        return Ok(None);
    }

    let name_blocks = (name_len + (bs - 1)) / bs;
    let data_lba = entry_lba
        .saturating_add(1)
        .saturating_add(name_blocks as u64);
    if data_lba >= end_lba {
        return Ok(None);
    }

    let mut out = vec![0u8; data_len];
    read_exact_bytes(dev, data_lba, 0, &mut out)?;

    let mut hasher = Sha256::new();
    hasher.update(&out);
    let digest = hasher.finalize();
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&digest[..]);
    if sha != hdr.sha256 {
        return Ok(None);
    }

    Ok(Some(out))
}

/// Read a file from a specific entry LBA, but also verify the entry's stored
/// name matches `name`.
///
/// This is meant for index-based lookups where a stale/corrupted index must not
/// cause returning the wrong file's contents.
pub fn read_file_at_for_name<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
    entry_lba: u64,
) -> Result<Option<Vec<u8>>, FsError<D::Error>> {
    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    if name.is_empty() {
        return Ok(None);
    }
    let name_bytes = name.as_bytes();
    if name_bytes.len() > (u16::MAX as usize) {
        return Ok(None);
    }

    // Bounds check (best-effort).
    if entry_lba < params.data_lba {
        return Ok(None);
    }
    let end_lba = disk_data_end_lba_exclusive(dev, params);
    if entry_lba >= end_lba {
        return Ok(None);
    }

    let hdr_block = read_one_block(dev, entry_lba)?;
    let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
        return Ok(None);
    };
    if !hdr.committed || hdr.kind != LogKind::Put {
        return Ok(None);
    }

    let stored_name_len = hdr.name_len as usize;
    if stored_name_len != name_bytes.len() {
        return Ok(None);
    }

    let name_blocks = (stored_name_len + (bs - 1)) / bs;
    let name_lba = entry_lba.saturating_add(1);
    let mut stored_name = vec![0u8; stored_name_len];
    read_exact_bytes(dev, name_lba, 0, &mut stored_name)?;
    if stored_name != name_bytes {
        return Ok(None);
    }

    let data_lba = entry_lba
        .saturating_add(1)
        .saturating_add(name_blocks as u64);
    if data_lba >= end_lba {
        return Ok(None);
    }

    let data_len = hdr.data_len as usize;
    let mut out = vec![0u8; data_len];
    read_exact_bytes(dev, data_lba, 0, &mut out)?;

    let mut hasher = Sha256::new();
    hasher.update(&out);
    let digest = hasher.finalize();
    let mut sha = [0u8; 32];
    sha.copy_from_slice(&digest[..]);
    if sha != hdr.sha256 {
        return Ok(None);
    }

    Ok(Some(out))
}

pub fn delete_file<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<bool, FsError<D::Error>> {
    if name.is_empty() || name.as_bytes().len() > (u16::MAX as usize) {
        return Ok(false);
    }
    if find_latest_record(dev, params, name)?.is_none() {
        return Ok(false);
    }

    let Some(base) = find_latest_record(dev, params, name)? else {
        return Ok(false);
    };

    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };
    let entry_lba = params.data_lba.saturating_add(sb.log_head_rel_blocks);

    let blocks = entry_blocks(bs, name.as_bytes().len(), DELETE_REF_BYTES);
    let end_lba = disk_data_end_lba_exclusive(dev, params);
    if entry_lba.saturating_add(blocks) > end_lba {
        return Ok(false);
    }

    let written_blocks = write_delete_entry(dev, entry_lba, name, base.entry_lba, base.sha256)?;
    advance_log_head(dev, params, sb, written_blocks)?;
    Ok(true)
}

pub fn file_valid<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<bool, FsError<D::Error>> {
    let Some(rec) = find_latest_record(dev, params, name)? else {
        return Ok(false);
    };
    let sha = compute_sha256_of_entry_data(dev, &rec)?;
    Ok(sha == rec.sha256)
}

pub fn file_exists<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
) -> Result<bool, FsError<D::Error>> {
    Ok(find_latest_record(dev, params, name)?.is_some())
}

fn normalize_rel_no_parent(path: &str) -> Option<String> {
    if path.is_empty() {
        return Some(String::new());
    }
    let mut out = String::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(part);
    }
    Some(out)
}

/// List immediate children of a directory, treating stored keys as `/`-separated paths.
///
/// Output is newline-separated entry names.
pub fn list_dir<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    dir: &str,
) -> Result<String, FsError<D::Error>> {
    let Some(rel) = normalize_rel_no_parent(dir) else {
        return Ok(String::new());
    };
    let prefix = if rel.is_empty() {
        String::new()
    } else {
        let mut p = rel;
        p.push('/');
        p
    };

    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let sb_block = read_one_block(dev, params.super_lba)?;
    let Some(sb) = parse_superblock(&sb_block) else {
        return Err(FsError::Corrupted);
    };
    let mut lba = params.data_lba;
    let end_lba = params.data_lba.saturating_add(sb.log_head_rel_blocks);

    let mut live: BTreeSet<String> = BTreeSet::new();

    while lba < end_lba {
        let hdr_block = read_one_block(dev, lba)?;
        let Some(hdr) = LogHeader::decode_from_block(&hdr_block) else {
            break;
        };
        if !hdr.committed {
            break;
        }

        let name_len = hdr.name_len as usize;
        let data_len = hdr.data_len as usize;
        match hdr.kind {
            LogKind::Put | LogKind::Delete => {
                if name_len == 0 || name_len > 4096 {
                    break;
                }
            }
            LogKind::IndexCheckpoint => {
                if name_len != 0 {
                    break;
                }
                if data_len < 8 {
                    break;
                }
            }
        }

        let name_blocks = (name_len + (bs - 1)) / bs;
        let data_blocks = (data_len + (bs - 1)) / bs;
        let blocks = 1u64
            .saturating_add(name_blocks as u64)
            .saturating_add(data_blocks as u64);

        if hdr.kind != LogKind::IndexCheckpoint {
            let name_lba = lba.saturating_add(1);
            let mut tmp_name = vec![0u8; name_len];
            read_exact_bytes(dev, name_lba, 0, &mut tmp_name)?;
            if let Ok(name) = core::str::from_utf8(&tmp_name) {
                match hdr.kind {
                    LogKind::Put => {
                        live.insert(String::from(name));
                    }
                    LogKind::Delete => {
                        let _ = live.remove(name);
                    }
                    LogKind::IndexCheckpoint => {}
                }
            }
        }

        lba = lba.saturating_add(blocks);
    }

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
                children.insert(String::from(seg));
            }
        } else {
            let seg = name.split('/').next().unwrap_or("");
            if !seg.is_empty() {
                children.insert(String::from(seg));
            }
        }
    }

    const MAX_LISTING_BYTES: usize = 64 * 1024;
    let mut out = String::new();
    for entry in children.iter() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(entry);
        if out.len() > MAX_LISTING_BYTES {
            return Ok(String::new());
        }
    }

    Ok(out)
}

pub fn append_file<D: BlockIo>(
    dev: &D,
    params: &FsParams,
    name: &str,
    append_bytes: &[u8],
) -> Result<bool, FsError<D::Error>> {
    if append_bytes.is_empty() {
        return Ok(true);
    }

    let Some(base) = find_latest_record(dev, params, name)? else {
        return write_file(dev, params, name, append_bytes);
    };

    let bs = dev.block_size();
    if bs == 0 {
        return Err(FsError::InvalidParam);
    }
    let base_name_blocks = ((base.name_len as usize) + (bs - 1)) / bs;
    let base_data_lba = base.entry_lba.saturating_add(1).saturating_add(base_name_blocks as u64);

    let new_len = (base.data_len as usize).saturating_add(append_bytes.len());
    let Some((sb, entry_lba, blocks)) = check_space_for_put(dev, params, name.as_bytes().len(), new_len)? else {
        return Ok(false);
    };

    struct Src<'a, D: BlockIo> {
        dev: &'a D,
        bs: usize,
        base_data_lba: u64,
        base_len: usize,
        base_pos: usize,
        append: &'a [u8],
        append_off: usize,
        cache_lba: Option<u64>,
        cache_block: Vec<u8>,
    }

    impl<'a, D: BlockIo> Src<'a, D> {
        fn fill(&mut self, dst: &mut [u8]) -> Result<usize, FsError<D::Error>> {
            if dst.is_empty() {
                return Ok(0);
            }
            if self.base_pos < self.base_len {
                let abs = self.base_pos;
                let lba = self.base_data_lba.saturating_add((abs / self.bs) as u64);
                let off = abs % self.bs;
                if self.cache_lba != Some(lba) {
                    self.cache_block = read_one_block(self.dev, lba)?;
                    self.cache_lba = Some(lba);
                }
                if self.cache_block.len() < self.bs {
                    return Err(FsError::Corrupted);
                }
                let remaining_base = self.base_len - self.base_pos;
                let take = core::cmp::min(core::cmp::min(self.bs - off, remaining_base), dst.len());
                dst[..take].copy_from_slice(&self.cache_block[off..off + take]);
                self.base_pos = self.base_pos.saturating_add(take);
                return Ok(take);
            }

            if self.append_off < self.append.len() {
                let take = core::cmp::min(dst.len(), self.append.len() - self.append_off);
                dst[..take].copy_from_slice(&self.append[self.append_off..self.append_off + take]);
                self.append_off = self.append_off.saturating_add(take);
                return Ok(take);
            }

            Ok(0)
        }
    }

    let mut src = Src {
        dev,
        bs,
        base_data_lba,
        base_len: base.data_len as usize,
        base_pos: 0,
        append: append_bytes,
        append_off: 0,
        cache_lba: None,
        cache_block: Vec::new(),
    };

    let mut src_fn = |dst: &mut [u8]| -> Result<usize, FsError<D::Error>> { src.fill(dst) };
    write_put_entry(dev, entry_lba, blocks, name, &mut src_fn, new_len as u64)?;
    advance_log_head(dev, params, sb, blocks)?;
    Ok(true)
}
