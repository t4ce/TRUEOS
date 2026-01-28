use alloc::string::String;
use alloc::vec::Vec;

use crate::disc::block;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KnownFs {
    Ext,
    Ntfs,
    Exfat,
    Fat,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiscStatus {
    /// No recognizable structure.
    Unknown,

    /// Our filesystem exists somewhere on this disk.
    ///
    /// `bootable` here means "has an EFI system partition" (structural bootability).
    /// We intentionally do not parse files on that partition at this stage.
    Trueos { bootable: bool },

    /// A recognized non-TRUEOS format.
    Detected { fs: KnownFs, detail: Option<String> },
}

impl DiscStatus {
    pub fn short(&self) -> &'static str {
        match self {
            DiscStatus::Unknown => "unknown",
            DiscStatus::Trueos { bootable: true } => "trueos (bootable)",
            DiscStatus::Trueos { bootable: false } => "trueos (data-only)",
            DiscStatus::Detected { fs: KnownFs::Ext, .. } => "detected (ext)",
            DiscStatus::Detected { fs: KnownFs::Ntfs, .. } => "detected (ntfs)",
            DiscStatus::Detected { fs: KnownFs::Exfat, .. } => "detected (exfat)",
            DiscStatus::Detected { fs: KnownFs::Fat, .. } => "detected (fat)",
        }
    }
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

fn looks_like_ntfs(bs0: &[u8]) -> bool {
    bs0.len() >= 11 && &bs0[3..11] == b"NTFS    "
}

fn looks_like_exfat(bs0: &[u8]) -> bool {
    bs0.len() >= 11 && &bs0[3..11] == b"EXFAT   "
}

fn looks_like_ext_superblock(sb: &[u8]) -> bool {
    // ext superblock magic is 0xEF53 at offset 56 from superblock start.
    sb.len() >= 58 && sb[56] == 0x53 && sb[57] == 0xEF
}

pub fn detect_physical_disk(handle: block::DeviceHandle) -> DiscStatus {
    // Only classify whole devices (not already-registered partitions).
    if handle.parent().is_some() {
        return DiscStatus::Unknown;
    }

    let info = handle.info();
    if info.block_size != 512 {
        return DiscStatus::Unknown;
    }
    if info.block_count < 8 {
        return DiscStatus::Unknown;
    }

    // Read MBR/boot sector.
    let bs0 = match read_blocks_aligned(handle, 0, 1) {
        Ok(v) => v,
        Err(_) => return DiscStatus::Unknown,
    };

    // Quick signature checks for common formats.
    if looks_like_ntfs(&bs0) {
        return DiscStatus::Detected {
            fs: KnownFs::Ntfs,
            detail: None,
        };
    }
    if looks_like_exfat(&bs0) {
        return DiscStatus::Detected {
            fs: KnownFs::Exfat,
            detail: None,
        };
    }

    // TRUEOSFS detection: the low-level placement logic decides whether this is
    // a bootable GPT layout (ESP + TRUEOS partition) or a data-only layout.
    if let Ok(Some(loc)) = crate::disc::trueosfs::locate(handle) {
        return DiscStatus::Trueos { bootable: loc.bootable };
    }

    // MBR-style FAT probe (superfloppy or partitioned) using the existing layout heuristic.
    if crate::disc::layout::probe_fat_volume(handle).is_ok() {
        return DiscStatus::Detected {
            fs: KnownFs::Fat,
            detail: None,
        };
    }

    // ext probe: read from 1024-byte offset (LBA2) for 2 sectors.
    if let Ok(sb) = read_blocks_aligned(handle, 2, 2) {
        if looks_like_ext_superblock(&sb) {
            return DiscStatus::Detected {
                fs: KnownFs::Ext,
                detail: None,
            };
        }
    }

    DiscStatus::Unknown
}
