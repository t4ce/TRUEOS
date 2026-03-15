use alloc::string::String;

use super::{ShellBackend2, print_shell_line};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum QjsPromptMode {
    Repl,
    Eval,
}

impl QjsPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Repl => Self::Eval,
            Self::Eval => Self::Repl,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Repl => "repl",
            Self::Eval => "eval",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScanMode {
    Normal,
    SingleQuote,
    DoubleQuote,
    Backtick,
    LineComment,
    BlockComment,
}

pub(crate) fn is_likely_valid(source: &str) -> bool {
    let src = source.trim();
    if src.is_empty() {
        return false;
    }

    let mut mode = ScanMode::Normal;
    let mut escaped = false;
    let mut stack: heapless::Vec<char, 64> = heapless::Vec::new();
    let mut prev = '\0';

    for ch in src.chars() {
        match mode {
            ScanMode::Normal => {
                if prev == '/' && ch == '/' {
                    mode = ScanMode::LineComment;
                    prev = '\0';
                    continue;
                }
                if prev == '/' && ch == '*' {
                    mode = ScanMode::BlockComment;
                    prev = '\0';
                    continue;
                }

                match ch {
                    '\'' => mode = ScanMode::SingleQuote,
                    '"' => mode = ScanMode::DoubleQuote,
                    '`' => mode = ScanMode::Backtick,
                    '(' | '[' | '{' => {
                        let _ = stack.push(ch);
                    }
                    ')' => {
                        if stack.pop() != Some('(') {
                            return false;
                        }
                    }
                    ']' => {
                        if stack.pop() != Some('[') {
                            return false;
                        }
                    }
                    '}' => {
                        if stack.pop() != Some('{') {
                            return false;
                        }
                    }
                    _ => {}
                }
            }
            ScanMode::SingleQuote => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '\'' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::DoubleQuote => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '"' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::Backtick => {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == '`' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::LineComment => {
                if ch == '\n' {
                    mode = ScanMode::Normal;
                }
            }
            ScanMode::BlockComment => {
                if prev == '*' && ch == '/' {
                    mode = ScanMode::Normal;
                    prev = '\0';
                    continue;
                }
            }
        }

        prev = ch;
    }

    mode == ScanMode::Normal && stack.is_empty()
}

pub(crate) fn submit(io: &'static dyn ShellBackend2, mode: QjsPromptMode, submitted: &str) {
    let source = submitted.trim();
    if source.is_empty() {
        print_shell_line(io, "qjs: empty input");
        return;
    }

    if !is_likely_valid(source) {
        print_shell_line(io, "qjs: input looks incomplete");
        return;
    }

    let entry = trueos_qjs::browser_task::QjsInputEntry {
        code: String::from(source),
        repl: mode == QjsPromptMode::Repl,
    };

    if !trueos_qjs::browser_task::queue_qjs_input(entry) {
        print_shell_line(io, "qjs: browser bridge not running");
        return;
    }

    let msg = alloc::format!("qjs: queued ({})", mode.label());
    print_shell_line(io, msg.as_str());
}
