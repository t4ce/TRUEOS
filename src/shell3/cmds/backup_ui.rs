//! Independent built-in backup Blueprint. Its only privilege is returning a
//! validated selection; storage and network work stay in this shell session.
use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, matrix_target_interrupted,
    print_matrix_target_line, print_shell_line, switch_matrix_target_slot,
};
use crate::disc::{backup_local, block};
use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
use trueos_executor::{Spawner, task};
use trueos_time::{Duration, Instant, Timer};
const ARCHIVE: &str = "backup.bp";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
struct ImageChoice {
    root: block::DeviceHandle,
    artifact: backup_local::BackupArtifact,
}
fn clean(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() || c == '|' { ' ' } else { c })
        .collect()
}
pub(crate) fn start(spawner: &Spawner, io: &'static dyn ShellBackend2) -> ParseOutcome {
    let target = matrix_target_for_backend(io);
    match ui_task(*spawner, target) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "backup: a disk picker is already open"),
    }
    ParseOutcome::Handled
}
#[task(pool_size = 1)]
async fn ui_task(spawner: Spawner, target: MatrixTarget) {
    print_matrix_target_line(&target, "backup: scanning disks and completed /backups images…");
    let disks = super::tlb_helper::collect_top_level_disk_choices();
    let roots = crate::r::fs::trueosfs::list_roots();
    let mut args = Vec::new();
    let mut images = Vec::new();
    for disk in disks {
        if matrix_target_interrupted(&target) {
            return;
        }
        let handle = disk.handle;
        let free = if roots.iter().any(|r| r.disk_id == handle.id()) && handle.supports_write() {
            backup_local::available_backup_bytes_async(handle)
                .await
                .ok()
        } else {
            None
        };
        args.push(format!(
            "disk={}|{}|{}|{}|{}",
            handle.id().raw(),
            handle
                .block_count()
                .saturating_mul(handle.block_size() as u64),
            handle.block_size(),
            free.map(|n| format!("{n}"))
                .unwrap_or_else(|| String::from("-")),
            clean(&disk.label_text())
        ));
        if roots.iter().any(|r| r.disk_id == handle.id()) {
            match backup_local::list_local_backups_async(handle).await {
                Ok(artifacts) => {
                    for artifact in artifacts {
                        if images.len() >= 64 {
                            break;
                        }
                        args.push(format!(
                            "image={}|{}|{}|{}|{}|{}",
                            images.len(),
                            handle.id().raw(),
                            artifact.image_bytes,
                            artifact.block_size,
                            clean(artifact.source_label.as_deref().unwrap_or("whole disk")),
                            clean(&artifact.name)
                        ));
                        images.push(ImageChoice {
                            root: handle,
                            artifact,
                        });
                    }
                }
                Err(error) => print_matrix_target_line(
                    &target,
                    &format!("backup: could not list {} /backups: {error:?}", handle.id()),
                ),
            }
        }
    }
    let instance = format!("backup-{}", SEQUENCE.fetch_add(1, Ordering::AcqRel) + 1);
    if let Err(error) = super::run::submit_archive_name_to_target_from_app_db_with_instance_async(
        target.clone(),
        ARCHIVE,
        args,
        crate::hv::BlueprintInstanceRequest::named(instance.clone()),
    )
    .await
    {
        print_matrix_target_line(&target, &format!("backup: cannot launch {ARCHIVE}: {error}"));
        return;
    }
    let deadline = Instant::now().as_millis() + 30_000;
    let vm = loop {
        if let Some(vm) = crate::hv::named_app_instance_vms(ARCHIVE)
            .into_iter()
            .find_map(|(vm, name)| (name == instance).then_some(vm))
        {
            break vm;
        }
        if Instant::now().as_millis() >= deadline || matrix_target_interrupted(&target) {
            print_matrix_target_line(&target, "backup: disk picker launch cancelled or timed out");
            return;
        }
        Timer::after(Duration::from_millis(25)).await;
    };
    let deadline = Instant::now().as_millis() + 30_000;
    let mut active = false;
    let reason = loop {
        if let Some(reason) = crate::hv::blueprint_console_exit_reason(vm) {
            break reason;
        }
        let state = crate::hv::vm_state(vm);
        if state.running || state.starting {
            active = true;
        } else if active || Instant::now().as_millis() >= deadline {
            return;
        }
        if matrix_target_interrupted(&target) {
            let _ = crate::hv::stop(vm);
            return;
        }
        Timer::after(Duration::from_millis(25)).await;
    };
    let deadline = Instant::now().as_millis() + 5_000;
    loop {
        let state = crate::hv::vm_state(vm);
        if !state.running && !state.starting {
            break;
        }
        if Instant::now().as_millis() >= deadline {
            let _ = crate::hv::stop(vm);
            // Never perform privileged disk work before terminal teardown.
            print_matrix_target_line(
                &target,
                "backup: picker did not exit cleanly; operation cancelled",
            );
            return;
        }
        Timer::after(Duration::from_millis(25)).await;
    }
    let action_target = switch_matrix_target_slot(&target, "");
    dispatch(&spawner, action_target, &reason, &images);
}
fn disk(raw: &str) -> Option<block::DeviceHandle> {
    super::tlb_helper::parse_disc_id_raw(raw).and_then(super::tlb_helper::select_top_level_disk)
}
fn dispatch(spawner: &Spawner, target: MatrixTarget, reason: &str, images: &[ImageChoice]) {
    let fields: Vec<_> = reason.split(':').collect();
    match fields.as_slice() {
        ["backup", "quit" | "cancel"] => {}
        ["backup", "network", source] => {
            if let Some(source) = disk(source) {
                super::backup::submit(spawner, target, source);
            } else {
                print_matrix_target_line(&target, "backup: source disk is no longer available");
            }
        }
        ["backup", "local", source, destination] => {
            if let (Some(source), Some(destination)) = (disk(source), disk(destination)) {
                super::backup::submit_local(spawner, target, source, destination);
            } else {
                print_matrix_target_line(&target, "backup: a selected disk is no longer available");
            }
        }
        ["backup", "restore", destination, index] => {
            if let (Some(destination), Some(image)) = (
                disk(destination),
                index
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| images.get(index)),
            ) {
                super::backup::submit_restore(
                    spawner,
                    target,
                    image.root,
                    image.artifact.clone(),
                    destination,
                );
            } else {
                print_matrix_target_line(
                    &target,
                    "backup: restore selection is no longer available",
                );
            }
        }
        _ => print_matrix_target_line(&target, "backup: rejected unknown action"),
    }
}
