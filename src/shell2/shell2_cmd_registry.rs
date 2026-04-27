use alloc::string::String as AllocString;
use alloc::vec::Vec;

use embassy_executor::Spawner;

use super::ShellBackend2;
use super::print_shell_line;
use super::shell2_cmd::ParseOutcome;

pub(crate) type Shell2CmdHandler = fn(&Spawner, &'static dyn ShellBackend2, &str) -> ParseOutcome;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RegisterCommandError {
    DuplicateName,
}

#[derive(Clone, Copy)]
struct BuiltinShell2CmdEntry {
    name: &'static str,
    mode: &'static str,
    color: Option<(u8, u8, u8)>,
    handler: Shell2CmdHandler,
    tool_description: Option<&'static str>,
    tool_parameters_json: Option<&'static str>,
}

struct ApiShell2CmdEntry {
    name: AllocString,
    mode: AllocString,
    color: Option<(u8, u8, u8)>,
    handler: Shell2CmdHandler,
}

static API_CMD_REGISTRY: spin::Mutex<Vec<ApiShell2CmdEntry>> = spin::Mutex::new(Vec::new());

const TOOL_JSON_ACPI: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["reboot","S1","S2","S3","S4","S5"],"description":"ACPI action to run."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_C4: &str = r#"{"type":"object","properties":{"mode":{"type":"string","enum":["file","inline"],"description":"Compile from a TRUEOSFS file or inline C4 source."},"path":{"type":"string","description":"TRUEOSFS source path when mode=file."},"source":{"type":"string","description":"Inline C4 source when mode=inline."}},"required":["mode"],"additionalProperties":false}"#;
const TOOL_JSON_EMAIL: &str = r#"{"type":"object","properties":{"mode":{"type":"string","enum":["send","set_from"],"description":"Choose whether to send a mail log entry or set the default from address."},"to":{"type":"string","description":"Recipient address when mode=send."},"mail_text":{"type":"string","description":"Mail body text when mode=send."},"from":{"type":"string","description":"Sender address when mode=set_from."}},"required":["mode"],"additionalProperties":false}"#;
const TOOL_JSON_ETC: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["ample","go","go2","insane"],"description":"etc subcommand to run."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_FILE: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format","ramdisc"],"description":"file action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format."},"size":{"type":"string","description":"Optional ramdisc size like 512MB or 1GiB for action=ramdisc."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_LC: &str = r#"{"type":"object","properties":{"prompt":{"type":"string","description":"Optional prompt text to pass into the dedicated localcoder task."},"continue":{"type":"boolean","description":"Continue the latest localcoder session."},"new":{"type":"boolean","description":"Start a fresh localcoder session."},"resume":{"type":"string","description":"Resume the specified localcoder session id."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_PROBE: &str = r#"{"type":"object","properties":{"domain":{"type":"string","enum":["usb","nvme"],"description":"Probe domain."},"action":{"type":"string","enum":["status","snapshot","kick","rebind","recover","fix","mysterybox","probe","flr"],"description":"Action inside the selected domain."},"controller":{"type":"integer","minimum":0,"description":"Optional controller index for usb snapshot/kick/rebind/recover/fix/mysterybox."},"pci":{"type":"string","description":"PCI BDF like 00:1f.0 for nvme flr."}},"required":["domain","action"],"additionalProperties":false}"#;
const TOOL_JSON_SET: &str = r#"{"type":"object","properties":{"width":{"type":"integer","minimum":50,"maximum":500,"description":"Shell line width."}},"required":["width"],"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pciids","pcibar","mem","cpu","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;
const TOOL_JSON_TURBO: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["status","arm","disarm","on","off","verify"],"description":"turbo action to run."},"spins":{"type":"integer","minimum":0,"description":"Optional spin count for action=verify."}},"required":["action"],"additionalProperties":false}"#;

fn dispatch_acpi(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::acpi::try_parse(io, &mut args)
}

fn dispatch_etc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::etc::try_parse(io, &mut args)
}

fn dispatch_email(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::email::try_parse(io, &mut args)
}

fn dispatch_hv(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::hv::try_parse(spawner, io, &mut args)
}

fn dispatch_install(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::install::try_parse(spawner, io, &mut args)
}

fn dispatch_lc(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::lc::try_parse(spawner, io, &mut args)
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
    let mut args = rest.split_whitespace();
    super::cmds::update::try_parse(spawner, io, &mut args)
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

fn dispatch_c4(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::c4::try_parse(io, rest)
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
        tool_description: Some("Run ACPI power actions."),
        tool_parameters_json: Some(TOOL_JSON_ACPI),
    },
    BuiltinShell2CmdEntry {
        name: "bench",
        mode: "cmd",
        color: None,
        handler: dispatch_bench,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "c4",
        mode: "cmd",
        color: Some((255, 190, 90)),
        handler: dispatch_c4,
        tool_description: Some("Compile C4 source to Rust and TC4O, then run the TC4O VM object."),
        tool_parameters_json: Some(TOOL_JSON_C4),
    },
    BuiltinShell2CmdEntry {
        name: "etc",
        mode: "cmd",
        color: None,
        handler: dispatch_etc,
        tool_description: Some("Run small shell demo and utility subcommands."),
        tool_parameters_json: Some(TOOL_JSON_ETC),
    },
    BuiltinShell2CmdEntry {
        name: "email",
        mode: "cmd",
        color: None,
        handler: dispatch_email,
        tool_description: Some("Send a shell email log entry or set the default from address."),
        tool_parameters_json: Some(TOOL_JSON_EMAIL),
    },
    BuiltinShell2CmdEntry {
        name: "file",
        mode: "cmd",
        color: None,
        handler: dispatch_file,
        tool_description: Some("List mounted TRUEOSFS roots, format a disk, or create a ramdisc."),
        tool_parameters_json: Some(TOOL_JSON_FILE),
    },
    BuiltinShell2CmdEntry {
        name: "hv",
        mode: "cmd",
        color: None,
        handler: dispatch_hv,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "install",
        mode: "cmd",
        color: Some((255, 55, 255)),
        handler: dispatch_install,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "lc",
        mode: "cmd",
        color: Some((80, 200, 120)),
        handler: dispatch_lc,
        tool_description: Some("Start the dedicated shell2 localcoder worker task."),
        tool_parameters_json: Some(TOOL_JSON_LC),
    },
    BuiltinShell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: None,
        handler: dispatch_net,
        tool_description: Some(
            "Inspect network state, run ICMP, use IRC, or get/set the hostname.",
        ),
        tool_parameters_json: Some(TOOL_JSON_NET),
    },
    BuiltinShell2CmdEntry {
        name: "tlb",
        mode: "cmd",
        color: None,
        handler: dispatch_tlb,
        tool_description: Some("Print one of the table and hardware inspection views."),
        tool_parameters_json: Some(TOOL_JSON_TLB),
    },
    BuiltinShell2CmdEntry {
        name: "tetris",
        mode: "cmd",
        color: None,
        handler: dispatch_tetris,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "turbo",
        mode: "cmd",
        color: None,
        handler: dispatch_turbo,
        tool_description: Some("Inspect or change CPU turbo state."),
        tool_parameters_json: Some(TOOL_JSON_TURBO),
    },
    BuiltinShell2CmdEntry {
        name: "txt",
        mode: "cmd",
        color: None,
        handler: dispatch_txt,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "update",
        mode: "cmd",
        color: Some((255, 55, 255)),
        handler: dispatch_update,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "set",
        mode: "cmd",
        color: None,
        handler: dispatch_set,
        tool_description: Some("Set the shell line width."),
        tool_parameters_json: Some(TOOL_JSON_SET),
    },
    BuiltinShell2CmdEntry {
        name: "smp",
        mode: "cmd",
        color: None,
        handler: dispatch_smp,
        tool_description: Some("Inspect SMP slot state."),
        tool_parameters_json: Some(TOOL_JSON_SMP),
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
            let styled =
                alloc::format!("{}", super::term_style::paint(entry.name).bold().color(color));
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
                super::term_style::paint(entry.name.as_str())
                    .bold()
                    .color(color)
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
        out.push('"');
        if let (Some(description), Some(parameters_json)) =
            (entry.tool_description, entry.tool_parameters_json)
        {
            out.push_str(",\"tool\":{\"description\":\"");
            out.push_str(description);
            out.push_str("\",\"parameters\":");
            out.push_str(parameters_json);
            out.push('}');
        }
        out.push('}');
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
