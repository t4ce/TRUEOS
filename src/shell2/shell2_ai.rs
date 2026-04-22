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
    let _ = spawner;
    let trimmed = submitted.trim();

    if mode == AiPromptMode::NewChat {
        if trimmed.is_empty() {
            print_shell_line(io, "ai: removed");
            return SubmitResult::ResetToNormal;
        }
    }

    if !trimmed.is_empty() {
        print_shell_line(io, "ai: removed from trueos-qjs");
    }

    if mode == AiPromptMode::NewChat {
        SubmitResult::ResetToNormal
    } else {
        SubmitResult::Queued
    }
}
