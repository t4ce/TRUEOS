use alloc::format;
use alloc::string::{String, ToString};

use super::super::{ShellBackend2, line_width_for_backend, print_native_line, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

struct ShellTxtCallbacks {
    io: &'static dyn ShellBackend2,
    source_path: String,
}

impl ShellTxtCallbacks {
    fn new(io: &'static dyn ShellBackend2, source_path: &str) -> Self {
        Self {
            io,
            source_path: String::from(source_path),
        }
    }

    fn write_text(&self, action: &str, path: &str, text: &str) {
        match write_file(path, text.as_bytes()) {
            Ok(()) => print_shell_line(
                self.io,
                format!("txt: {action} path={} bytes={}", path, text.len()).as_str(),
            ),
            Err(err) => print_shell_line(
                self.io,
                format!("txt: {action} failed path={} err={:?}", path, err).as_str(),
            ),
        }
    }

    fn remove_source_if_needed(&self, new_path: &str) {
        if self.source_path == new_path {
            return;
        }
        let _ = crate::r::io::kfs::remove(self.source_path.as_str());
    }
}

impl trueos_txt::TxtCallbacks for ShellTxtCallbacks {
    fn set_page(&mut self, change: trueos_txt::PageChange) {
        print_shell_line(
            self.io,
            format!(
                "txt: page {} -> {} of {}",
                change.old_page + 1,
                change.new_page + 1,
                change.page_count
            )
            .as_str(),
        );
    }

    fn exit_replace(&mut self, text: &str) {
        let path = self.source_path.clone();
        self.write_text("wrote", path.as_str(), text);
    }

    fn exit_forget(&mut self) {
        print_shell_line(self.io, "txt: closed without writing");
    }

    fn exit_rename(&mut self, new_name: &str, text: &str) {
        self.write_text("renamed", new_name, text);
        self.remove_source_if_needed(new_name);
    }

    fn exit_move(&mut self, new_path: &str, text: &str) {
        self.write_text("moved", new_path, text);
        self.remove_source_if_needed(new_path);
    }
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let Some(first) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    match first {
        "new" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let text = args.collect::<alloc::vec::Vec<_>>().join(" ");
            let mut editor = trueos_txt::TxtEditor::new(path, text, metrics_for_backend(io));
            let mut callbacks = ShellTxtCallbacks::new(io, path);
            editor.exit(trueos_txt::ExitRequest::Replace, &mut callbacks);
        }
        "page" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let page = args.next().and_then(parse_one_based_page).unwrap_or(0);
            open_page(io, path, page);
        }
        "replace" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let text = args.collect::<alloc::vec::Vec<_>>().join(" ");
            let mut editor = trueos_txt::TxtEditor::new(path, text, metrics_for_backend(io));
            let mut callbacks = ShellTxtCallbacks::new(io, path);
            editor.exit(trueos_txt::ExitRequest::Replace, &mut callbacks);
        }
        "rename" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(new_name) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(mut editor) = load_editor(io, path) else {
                return ParseOutcome::Handled;
            };
            let mut callbacks = ShellTxtCallbacks::new(io, path);
            editor.exit(
                trueos_txt::ExitRequest::Rename {
                    new_name: new_name.to_string(),
                },
                &mut callbacks,
            );
        }
        "move" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(new_path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(mut editor) = load_editor(io, path) else {
                return ParseOutcome::Handled;
            };
            let mut callbacks = ShellTxtCallbacks::new(io, path);
            editor.exit(
                trueos_txt::ExitRequest::Move {
                    new_path: new_path.to_string(),
                },
                &mut callbacks,
            );
        }
        "move-page" => {
            let Some(path) = args.next() else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(from) = args.next().and_then(parse_one_based_page) else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(to) = args.next().and_then(parse_one_based_page) else {
                print_usage(io);
                return ParseOutcome::Handled;
            };
            let Some(mut editor) = load_editor(io, path) else {
                return ParseOutcome::Handled;
            };
            let mut callbacks = ShellTxtCallbacks::new(io, path);
            editor.set_page(from, &mut callbacks);
            if editor.move_current_page(to, &mut callbacks) {
                editor.exit(trueos_txt::ExitRequest::Replace, &mut callbacks);
            } else {
                print_shell_line(io, "txt: move-page failed");
            }
        }
        path => {
            let page = args.next().and_then(parse_one_based_page).unwrap_or(0);
            open_page(io, path, page);
        }
    }

    ParseOutcome::Handled
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "txt: usage `txt <path> [page]` | `txt page <path> <page>` | `txt new <path> [text]` | `txt replace <path> <text>` | `txt move-page <path> <from> <to>` | `txt rename <path> <new>` | `txt move <path> <new>`",
    );
}

fn open_page(io: &'static dyn ShellBackend2, path: &str, page: usize) {
    let Some(mut editor) = load_editor(io, path) else {
        return;
    };
    let mut callbacks = ShellTxtCallbacks::new(io, path);
    editor.set_page(page, &mut callbacks);
    render_editor_page(io, &editor);
}

fn load_editor(io: &'static dyn ShellBackend2, path: &str) -> Option<trueos_txt::TxtEditor> {
    let bytes = match crate::r::io::kfs::read_file(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            print_shell_line(io, format!("txt: read failed path={} err={:?}", path, err).as_str());
            return None;
        }
    };
    let text = String::from_utf8_lossy(bytes.as_slice()).into_owned();
    Some(trueos_txt::TxtEditor::new(path, text, metrics_for_backend(io)))
}

fn render_editor_page(io: &'static dyn ShellBackend2, editor: &trueos_txt::TxtEditor) {
    let cursor = editor.cursor();
    let rendered = editor.render_page(cursor.page);
    let width = line_width_for_backend(io).saturating_sub(2).max(24);
    print_shell_line(
        io,
        format!(
            "txt: {} page {}/{} language={} flow={:?}",
            editor.path(),
            rendered.page + 1,
            editor.page_count(),
            editor.language().name(),
            editor.render_flow(),
        )
        .as_str(),
    );

    for (idx, line) in rendered.lines.iter().enumerate() {
        if line.text.is_empty() && idx > last_non_empty_line(&rendered) {
            break;
        }
        let marker = if line.continuation { '+' } else { ' ' };
        let mut out = format!("{:>3}{} ", line.source_row + 1, marker);
        let remaining = width.saturating_sub(out.len());
        push_visible(&mut out, line.text.as_str(), remaining);
        print_native_line(io, out.as_str());
    }
}

fn last_non_empty_line(page: &trueos_txt::RenderPage) -> usize {
    page.lines
        .iter()
        .rposition(|line| !line.text.is_empty())
        .unwrap_or(0)
}

fn push_visible(out: &mut String, text: &str, cap: usize) {
    for ch in text.chars().take(cap) {
        out.push(ch);
    }
}

fn metrics_for_backend(io: &'static dyn ShellBackend2) -> trueos_txt::PageMetrics {
    let cols = line_width_for_backend(io).saturating_sub(8).max(24);
    trueos_txt::PageMetrics::din_a4(cols, 18)
}

fn parse_one_based_page(text: &str) -> Option<usize> {
    text.parse::<usize>()
        .ok()
        .map(|page| page.saturating_sub(1))
}

fn write_file(path: &str, bytes: &[u8]) -> crate::r::io::kfs::Result<()> {
    let handle = crate::r::io::kfs::write_file_begin(path, bytes.len() as u64)?;
    if let Err(err) = crate::r::io::kfs::write_file_chunk(handle, bytes) {
        let _ = crate::r::io::kfs::write_file_abort(handle);
        return Err(err);
    }
    crate::r::io::kfs::write_file_finish(handle)
}
