#![allow(dead_code)]

use alloc::string::String;

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
            DiscStatus::Detected {
                fs: KnownFs::Ext, ..
            } => "detected (ext)",
            DiscStatus::Detected {
                fs: KnownFs::Ntfs, ..
            } => "detected (ntfs)",
            DiscStatus::Detected {
                fs: KnownFs::Exfat, ..
            } => "detected (exfat)",
            DiscStatus::Detected {
                fs: KnownFs::Fat, ..
            } => "detected (fat)",
        }
    }
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

pub async fn detect_physical_disk(handle: block::DeviceHandle) -> DiscStatus {
    detect_physical_disk_detail(handle).await.0
}

/// Like `detect_physical_disk`, but also returns a best-effort error reason when the
/// result is `Unknown` due to an I/O or parse failure.
pub async fn detect_physical_disk_detail(
    handle: block::DeviceHandle,
) -> (DiscStatus, Option<block::Error>) {
    // Only classify whole devices (not already-registered partitions).
    if handle.parent().is_some() {
        return (DiscStatus::Unknown, None);
    }

    let info = handle.info();
    if info.block_size != 512 {
        return (DiscStatus::Unknown, None);
    }
    if info.block_count < 8 {
        return (DiscStatus::Unknown, None);
    }

    // Read MBR/boot sector.
    let bs0 = match handle.read_blocks(0, 1).await {
        Ok(v) => v,
        Err(e) => {
            if handle.info().kind == block::DeviceKind::Nvme {
                crate::log!(
                    "disc-detect: dev={} stage=read_lba0 kind={:?} err={:?}\n",
                    handle.id(),
                    handle.info().kind,
                    e
                );
            }
            return (DiscStatus::Unknown, Some(e));
        }
    };

    // Quick signature checks for common formats.
    if looks_like_ntfs(&bs0) {
        return (
            DiscStatus::Detected {
                fs: KnownFs::Ntfs,
                detail: None,
            },
            None,
        );
    }
    if looks_like_exfat(&bs0) {
        return (
            DiscStatus::Detected {
                fs: KnownFs::Exfat,
                detail: None,
            },
            None,
        );
    }

    // TRUEOSFS detection: the low-level placement logic decides whether this is
    // a bootable GPT layout (ESP + TRUEOS partition) or a data-only layout.
    match crate::v::fs::trueosfs::locate_async(handle).await {
        Ok(Some(loc)) => {
            return (
                DiscStatus::Trueos {
                    bootable: loc.bootable,
                },
                None,
            );
        }
        Ok(None) => {}
        Err(e) => {
            if handle.info().kind == block::DeviceKind::Nvme {
                crate::log!(
                    "disc-detect: dev={} stage=trueosfs_locate kind={:?} err={:?}\n",
                    handle.id(),
                    handle.info().kind,
                    e
                );
            }
            return (DiscStatus::Unknown, Some(e));
        }
    };

    // MBR-style FAT probe (superfloppy or partitioned) using the existing layout heuristic.
    if crate::disc::layout::probe_fat_volume(handle).await.is_ok() {
        return (
            DiscStatus::Detected {
                fs: KnownFs::Fat,
                detail: None,
            },
            None,
        );
    }

    // ext probe: read from 1024-byte offset (LBA2) for 2 sectors.
    match handle.read_blocks(2, 2).await {
        Ok(sb) => {
            if looks_like_ext_superblock(&sb) {
                return (
                    DiscStatus::Detected {
                        fs: KnownFs::Ext,
                        detail: None,
                    },
                    None,
                );
            }
        }
        Err(e) => {
            // If everything else failed and even this probe errors, surface it.
            if handle.info().kind == block::DeviceKind::Nvme {
                crate::log!(
                    "disc-detect: dev={} stage=read_ext_probe kind={:?} err={:?}\n",
                    handle.id(),
                    handle.info().kind,
                    e
                );
            }
            return (DiscStatus::Unknown, Some(e));
        }
    }

    (DiscStatus::Unknown, None)
}
