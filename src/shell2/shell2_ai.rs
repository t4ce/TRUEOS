use alloc::string::String;

use embassy_executor::Spawner;

use super::{ShellBackend2, print_shell_line};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiPromptMode {
    Normal,
    WebSearch,
    FileSearch,
    NewChat,
    AiPc,
}

impl AiPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Normal => Self::WebSearch,
            Self::WebSearch => Self::FileSearch,
            Self::FileSearch => Self::NewChat,
            Self::NewChat => Self::AiPc,
            Self::AiPc => Self::Normal,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::WebSearch => "web",
            Self::FileSearch => "file",
            Self::NewChat => "newchat",
            Self::AiPc => "ai-pc",
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

    if mode == AiPromptMode::NewChat {
        trueos_qjs::ai_task::forget_conversation(shell_target_mask);
        print_shell_line(io, "ai: conversation reset");
        return SubmitResult::ResetToNormal;
    }

    let entry = trueos_qjs::ai_task::AiInputEntry {
        text: String::from(submitted.trim()),
        web_search: mode == AiPromptMode::WebSearch,
        file_search: mode == AiPromptMode::FileSearch,
        new_conversation: false,
        computer_use: mode == AiPromptMode::AiPc,
        shell_target_mask,
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

    SubmitResult::Queued
}
