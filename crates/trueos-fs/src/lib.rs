#![no_std]

pub const MAGIC: [u8; 8] = *b"TRUEOSFS";
pub const VERSION: u32 = 1;

// Superblock layout (little-endian):
// [0..8]   MAGIC
// [8..12]  VERSION
// [12..16] FLAGS (reserved)
// [16..24] LOG_HEAD_REL_BLOCKS: u64 (relative to data_lba)

pub const SUPERBLOCK_MIN_BYTES: usize = 24;

pub const SUPERBLOCK_FLAGS_OFF: usize = 12;
pub const SUPERBLOCK_LOG_HEAD_REL_OFF: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Superblock {
    pub version: u32,
    pub flags: u32,
    /// Next free block in the data region (relative to `data_lba_from_super(super_lba)`).
    pub log_head_rel_blocks: u64,
}

pub fn parse_superblock(block0: &[u8]) -> Option<Superblock> {
    if block0.len() < SUPERBLOCK_MIN_BYTES {
        return None;
    }
    if &block0[0..8] != &MAGIC {
        return None;
    }
    let version = u32::from_le_bytes([block0[8], block0[9], block0[10], block0[11]]);
    let flags = u32::from_le_bytes([
        block0[SUPERBLOCK_FLAGS_OFF],
        block0[SUPERBLOCK_FLAGS_OFF + 1],
        block0[SUPERBLOCK_FLAGS_OFF + 2],
        block0[SUPERBLOCK_FLAGS_OFF + 3],
    ]);
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
    Some(Superblock {
        version,
        flags,
        log_head_rel_blocks,
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
    block0[8..12].copy_from_slice(&sb.version.to_le_bytes());
    block0[SUPERBLOCK_FLAGS_OFF..SUPERBLOCK_FLAGS_OFF + 4].copy_from_slice(&sb.flags.to_le_bytes());
    block0[SUPERBLOCK_LOG_HEAD_REL_OFF..SUPERBLOCK_LOG_HEAD_REL_OFF + 8]
        .copy_from_slice(&sb.log_head_rel_blocks.to_le_bytes());
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
            version: VERSION,
            flags: 0,
            log_head_rel_blocks: 0,
        },
    );
}
