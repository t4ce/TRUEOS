use alloc::string::String;
use trueos_executor::Spawner;
use trueos_time::{Duration as EmbassyDuration, Timer, with_timeout};

use super::super::{
    MatrixTarget, ShellBackend2, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const FORMAT_OPERATION_TIMEOUT_MS: u64 =
    crate::allcaps::storage::USB_MASS_UAS_IO_TIMEOUT_MS.saturating_add(5_000);

pub(crate) fn print_format_disk_table(io: &'static dyn ShellBackend2) {
    let choices = super::tlb_helper::collect_top_level_disk_choices();
    super::tlb_helper::print_disk_choice_table(io, "format", "disk selection", choices.as_slice());
}

fn print_target_summary(io: &'static dyn ShellBackend2, disk: DeviceHandle, prefix: &str) {
    let info = disk.info();
    let msg = alloc::format!(
        "{prefix}: target id={} ({}) blocks={} bs={} writable={} label={:?}",
        info.id.raw(),
        info.id,
        info.block_count,
        info.block_size,
        info.writable,
        info.label,
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn start_format_session_for_disk(
    io: &'static dyn ShellBackend2,
    disk: DeviceHandle,
    prefix: &str,
) -> ParseOutcome {
    print_target_summary(io, disk, prefix);
    if !disk.info().writable {
        print_shell_line(io, &alloc::format!("{prefix}: refused; target is read-only"));
        return ParseOutcome::Handled;
    }
    print_shell_line(io, &alloc::format!("{prefix}: DANGER: this destroys all data on the disk"));
    print_shell_line(io, &alloc::format!("{prefix}: type `sure`"));
    ParseOutcome::StartSession(CommandSessionKind::FormatSure(disk.id().raw()))
}

pub(crate) fn handle_session_input(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    target: &MatrixTarget,
    submitted: &str,
    disc_id: u32,
) -> CommandSessionInputResult {
    if !submitted.eq_ignore_ascii_case("sure") {
        print_matrix_target_line(target, "format: cancelled");
        return CommandSessionInputResult::CompleteIdle;
    }

    let Some(disk) = super::tlb_helper::select_top_level_disk(disc_id) else {
        print_shell_line(io, "format: selected disk disappeared");
        return CommandSessionInputResult::CompleteIdle;
    };
    if !disk.info().writable {
        print_shell_line(io, "format: refused; selected disk is read-only");
        return CommandSessionInputResult::CompleteIdle;
    }

    submit_format(spawner, io, target, disk);
    CommandSessionInputResult::CompleteRunning
}

fn submit_format(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    target: &MatrixTarget,
    disk: DeviceHandle,
) {
    let info = disk.info();
    print_matrix_target_line(
        target,
        alloc::format!("format: starting on disk id={} ({})", info.id.raw(), info.id).as_str(),
    );

    set_matrix_target_active(target, true);
    match format_command_task(target.clone(), disk) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(target, false);
            print_shell_line(io, "format: spawn failed");
        }
    }
}

/// Shared formatting operation; the caller owns confirmation and work activity.
pub(crate) async fn format_disk(
    disk: DeviceHandle,
    mut log: impl FnMut(&str),
) -> Result<String, String> {
    with_timeout(EmbassyDuration::from_millis(FORMAT_OPERATION_TIMEOUT_MS), async {
        Timer::after(EmbassyDuration::from_millis(1)).await;
        let live = super::tlb_helper::select_top_level_disk(disk.id().raw())
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

#[trueos_executor::task(pool_size = 2)]
async fn format_command_task(target: MatrixTarget, disk: DeviceHandle) {
    let result = format_disk(disk, |line| print_matrix_target_line(&target, line)).await;
    let message = match result {
        Ok(message) => message,
        Err(error) => alloc::format!("Error: {error}"),
    };
    print_matrix_target_line(&target, &message);
    set_matrix_target_active(&target, false);
}
