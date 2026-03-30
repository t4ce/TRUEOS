use alloc::string::String as AllocString;
use core::str::SplitWhitespace;

use spin::Mutex;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

static EMAIL_FROM: Mutex<AllocString> = Mutex::new(AllocString::new());

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "email: usage `email <to> \"<mail text>\"` or `email set <from>`");
}

fn parse_mail_text(args: &mut SplitWhitespace<'_>) -> Option<AllocString> {
    let mut text = AllocString::new();
    for (idx, part) in args.enumerate() {
        if idx != 0 {
            text.push(' ');
        }
        text.push_str(part);
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let mut normalized = AllocString::from(trimmed);
    if normalized.len() >= 2 && normalized.starts_with('"') && normalized.ends_with('"') {
        normalized.remove(0);
        normalized.pop();
    }

    Some(normalized)
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(first) = args.next() else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    if first == "set" {
        let Some(from) = args.next() else {
            print_shell_line(io, "email: usage `email set <from>`");
            return ParseOutcome::Handled;
        };
        if args.next().is_some() {
            print_shell_line(io, "email: usage `email set <from>`");
            return ParseOutcome::Handled;
        }

        let mut stored_from = EMAIL_FROM.lock();
        stored_from.clear();
        stored_from.push_str(from);

        let msg = alloc::format!("email: from set to \"{}\"", from);
        print_shell_line(io, msg.as_str());
        return ParseOutcome::Handled;
    }

    let to = first;
    let Some(mail_text) = parse_mail_text(args) else {
        print_usage(io);
        return ParseOutcome::Handled;
    };

    let from = {
        let stored_from = EMAIL_FROM.lock();
        if stored_from.is_empty() {
            AllocString::from("asd")
        } else {
            stored_from.clone()
        }
    };

    print_shell_line(io, "email-log:");
    let header = alloc::format!("from \"{}\", to \"{}\"", from, to);
    print_shell_line(io, header.as_str());
    let body = alloc::format!("subject \"\", mail text \"{}\"", mail_text);
    print_shell_line(io, body.as_str());
    ParseOutcome::Handled
}
