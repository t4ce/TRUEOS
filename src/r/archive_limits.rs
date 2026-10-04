//! Shared, bounded archive budgets for packing and extraction.
//! The voxygen seed archive contains about 413 MiB across 8,008 files.
pub const MAX_ARCHIVE_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_SOURCE_FILE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SOURCE_TOTAL_BYTES: usize = 512 * 1024 * 1024;
pub const MAX_ARCHIVE_ENTRIES: usize = 16_384;
pub const MAX_TAR_BYTES: usize = MAX_SOURCE_TOTAL_BYTES + MAX_ARCHIVE_ENTRIES * 4096 + 1024;
