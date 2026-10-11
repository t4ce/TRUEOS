//! Disk administration, using existing formatting and RAM disk services.
use crate::disc::block::{self, DeviceHandle};
use alloc::string::String;
use trueos_time::{Duration as EmbassyDuration, Timer, with_timeout};
const RAMDISK_BLOCK_SIZE: u32 = 512;
const FORMAT_OPERATION_TIMEOUT_MS: u64 =
    crate::allcaps::storage::USB_MASS_UAS_IO_TIMEOUT_MS.saturating_add(5_000);
pub(super) fn start(frontend: super::tui::Frontend) -> Result<(), String> {
    super::admin::start("disc", frontend)
}
pub(crate) fn parse_size_bytes(raw: &str) -> Option<u64> {
    let text = raw.trim();
    if text.is_empty() {
        return None;
    }

    let digits_len = text.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits_len == 0 {
        return None;
    }

    let number = text[..digits_len].parse::<u64>().ok()?;
    let suffix = text[digits_len..].trim();
    let mul = if suffix.is_empty() {
        1_048_576u64
    } else if suffix.eq_ignore_ascii_case("B") {
        1u64
    } else if suffix.eq_ignore_ascii_case("KB") || suffix.eq_ignore_ascii_case("K") {
        1_000u64
    } else if suffix.eq_ignore_ascii_case("MB") || suffix.eq_ignore_ascii_case("M") {
        1_000_000u64
    } else if suffix.eq_ignore_ascii_case("GB") || suffix.eq_ignore_ascii_case("G") {
        1_000_000_000u64
    } else if suffix.eq_ignore_ascii_case("KIB") {
        1_024u64
    } else if suffix.eq_ignore_ascii_case("MIB") {
        1_048_576u64
    } else if suffix.eq_ignore_ascii_case("GIB") {
        1_073_741_824u64
    } else {
        return None;
    };

    number.checked_mul(mul)
}

pub(crate) async fn create_ramdisc_bytes(
    size_bytes: u64,
) -> Result<crate::disc::block::DeviceHandle, alloc::string::String> {
    let label = alloc::format!("ramdisc-{}mb", size_bytes / (1024 * 1024));
    let disk = crate::r::disc::ramdisk::create_trueos_public(size_bytes, RAMDISK_BLOCK_SIZE, label)
        .await
        .map_err(|error| alloc::format!("Create/format failed: {error:?}"))?;
    crate::r::fs::trueosfs::mount_root_async(disk)
        .await
        .map_err(|error| alloc::format!("Mount failed: {error:?}"))?;
    Ok(disk)
}

pub(crate) async fn format_disk(
    disk: DeviceHandle,
    mut log: impl FnMut(&str),
) -> Result<String, String> {
    with_timeout(EmbassyDuration::from_millis(FORMAT_OPERATION_TIMEOUT_MS), async {
        Timer::after(EmbassyDuration::from_millis(1)).await;
        let live = super::table::select_top_level_disk(disk.id().raw())
            .ok_or("Selected disk disappeared.")?;
        if live != disk {
            return Err(String::from("Selected disk was replaced."));
        }
        if !disk.info().writable {
            return Err(String::from("Selected disk is read-only."));
        }
        log("Creating GPT partition…");
        let parts = [crate::disc::install::gpt::GptPartitionSpec {
            type_guid: crate::r::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
            name: "TRUEOS",
            size: crate::disc::install::gpt::PartitionSize::Remaining,
            attributes: 0,
        }];
        crate::disc::install::gpt::write_gpt_layout_with_log(disk, &parts, &mut log)
            .await
            .map_err(|error| alloc::format!("GPT write failed: {error:?}"))?;
        let registered = crate::r::disc::partition::register_gpt_partitions(disk)
            .await
            .map_err(|error| alloc::format!("Partition registration failed: {error:?}"))?;
        let partition = registered
            .first()
            .and_then(|first| block::device_handle(first.id))
            .ok_or("No partition is available after registration.")?;
        log("Creating TRUEOSFS…");
        crate::r::fs::trueosfs::format_blank_partition_async(partition)
            .await
            .map_err(|error| alloc::format!("TRUEOSFS format failed: {error:?}"))?;
        log("Mounting TRUEOSFS…");
        crate::r::fs::trueosfs::remount_root_async(disk)
            .await
            .map_err(|error| alloc::format!("Remount failed: {error:?}"))?
            .ok_or("TRUEOSFS was not found after formatting.")?;
        Ok(alloc::format!("Formatted disc{} as TRUEOSFS.", disk.id().raw()))
    })
    .await
    .map_err(|_| alloc::format!("Disk I/O timed out after {} ms.", FORMAT_OPERATION_TIMEOUT_MS))?
}
