use alloc::format;
use alloc::string::String as AllocString;

use super::{
    MatrixTarget, ShellBackend2, matrix, matrix_target_for_backend, print_matrix_target_line,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LumenPromptMode {
    Default,
}

pub(crate) const DEFAULT_LUMEN_SLOT: &str = "LUM";

pub(crate) fn ensure_lumen_slot(output_mask: u8) -> matrix::MatrixSlotId {
    matrix::switch_active_slot(output_mask, DEFAULT_LUMEN_SLOT)
}

pub(crate) fn lumen_status(output_mask: u8) -> AllocString {
    let slot_id = matrix::slot_id_from_name(DEFAULT_LUMEN_SLOT);
    let active = matrix::active_slot_id(output_mask);
    if active.as_str() == DEFAULT_LUMEN_SLOT {
        format!("lumen(§{})", slot_id.as_str())
    } else {
        format!("lumen -> §{}", slot_id.as_str())
    }
}

#[embassy_executor::task(pool_size = 4)]
async fn shell_lumen_submit_task(target: MatrixTarget, text: AllocString) {
    if crate::lumen::lumen_service::submit_lumen_prompt(text.as_str()) {
        print_matrix_target_line(&target, "lumen: submitted");
    } else {
        print_matrix_target_line(&target, "lumen: service offline");
    }
}

pub(crate) fn submit(
    io: &'static dyn ShellBackend2,
    mode: LumenPromptMode,
    target: &MatrixTarget,
    line: &str,
) {
    let trimmed = line.trim();
    let _ = mode;
    if trimmed.is_empty() {
        return;
    }

    ensure_lumen_slot(target.output_mask);
    let active_target = matrix_target_for_backend(io);
    let text = AllocString::from(trimmed);
    match shell_lumen_submit_task(active_target.clone(), text.clone()) {
        Ok(token) => {
            if let Some(spawner) = crate::workers::pick_background_spawner() {
                spawner.spawn(token);
            } else {
                print_matrix_target_line(&active_target, "lumen: no worker spawner");
            }
        }
        Err(_) => print_matrix_target_line(&active_target, "lumen: submit busy"),
    }
}
