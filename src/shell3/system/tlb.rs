//! Native TLB selector and return to the originating Matrix slot.
use super::output::{DumpTarget as MatrixTarget, Output};
use crate::efi::acpi::tlb::{
    TLB_ACPI_USAGE, TLB_AML_USAGE, TLB_MENU_ROWS, execute, parse_acpi_signature,
};
use alloc::{string::String, vec::Vec};
// Preserve the existing in-kernel export entry points for callers.

use super::{
    helper::{self, Screen},
    picker::{Event, Picker},
    tui::{self, Frontend},
};
use spin::Mutex;
use trueos_time::{Duration, Timer};
static ORIGINS: Mutex<Vec<(super::MatrixSlotLease, MatrixTarget)>> = Mutex::new(Vec::new());
static RETURNS: Mutex<Vec<(u64, MatrixTarget)>> = Mutex::new(Vec::new());
pub(super) fn take_return(frontend: u64) -> Option<MatrixTarget> {
    let mut entries = RETURNS.lock();
    let index = entries.iter().position(|(id, _)| *id == frontend)?;
    Some(entries.remove(index).1)
}
pub(super) fn start(frontend: Frontend, origin: MatrixTarget) -> Result<(), String> {
    let Some((target, spawner)) = helper::admit("tlb", frontend)? else {
        return Ok(());
    };
    let task = menu_task(target.clone()).map_err(|_| {
        tui::cancel_native_attach(&target);
        String::from("TLB menu task pool is full.")
    })?;
    {
        let mut origins = ORIGINS.lock();
        let lease = super::matrix_target_slot_lease(&target);
        origins.retain(|(slot, _)| slot != &lease);
        origins.push((lease, origin));
    }
    spawner.spawn(task);
    Ok(())
}
fn selected_command(index: usize) -> Option<String> {
    if index < TLB_MENU_ROWS.len() {
        Some(TLB_MENU_ROWS[index].0.into())
    } else if index == TLB_MENU_ROWS.len() {
        Some("rapl store".into())
    } else {
        None
    }
}
fn close_selector(
    target: &super::MatrixTarget,
    frontend: u64,
    return_origin: bool,
) -> Option<MatrixTarget> {
    let mut origins = ORIGINS.lock();
    let lease = super::matrix_target_slot_lease(target);
    let index = origins.iter().position(|(slot, _)| slot == &lease)?;
    let origin = origins.remove(index).1;
    drop(origins);
    if return_origin {
        RETURNS.lock().push((frontend, origin.clone()));
    }
    super::MatrixSlots::drop_slot(Some(super::matrix_target_slot_lease(target).name()));
    super::service::notify_work();
    Some(origin)
}
#[trueos_executor::task(pool_size = 2)]
async fn menu_task(target: super::MatrixTarget) {
    let mut picker = Picker::default();
    let mut screen = Screen::default();
    let mut field: Option<String> = None;
    let mut message = String::from("Choose a table to dump into the originating Matrix slot.");
    let mut labels: Vec<String> = TLB_MENU_ROWS
        .iter()
        .map(|(name, description)| alloc::format!("{name} — {description}"))
        .collect();
    labels.push("rapl store — Save RAPL history to TRUEOSFS".into());
    labels.push("Return".into());
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        let Some(frontend) = tui::native_frontend(&target) else {
            break;
        };
        let (cols, rows) = (frontend.cols, frontend.rows);
        let now = crate::chronos::monotonic_nanos();
        let mut command = None;
        if let Some(event) = picker.input(&bytes, labels.len(), rows, now) {
            match event {
                Event::Choose(index) => {
                    if let Some(selected) = selected_command(index) {
                        if selected == "acpi" || selected == "aml" {
                            picker.form(if selected == "acpi" {
                                "Signature [index] (empty lists ACPI tables)"
                            } else {
                                "ec | symbol PATH | prefix PATH (empty lists AML)"
                            });
                            field = Some(selected);
                        } else {
                            command = Some(selected);
                        }
                    } else {
                        close_selector(&target, frontend.id, true);
                        break;
                    }
                }
                Event::Submit(value) => {
                    if let Some(prefix) = &field {
                        let selected = alloc::format!("{} {}", prefix, value.trim());
                        if valid_selection(&selected) {
                            command = Some(selected);
                        } else {
                            message = if prefix == "acpi" {
                                TLB_ACPI_USAGE.into()
                            } else {
                                TLB_AML_USAGE.into()
                            };
                        }
                    }
                }
                Event::Quit => {
                    if field.take().is_some() {
                        picker.clear_form();
                    } else {
                        close_selector(&target, frontend.id, true);
                        break;
                    }
                }
            }
        }
        if let Some(command) = command {
            let Some(spawner) = crate::workers::pick_background_spawner() else {
                message = "No background worker is ready.".into();
                continue;
            };
            if let Some(origin) = close_selector(&target, frontend.id, true) {
                let output = Output::slot(origin, cols);
                execute(&spawner, &output, &mut command.split_whitespace());
            }
            break;
        }
        let lines = picker.frame("TLB  hardware tables", &message, &labels, rows);
        screen.paint(&target, &lines, cols, rows);
        Timer::after(Duration::from_millis(20)).await;
    }
    let lease = super::matrix_target_slot_lease(&target);
    ORIGINS.lock().retain(|(slot, _)| slot != &lease);
}
fn valid_selection(command: &str) -> bool {
    let args: Vec<&str> = command.split_whitespace().collect();
    match args.as_slice() {
        ["acpi"] => true,
        ["acpi", signature] => parse_acpi_signature(signature).is_some(),
        ["acpi", signature, index] => {
            parse_acpi_signature(signature).is_some() && index.parse::<usize>().is_ok()
        }
        ["aml"] | ["aml", "ec"] => true,
        ["aml", "symbol" | "prefix", path] => !path.is_empty(),
        _ => false,
    }
}
