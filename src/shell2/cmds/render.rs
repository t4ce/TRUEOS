use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "render joker <variant>");
    print_shell_line(io, "render joker list");
    print_shell_line(io, "render oa <action>");
    print_shell_line(io, "render oa list");
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn list_joker_variants(io: &'static dyn ShellBackend2) {
    let mut line = alloc::string::String::from("render joker variants:");
    for name in crate::intel::render::render_joker_variant_names() {
        line.push(' ');
        line.push_str(name);
    }
    print_shell_line(io, line.as_str());
}

fn list_oa_actions(io: &'static dyn ShellBackend2) {
    let mut line = alloc::string::String::from("render oa actions:");
    for name in crate::intel::render::render_oa_control_action_names() {
        line.push(' ');
        line.push_str(name);
    }
    print_shell_line(io, line.as_str());
}

fn run_oa(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(action) = args.next() else {
        usage(io);
        return;
    };
    if action.eq_ignore_ascii_case("list") {
        if expect_no_more(io, args) {
            list_oa_actions(io);
        }
        return;
    }
    if !expect_no_more(io, args) {
        return;
    }

    match crate::intel::render::render_oa_control_action(action) {
        Ok(result) => {
            let msg = alloc::format!(
                "render oa: action={} oactx=0x{:08X} oar=0x{:08X} ctx_ctrl=0x{:08X}",
                result.action,
                result.oactx,
                result.oar,
                result.ctx_ctrl,
            );
            print_shell_line(io, msg.as_str());
        }
        Err("unknown-action") => {
            print_shell_line(io, "render oa: unknown action");
            list_oa_actions(io);
        }
        Err(reason) => {
            let msg = alloc::format!("render oa: skipped reason={}", reason);
            print_shell_line(io, msg.as_str());
        }
    }
}

fn run_joker(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(variant) = args.next() else {
        usage(io);
        return;
    };
    if variant.eq_ignore_ascii_case("list") {
        if expect_no_more(io, args) {
            list_joker_variants(io);
        }
        return;
    }
    if !expect_no_more(io, args) {
        return;
    }

    match crate::intel::render::submit_render_joker_probe(variant) {
        Ok(result) => {
            let msg = alloc::format!(
                "render joker: variant={} submit={} target={} completed={} log=intel/render",
                result.variant,
                result.submit_name,
                result.target,
                result.completed as u8,
            );
            print_shell_line(io, msg.as_str());
        }
        Err("unknown-variant") => {
            print_shell_line(io, "render joker: unknown variant");
            list_joker_variants(io);
        }
        Err(reason) => {
            let msg = alloc::format!("render joker: skipped reason={}", reason);
            print_shell_line(io, msg.as_str());
        }
    }
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("joker") {
        run_joker(io, args);
    } else if cmd.eq_ignore_ascii_case("oa") {
        run_oa(io, args);
    } else if cmd.eq_ignore_ascii_case("list") {
        if expect_no_more(io, args) {
            list_joker_variants(io);
        }
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
