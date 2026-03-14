use alloc::string::String;

use super::{ShellBackend2, print_shell_line};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AiPromptMode {
    Normal,
    WebSearch,
    FileSearch,
    NewChat,
}

impl AiPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Normal => Self::WebSearch,
            Self::WebSearch => Self::FileSearch,
            Self::FileSearch => Self::NewChat,
            Self::NewChat => Self::Normal,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::WebSearch => "web",
            Self::FileSearch => "file",
            Self::NewChat => "newchat",
        }
    }
}

pub(crate) fn submit(io: &'static dyn ShellBackend2, mode: AiPromptMode, submitted: &str) {
    let entry = trueos_qjs::browser_task::AiInputEntry {
        text: String::from(submitted.trim()),
        web_search: mode == AiPromptMode::WebSearch,
        file_search: mode == AiPromptMode::FileSearch,
        new_conversation: mode == AiPromptMode::NewChat,
        computer_use: true,
    };

    if !trueos_qjs::browser_task::queue_ai_input(entry) {
        print_shell_line(io, "ai: browser/ai bridge not running");
        return;
    }

    let msg = alloc::format!("ai: queued ({})", mode.label());
    print_shell_line(io, msg.as_str());
}
