use alloc::string::String;

use embassy_executor::Spawner;

use super::{ShellBackend2, print_shell_line};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiPromptMode {
    Normal,
    Inteldev,
    Driverdev,
    NewChat,
}

impl AiPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Normal => Self::Inteldev,
            Self::Inteldev => Self::Driverdev,
            Self::Driverdev => Self::NewChat,
            Self::NewChat => Self::Normal,
        }
    }
}

pub(crate) enum SubmitResult {
    Queued,
    ResetToNormal,
}

pub(crate) fn submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mode: AiPromptMode,
    submitted: &str,
) -> SubmitResult {
    let shell_target_mask = super::output_target_for_backend(io);
    let trimmed = submitted.trim();

    if mode == AiPromptMode::NewChat {
        trueos_qjs::ai_task::forget_conversation(shell_target_mask);
        if trimmed.is_empty() {
            print_shell_line(io, "ai: conversation reset");
            return SubmitResult::ResetToNormal;
        }
    }

    let (web_search, file_search, mode_profile) = match mode {
        AiPromptMode::Normal | AiPromptMode::NewChat => (true, true, "normal"),
        AiPromptMode::Inteldev => (false, false, "inteldev"),
        AiPromptMode::Driverdev => (false, false, "driverdev"),
    };

    let entry = trueos_qjs::ai_task::AiInputEntry {
        text: String::from(trimmed),
        web_search,
        file_search,
        new_conversation: mode == AiPromptMode::NewChat,
        computer_use: false,
        mode_profile,
        shell_target_mask,
        request_id: 0,
    };

    match trueos_qjs::ai_task::ensure_started(spawner) {
        trueos_qjs::ai_task::EnsureStartedResult::Ready => {}
        trueos_qjs::ai_task::EnsureStartedResult::BrowserNotReady => {
            print_shell_line(io, "ai: browser not ready yet");
            return SubmitResult::Queued;
        }
        trueos_qjs::ai_task::EnsureStartedResult::SpawnFailed => {
            print_shell_line(io, "ai: ai-task start failed");
            return SubmitResult::Queued;
        }
    }

    if !trueos_qjs::ai_task::queue_ai_input(entry) {
        print_shell_line(io, "ai: ai-task not running");
        return SubmitResult::Queued;
    }

    if mode == AiPromptMode::NewChat {
        SubmitResult::ResetToNormal
    } else {
        SubmitResult::Queued
    }
}
