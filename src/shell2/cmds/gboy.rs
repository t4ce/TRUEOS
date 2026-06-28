use alloc::string::String;

use super::super::{
    ShellBackend2, matrix_target_for_backend, print_matrix_target_line, print_shell_line,
    set_matrix_target_active, switch_matrix_target_slot,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const GBOY_SLOT: &str = "gb";

fn parse_one_path(rest: &str) -> Result<Option<String>, &'static str> {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut saw = false;
    let mut finished = false;

    for ch in rest.trim().chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            saw = true;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                out.push(ch);
                saw = true;
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            saw = true;
            continue;
        }
        if ch.is_whitespace() {
            if saw {
                finished = true;
            }
            continue;
        }
        if finished {
            return Err("too many arguments");
        }
        out.push(ch);
        saw = true;
    }

    if quote.is_some() {
        return Err("unterminated quote");
    }
    if escaped {
        out.push('\\');
    }
    if !saw || out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let path = match parse_one_path(rest) {
        Ok(Some(path)) => path,
        Ok(None) => {
            print_shell_line(io, "gboy: usage `gboy <rom.gb>`");
            return ParseOutcome::Handled;
        }
        Err(err) => {
            print_shell_line(io, alloc::format!("gboy: {err}").as_str());
            return ParseOutcome::Handled;
        }
    };

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, GBOY_SLOT);
    print_matrix_target_line(&target, alloc::format!("gboy: queueing {}", path).as_str());

    let Some(ap1) = crate::workers::ap1_ui_core_spawner() else {
        print_matrix_target_line(&target, "gboy: AP1 uicore spawner is not registered");
        return ParseOutcome::Handled;
    };

    set_matrix_target_active(&target, true);
    let generation = crate::gb_demo::next_run_generation();
    match crate::gb_demo::gboy_task(path, target.clone(), generation) {
        Ok(token) => {
            ap1.spawn_and_wake_remote(token);
            print_matrix_target_line(&target, "gboy: queued");
        }
        Err(err) => {
            set_matrix_target_active(&target, false);
            print_matrix_target_line(
                &target,
                alloc::format!("gboy: spawn failed {err:?}").as_str(),
            );
        }
    }

    ParseOutcome::Handled
}
