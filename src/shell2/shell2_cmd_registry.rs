use alloc::string::String as AllocString;
use alloc::vec::Vec;

use embassy_executor::Spawner;

use super::ShellBackend2;
use super::print_shell_line;
use super::shell2_cmd::ParseOutcome;

pub(crate) type Shell2CmdHandler = fn(&Spawner, &'static dyn ShellBackend2, &str) -> ParseOutcome;

#[derive(Clone, Copy)]
struct BuiltinShell2CmdEntry {
    name: &'static str,
    mode: &'static str,
    color: Option<(u8, u8, u8)>,
    handler: Shell2CmdHandler,
}

struct ApiShell2CmdEntry {
    name: AllocString,
    mode: AllocString,
    color: Option<(u8, u8, u8)>,
    handler: Shell2CmdHandler,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterCommandError {
    DuplicateName,
}

static API_CMD_REGISTRY: spin::Mutex<Vec<ApiShell2CmdEntry>> = spin::Mutex::new(Vec::new());

fn dispatch_acpi(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::acpi::try_parse(io, &mut args)
}

fn dispatch_etc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::etc::try_parse(io, &mut args)
}

fn dispatch_format(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::format::try_parse(io, &mut args)
}

fn dispatch_hv(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::hv::try_parse(spawner, io, &mut args)
}

fn dispatch_install(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    if rest.split_whitespace().next().is_some() {
        print_shell_line(io, "install: usage `install`");
        return ParseOutcome::Handled;
    }

    super::cmds::install::submit_install(spawner, io);
    ParseOutcome::Handled
}

fn dispatch_set(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::set::try_parse(io, &mut args)
}

fn dispatch_smp(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::smp::try_parse(io, &mut args)
}

fn dispatch_update(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    if rest.split_whitespace().next().is_some() {
        print_shell_line(io, "update: usage `update`");
        return ParseOutcome::Handled;
    }

    super::cmds::update::submit_update(spawner, io);
    ParseOutcome::Handled
}

fn dispatch_not_wired(
    cmd_name: &'static str,
    _: &Spawner,
    io: &'static dyn ShellBackend2,
    _: &str,
) -> ParseOutcome {
    let msg = alloc::format!("{cmd_name}: not wired in shell2 yet");
    print_shell_line(io, msg.as_str());
    ParseOutcome::Handled
}

fn dispatch_bench(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::bench::try_parse(spawner, io, &mut args)
}

fn dispatch_file(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::file::try_parse(io, &mut args)
}

fn dispatch_net(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let _ = spawner;
    let mut args = rest.split_whitespace();
    super::cmds::net::try_parse(io, &mut args)
}

fn dispatch_run(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::run::try_parse(spawner, io, &mut args)
}

fn dispatch_tlb(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let _ = spawner;
    let mut args = rest.split_whitespace();
    super::cmds::tlb::try_parse(io, &mut args)
}

fn dispatch_tetris(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::tetris::try_parse(io, &mut args)
}

fn dispatch_turbo(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::turbo::try_parse(io, &mut args)
}

fn dispatch_txt(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    dispatch_not_wired("txt", spawner, io, rest)
}

const BUILTIN_CMD_REGISTRY: &[BuiltinShell2CmdEntry] = &[
    BuiltinShell2CmdEntry {
        name: "acpi",
        mode: "cmd",
        color: None,
        handler: dispatch_acpi,
    },
    BuiltinShell2CmdEntry {
        name: "bench",
        mode: "cmd",
        color: None,
        handler: dispatch_bench,
    },
    BuiltinShell2CmdEntry {
        name: "etc",
        mode: "cmd",
        color: None,
        handler: dispatch_etc,
    },
    BuiltinShell2CmdEntry {
        name: "file",
        mode: "cmd",
        color: None,
        handler: dispatch_file,
    },
    BuiltinShell2CmdEntry {
        name: "format",
        mode: "cmd",
        color: None,
        handler: dispatch_format,
    },
    BuiltinShell2CmdEntry {
        name: "hv",
        mode: "cmd",
        color: None,
        handler: dispatch_hv,
    },
    BuiltinShell2CmdEntry {
        name: "install",
        mode: "cmd",
        color: Some((255, 55, 255)),
        handler: dispatch_install,
    },
    BuiltinShell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: None,
        handler: dispatch_net,
    },
    BuiltinShell2CmdEntry {
        name: "run",
        mode: "cmd",
        color: Some((60, 183, 161)),
        handler: dispatch_run,
    },
    BuiltinShell2CmdEntry {
        name: "tlb",
        mode: "cmd",
        color: None,
        handler: dispatch_tlb,
    },
    BuiltinShell2CmdEntry {
        name: "tetris",
        mode: "cmd",
        color: None,
        handler: dispatch_tetris,
    },
    BuiltinShell2CmdEntry {
        name: "turbo",
        mode: "cmd",
        color: None,
        handler: dispatch_turbo,
    },
    BuiltinShell2CmdEntry {
        name: "txt",
        mode: "cmd",
        color: None,
        handler: dispatch_txt,
    },
    BuiltinShell2CmdEntry {
        name: "update",
        mode: "cmd",
        color: Some((255, 55, 255)),
        handler: dispatch_update,
    },
    BuiltinShell2CmdEntry {
        name: "set",
        mode: "cmd",
        color: None,
        handler: dispatch_set,
    },
    BuiltinShell2CmdEntry {
        name: "smp",
        mode: "cmd",
        color: None,
        handler: dispatch_smp,
    },
];

fn starts_with_command<'a>(submitted: &'a str, name: &str) -> Option<&'a str> {
    if submitted.len() < name.len() {
        return None;
    }

    let (head, tail) = submitted.split_at(name.len());
    if !head.eq_ignore_ascii_case(name) {
        return None;
    }
    if tail.is_empty() {
        return Some("");
    }

    match tail.as_bytes()[0] {
        b' ' | b'\t' | b'\r' | b'\n' => Some(tail),
        _ => None,
    }
}

fn name_in_use(name: &str) -> bool {
    if BUILTIN_CMD_REGISTRY
        .iter()
        .any(|entry| entry.name.eq_ignore_ascii_case(name))
    {
        return true;
    }

    API_CMD_REGISTRY
        .lock()
        .iter()
        .any(|entry| entry.name.eq_ignore_ascii_case(name))
}

pub fn register_command(
    name: &str,
    mode: &str,
    color: Option<(u8, u8, u8)>,
    handler: Shell2CmdHandler,
) -> Result<(), RegisterCommandError> {
    if name_in_use(name) {
        return Err(RegisterCommandError::DuplicateName);
    }

    API_CMD_REGISTRY.lock().push(ApiShell2CmdEntry {
        name: AllocString::from(name),
        mode: AllocString::from(mode),
        color,
        handler,
    });

    Ok(())
}

pub(crate) fn try_dispatch(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    submitted: &str,
) -> ParseOutcome {
    for entry in BUILTIN_CMD_REGISTRY {
        if let Some(rest) = starts_with_command(submitted, entry.name) {
            return (entry.handler)(spawner, io, rest);
        }
    }

    let api_registry = API_CMD_REGISTRY.lock();
    for entry in api_registry.iter() {
        if let Some(rest) = starts_with_command(submitted, entry.name.as_str()) {
            return (entry.handler)(spawner, io, rest);
        }
    }

    ParseOutcome::NotCommand
}

pub(crate) fn command_names_status_text() -> AllocString {
    let mut out = AllocString::new();

    for (idx, entry) in BUILTIN_CMD_REGISTRY.iter().enumerate() {
        if idx != 0 {
            out.push(' ');
        }
        if let Some(color) = entry.color {
            let styled = alloc::format!("{}", super::ecma48::style(entry.name).bold().fg(color));
            out.push_str(styled.as_str());
        } else {
            out.push_str(entry.name);
        }
    }

    let api_registry = API_CMD_REGISTRY.lock();
    for entry in api_registry.iter() {
        if !out.is_empty() {
            out.push(' ');
        }
        if let Some(color) = entry.color {
            let styled = alloc::format!(
                "{}",
                super::ecma48::style(entry.name.as_str()).bold().fg(color)
            );
            out.push_str(styled.as_str());
        } else {
            out.push_str(entry.name.as_str());
        }
    }

    out
}

pub(crate) fn command_registry_json() -> AllocString {
    let mut out = AllocString::from("{\"version\":1,\"commands\":[");
    let mut first = true;

    for entry in BUILTIN_CMD_REGISTRY {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"name\":\"");
        out.push_str(entry.name);
        out.push_str("\",\"mode\":\"");
        out.push_str(entry.mode);
        out.push_str("\"}");
    }

    let api_registry = API_CMD_REGISTRY.lock();
    for entry in api_registry.iter() {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"name\":\"");
        out.push_str(entry.name.as_str());
        out.push_str("\",\"mode\":\"");
        out.push_str(entry.mode.as_str());
        out.push_str("\"}");
    }

    out.push_str("]}");
    out
}
