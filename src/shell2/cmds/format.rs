use alloc::string::String;
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer, with_timeout};

use super::super::{
    MatrixTarget, ShellBackend2, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};
use crate::disc::block::{self, DeviceHandle};
use crate::shell2::CommandSessionInputResult;
use crate::shell2::shell2_cmd::{CommandSessionKind, ParseOutcome};

const FORMAT_STATUS_TIMEOUT_MS: u64 = 5_000;
const FORMAT_OPERATION_TIMEOUT_MS: u64 = 6_000;

pub(crate) fn print_format_disk_table(io: &'static dyn ShellBackend2) {
    let choices = super::tlb_helper::collect_top_level_disk_choices();
    super::tlb_helper::print_disk_choice_table(io, "format", "disk selection", choices.as_slice());
}

fn print_target_summary(io: &'static dyn ShellBackend2, disk: DeviceHandle, prefix: &str) {
    let info = disk.info();
    let (status, err) = crate::wait::spawn_and_wait_local(async move {
        match with_timeout(
            EmbassyDuration::from_millis(FORMAT_STATUS_TIMEOUT_MS),
            crate::r::disc::detect::detect_physical_disk_detail(disk),
        )
        .await
        {
            Ok(result) => result,
            Err(_timeout) => {
                (crate::r::disc::detect::DiscStatus::Unknown, Some(block::Error::Timeout))
            }
        }
    });

    let msg = alloc::format!(
        "{prefix}: target id={} ({}) blocks={} bs={} writable={} label={:?} status={}{}",
        info.id.raw(),
        info.id,
        info.block_count,
        info.block_size,
        info.writable,
        info.label,
        status.short(),
        match (&status, err) {
            (crate::r::disc::detect::DiscStatus::Unknown, Some(err)) => {
                alloc::format!(" err={:?}", err)
            }
            _ => String::new(),
        }
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn start_format_session_for_disk(
    io: &'static dyn ShellBackend2,
    disk: DeviceHandle,
    prefix: &str,
) -> ParseOutcome {
    print_target_summary(io, disk, prefix);
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

#[embassy_executor::task(pool_size = 2)]
async fn format_command_task(target: MatrixTarget, disk: DeviceHandle) {
    let task_target = target.clone();
    let result =
        with_timeout(EmbassyDuration::from_millis(FORMAT_OPERATION_TIMEOUT_MS), async move {
            Timer::after(EmbassyDuration::from_millis(1)).await;

            let log = |line: &str| {
                print_matrix_target_line(&task_target, line);
            };

            let info = disk.info();
            log(alloc::format!(
                "format: target id={} ({}) blocks={} bs={} writable={} label={:?}",
                info.id.raw(),
                info.id,
                info.block_count,
                info.block_size,
                info.writable,
                info.label,
            )
            .as_str());

            log("format: creating 1 partition + TRUEOSFS...");
            let parts = [crate::disc::install::gpt::GptPartitionSpec {
                type_guid: crate::r::disc::partition::GPT_TYPE_LINUX_FILESYSTEM_BYTES,
                name: "TRUEOS",
                size: crate::disc::install::gpt::PartitionSize::Remaining,
                attributes: 0,
            }];

            let mut step_log = |msg: &str| log(msg);
            match crate::disc::install::gpt::write_gpt_layout_with_log(disk, &parts, &mut step_log)
                .await
            {
                Ok(_) => match crate::r::disc::partition::register_gpt_partitions(disk).await {
                    Ok(reg) => {
                        if let Some(first) = reg.first() {
                            match block::device_handle(first.id) {
                                Some(part_handle) => {
                                    match crate::r::fs::trueosfs::format_blank_partition_async(
                                        part_handle,
                                    )
                                    .await
                                    {
                                        Ok(()) => {
                                            match crate::r::fs::trueosfs::remount_root_async(disk)
                                                .await
                                            {
                                                Ok(Some(_)) => {
                                                    let (status, err) =
                                                    crate::r::disc::detect::detect_physical_disk_detail(
                                                        disk,
                                                    )
                                                    .await;
                                                    log(alloc::format!(
                                                        "format: ok (status now: {}{})",
                                                        status.short(),
                                                        match (&status, err) {
                                                            (
                                                                crate::r::disc::detect::DiscStatus::Unknown,
                                                                Some(err),
                                                            ) => alloc::format!("; err={:?}", err),
                                                            _ => String::new(),
                                                        }
                                                    )
                                                    .as_str());
                                                }
                                                Ok(None) => {
                                                    log("format: remount failed (TRUEOSFS not found after format)");
                                                }
                                                Err(err) => {
                                                    log(alloc::format!(
                                                        "format: remount failed ({:?})",
                                                        err
                                                    )
                                                    .as_str());
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            log(alloc::format!(
                                                "format: TRUEOSFS failed ({:?})",
                                                err
                                            )
                                            .as_str());
                                        }
                                    }
                                }
                                None => log("format: partition disappeared after registration"),
                            }
                        } else {
                            log("format: no partition registered");
                        }
                    }
                    Err(err) => {
                        log(alloc::format!("format: partition register failed ({:?})", err)
                            .as_str());
                    }
                },
                Err(err) => log(alloc::format!("format: GPT write failed ({:?})", err).as_str()),
            }
        })
        .await;

    if result.is_err() {
        print_matrix_target_line(
            &target,
            alloc::format!(
                "format: cancelled after timeout ({}ms) while waiting for disk I/O/probe",
                FORMAT_OPERATION_TIMEOUT_MS
            )
            .as_str(),
        );
    }

    set_matrix_target_active(&target, false);
}
