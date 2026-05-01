use alloc::format;
use alloc::string::String as AllocString;
use core::sync::atomic::{AtomicU32, Ordering};

use super::{
    MatrixTarget, ShellBackend2, matrix, matrix_target_for_backend, print_matrix_target_line,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChatPromptMode {
    Default,
    Add,
}

impl ChatPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Default => Self::Add,
            Self::Add => Self::Default,
        }
    }
}

static NEXT_CHAT_CHANNEL: AtomicU32 = AtomicU32::new(1);
pub(crate) const DEFAULT_CHAT_SLOT: &str = "LUM";

fn channel_slot_label(slot_id: &matrix::MatrixSlotId) -> AllocString {
    if slot_id.is_empty() || slot_id.as_str() == DEFAULT_CHAT_SLOT {
        AllocString::from("Default Channel")
    } else if let Some(id) = slot_id.as_str().strip_prefix("AI") {
        format!("channel{}", id)
    } else {
        format!("channel{}", slot_id.as_str())
    }
}

pub(crate) fn ensure_default_channel(output_mask: u8) -> matrix::MatrixSlotId {
    let slot_id = matrix::active_slot_id(output_mask);
    if slot_id.is_empty() {
        matrix::switch_active_slot(output_mask, DEFAULT_CHAT_SLOT)
    } else {
        slot_id
    }
}

pub(crate) fn active_channel_status(output_mask: u8) -> AllocString {
    let mut slot_id = matrix::active_slot_id(output_mask);
    if slot_id.is_empty() {
        slot_id = matrix::slot_id_from_name(DEFAULT_CHAT_SLOT);
    }
    let mut out = channel_slot_label(&slot_id);
    out.push_str("(§");
    out.push_str(slot_id.as_str());
    out.push(')');
    out
}

fn create_channel(io: &'static dyn ShellBackend2, current: &MatrixTarget) {
    let id = NEXT_CHAT_CHANNEL.fetch_add(1, Ordering::Relaxed);
    let requested = format!("AI{}", id);
    let slot = matrix::switch_active_slot(current.output_mask, requested.as_str());
    let target = MatrixTarget {
        output_mask: current.output_mask,
        slot_id: slot.clone(),
    };
    print_matrix_target_line(
        &target,
        format!("chat: created channel{} (§{})", id, slot.as_str()).as_str(),
    );
    let _ = io;
}

pub(crate) fn submit(
    io: &'static dyn ShellBackend2,
    mode: ChatPromptMode,
    target: &MatrixTarget,
    line: &str,
) {
    let trimmed = line.trim();
    if mode == ChatPromptMode::Add || trimmed == "+" {
        create_channel(io, target);
        return;
    }
    if trimmed.is_empty() {
        return;
    }

    ensure_default_channel(target.output_mask);
    let active_target = matrix_target_for_backend(io);
    crate::r::lumen_service::submit_chat(active_target, trimmed);
}
