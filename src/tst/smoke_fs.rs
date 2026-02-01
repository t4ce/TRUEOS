use crate::disc::block;
use crate::v::disc::partition;
use core::sync::atomic::{AtomicBool, Ordering};
use embassy_time::{Duration as EmbassyDuration, Timer};

// Standard EFI System Partition type GUID.
// C12A7328-F81F-11D2-BA4B-00A0C93EC93B
const GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];

static BSP_SMOKE_REQUESTED: AtomicBool = AtomicBool::new(false);
static BSP_SMOKE_DONE: AtomicBool = AtomicBool::new(false);

/// Request that the BSP TrueOSFS smoke test run once.
///
/// Safe to call from hotplug/driver contexts (e.g. USB mass-storage attach).
/// The actual smoke test runs in [`bsp_smoke_service_task`].
pub(crate) fn request_bsp_smoke_test() {
    BSP_SMOKE_REQUESTED.store(true, Ordering::Release);
}

/// Background task that waits for [`request_bsp_smoke_test`] and then executes
/// the BSP TrueOSFS smoke test exactly once.
#[embassy_executor::task]
pub(crate) async fn bsp_smoke_service_task() {
    loop {
        if BSP_SMOKE_REQUESTED.swap(false, Ordering::AcqRel) {
            // Allow the USBMS device to settle after registration.
            Timer::after(EmbassyDuration::from_millis(100)).await;
            bsp_smoke_test_once_async().await;
            return;
        }
        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}

#[inline]
fn looks_like_trueos_superblock(block0: &[u8]) -> bool {
    block0.len() >= 8 && &block0[0..8] == &trueos_fs::MAGIC
}

#[inline]
fn is_transient_io(e: block::Error) -> bool {
    matches!(e, block::Error::NotReady | block::Error::Timeout | block::Error::Io)
}

async fn read_blocks_aligned_retry_async(
    handle: block::DeviceHandle,
    lba: u64,
    blocks: usize,
    attempts: u8,
) -> Result<alloc::vec::Vec<u8>, block::Error> {
    let mut last: Option<block::Error> = None;
    let mut i = 0u8;
    while i < attempts {
        match handle.read_blocks(lba, blocks).await {
            Ok(v) => return Ok(v),
            Err(e) if is_transient_io(e) => {
                last = Some(e);
                // Give USB storage some time to become ready after heavy writes.
                Timer::after(EmbassyDuration::from_millis(10)).await;
            }
            Err(e) => return Err(e),
        }
        i = i.wrapping_add(1);
    }
    Err(last.unwrap_or(block::Error::Io))
}

pub(crate) async fn bsp_smoke_test_once_async() {
    if BSP_SMOKE_DONE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    crate::log!("trueosfs-smoke: starting (bsp)\n");

    let devices = block::devices();
    if devices.is_empty() {
        crate::log!("trueosfs-smoke: no block devices; skipping\n");
        return;
    }

    // 1) Log present discs (whole devices).
    let discs: alloc::vec::Vec<_> = devices.iter().filter(|d| d.parent.is_none()).collect();
    crate::log!(
        "trueosfs-smoke: scan: devices={} root_discs={}\n",
        devices.len(),
        discs.len()
    );
    for info in discs.iter().copied() {
        crate::log!(
            "trueosfs-smoke: disc: id={} kind={:?} block_size={} blocks={} writable={} label={:?} pci={:?}\n",
            info.id,
            info.kind,
            info.block_size,
            info.block_count,
            info.writable,
            info.label,
            info.pci
        );
    }

    // 2) Detect classification for each disc.
    let mut trueos_disk: Option<block::DeviceHandle> = None;
    for h in block::device_handles().into_iter() {
        if h.parent().is_some() {
            continue;
        }

        // Debug convenience: if we see a GPT disk with an ESP (FAT) and a *blank* data partition,
        // auto-format TRUEOSFS into that partition so smoke testing works on fresh images.
        //
        // Safety properties:
        // - Only in debug builds.
        // - Only if the target partition's first LBA reads as all-zero.
        // - Only if the whole disk is writable.
        #[cfg(debug_assertions)]
        if trueos_disk.is_none() {
            for h in block::device_handles().into_iter() {
                if h.parent().is_some() {
                    continue;
                }
                if !h.supports_write() {
                    continue;
                }

                let parts = match partition::read_gpt_partitions(h).await {
                    Ok(p) => p,
                    Err(e) => {
                        if is_transient_io(e) {
                            continue;
                        }
                        crate::log!(
                            "trueosfs-smoke: gpt: read partitions failed dev={} err={:?}\n",
                            h.id(),
                            e
                        );
                        continue;
                    }
                };
                if parts.is_empty() {
                    continue;
                }

                let mut candidate_super_lba: Option<u64> = None;
                for p in parts.iter() {
                    // Skip ESP.
                    if p.type_guid.as_bytes() == &GPT_TYPE_EFI_SYSTEM_PARTITION_BYTES {
                        continue;
                    }
                    let p0 = match read_blocks_aligned_retry_async(h, p.range.first_lba(), 1, 3).await {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if p0.iter().all(|&b| b == 0) {
                        candidate_super_lba = Some(p.range.first_lba());
                        break;
                    }
                }

                let Some(super_lba) = candidate_super_lba else {
                    continue;
                };

                crate::log!(
                    "trueosfs-smoke: debug: gpt blank data partition dev={} super_lba={} -> formatting TRUEOSFS\n",
                    h.id(),
                    super_lba
                );
                match crate::v::fs::trueosfs::format_blank_at_async(h, super_lba).await {
                    Ok(()) => {
                        crate::log!(
                            "trueosfs-smoke: debug: format ok dev={} super_lba={} -> mount_root_async\n",
                            h.id(),
                            super_lba
                        );
                    }
                    Err(e) => {
                        crate::log!(
                            "trueosfs-smoke: debug: format failed dev={} super_lba={} err={:?}\n",
                            h.id(),
                            super_lba,
                            e
                        );
                        continue;
                    }
                }

                match crate::v::fs::trueosfs::mount_root_async(h).await {
                    Ok(Some(_)) => {
                        trueos_disk = Some(h);
                        break;
                    }
                    Ok(None) => {
                        crate::log!(
                            "trueosfs-smoke: debug: mount_root_async returned None after format dev={}\n",
                            h.id()
                        );
                    }
                    Err(e) => {
                        crate::log!(
                            "trueosfs-smoke: debug: mount_root_async failed after format dev={} err={:?}\n",
                            h.id(),
                            e
                        );
                    }
                }
            }
        }

        // Disks can briefly report transient I/O errors right after bring-up.
        // Retry a handful of times so BSP logs reflect the steady state.
        let mut last = (crate::v::disc::detect::DiscStatus::Unknown, None);
        let mut tries = 0u8;
        while tries < 10 {
            let r = crate::v::disc::detect::detect_physical_disk_detail(h).await;
            match r.1 {
                Some(e) if is_transient_io(e) => {
                    last = r;
                    Timer::after(EmbassyDuration::from_millis(25)).await;
                }
                _ => {
                    last = r;
                    break;
                }
            }
            tries = tries.wrapping_add(1);
        }

        let (status, err) = last;
        if let Some(e) = err {
            crate::log!(
                "trueosfs-smoke: disc-detect: dev={} => {} (err={:?})\n",
                h.id(),
                status.short(),
                e
            );
        } else {
            crate::log!(
                "trueosfs-smoke: disc-detect: dev={} => {}\n",
                h.id(),
                status.short()
            );
        }

        if trueos_disk.is_none() {
            if let crate::v::disc::detect::DiscStatus::Trueos { .. } = status {
                trueos_disk = Some(h);
            }
        }
    }

    // Debug convenience: allow the BSP smoke test to operate on a fresh, unformatted
    // `disk.img` by formatting a *completely blank* disk as data-only TRUEOSFS.
    //
    // Safety properties:
    // - Only in debug builds.
    // - Only if LBA0 is all-zero (strong signal of "empty" media).
    // - Only if the device is writable.
    #[cfg(debug_assertions)]
    if trueos_disk.is_none() {
        for h in block::device_handles().into_iter() {
            if h.parent().is_some() {
                continue;
            }
            if !h.supports_write() {
                continue;
            }

            let bs0 = match read_blocks_aligned_retry_async(h, 0, 1, 3).await {
                Ok(v) => v,
                Err(e) => {
                    crate::log!(
                        "trueosfs-smoke: debug: blank-check read LBA0 failed dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                    continue;
                }
            };

            if bs0.iter().any(|&b| b != 0) {
                continue;
            }

            crate::log!(
                "trueosfs-smoke: debug: blank disk dev={} -> formatting TRUEOSFS\n",
                h.id()
            );
            match crate::v::fs::trueosfs::format_blank_async(h).await {
                Ok(()) => {
                    crate::log!(
                        "trueosfs-smoke: debug: format ok dev={} -> mount_root_async\n",
                        h.id()
                    );
                }
                Err(e) => {
                    crate::log!(
                        "trueosfs-smoke: debug: format failed dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                    continue;
                }
            }

            match crate::v::fs::trueosfs::mount_root_async(h).await {
                Ok(Some(_)) => {
                    trueos_disk = Some(h);
                    break;
                }
                Ok(None) => {
                    crate::log!(
                        "trueosfs-smoke: debug: mount_root_async returned None after format dev={}\n",
                        h.id()
                    );
                }
                Err(e) => {
                    crate::log!(
                        "trueosfs-smoke: debug: mount_root_async failed after format dev={} err={:?}\n",
                        h.id(),
                        e
                    );
                }
            }
        }
    }

    // 3) If we have a TRUEOS partition, do a write/read smoke test.
    let Some(disk) = trueos_disk else {
        crate::log!("trueosfs-smoke: no TRUEOSFS disk found\n");
        return;
    };

    let placement = match crate::v::fs::trueosfs::locate_async(disk).await {
        Ok(Some(v)) => v,
        Ok(None) => {
            crate::log!("trueosfs-smoke: locate: detect=trueos but locate_async returned None\n");
            return;
        }
        Err(e) => {
            crate::log!("trueosfs-smoke: locate: failed err={:?}\n", e);
            return;
        }
    };
    crate::log!(
        "trueosfs-smoke: target: dev={} bootable={} super_lba={} data_lba={} end={:?} writable={}\n",
        disk.id(),
        placement.bootable,
        placement.super_lba,
        placement.data_lba,
        placement.data_end_lba_exclusive,
        disk.supports_write()
    );

    // Async-safe verification: re-read the superblock and parse it.
    let info = disk.info();
    let bs = info.block_size as usize;
    if bs == 0 {
        crate::log!("trueosfs-smoke: verify: invalid device block size\n");
        return;
    }
    match disk.read_blocks(placement.super_lba, 1).await {
        Ok(v) => {
            let b0 = &v[..core::cmp::min(bs, v.len())];
            if !looks_like_trueos_superblock(b0) {
                crate::log!("trueosfs-smoke: verify: superblock magic mismatch\n");
                return;
            }
            if let Some(sb) = trueos_fs::parse_superblock(b0) {
                crate::log!(
                    "trueosfs-smoke: verify: superblock ok log_head_rel_blocks={} checkpoint_rel_blocks={}\n",
                    sb.log_head_rel_blocks,
                    sb.checkpoint_rel_blocks
                );
            }
        }
        Err(e) => {
            crate::log!("trueosfs-smoke: verify: read superblock failed err={:?}\n", e);
            return;
        }
    }

    // Best-effort: ensure the root is registered for higher layers.
    let _ = crate::v::fs::trueosfs::mount_root_async(disk).await;

    crate::log!("trueosfs-smoke: ok (bsp)\n");
}
