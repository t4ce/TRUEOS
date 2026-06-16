use alloc::string::String as AllocString;

use embassy_executor::Spawner;

use super::ShellBackend2;
use super::shell2_cmd::ParseOutcome;

pub(crate) type Shell2CmdHandler = fn(&Spawner, &'static dyn ShellBackend2, &str) -> ParseOutcome;

#[derive(Clone, Copy)]
struct BuiltinShell2CmdEntry {
    name: &'static str,
    mode: &'static str,
    color: Option<(u8, u8, u8)>,
    advertised: bool,
    handler: Shell2CmdHandler,
    tool_description: Option<&'static str>,
    tool_parameters_json: Option<&'static str>,
}

const TOOL_JSON_ACPI: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["reboot","S1","S2","S3","S4","S5"],"description":"ACPI action to run."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_7Z: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file to compress into a sibling .7z archive."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_C4: &str = r#"{"type":"object","properties":{"mode":{"type":"string","enum":["file","inline"],"description":"Compile from a TRUEOSFS file or inline C4 source."},"path":{"type":"string","description":"TRUEOSFS source path when mode=file."},"source":{"type":"string","description":"Inline C4 source when mode=inline."}},"required":["mode"],"additionalProperties":false}"#;
const TOOL_JSON_DISC: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format","log"],"description":"disc action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format or optional disk id for action=log."},"max":{"type":"integer","minimum":1,"maximum":4096,"description":"Maximum raw TRUEOSFS log records to print for action=log."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_GPGPU: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","clear","copy","scanout","atlas","athlas","athlas_go","mandel","canvas","smoke"],"description":"GPGPU command to run."},"id":{"type":"integer","description":"Optional atlas sprite slot id."},"x":{"type":"integer","description":"Optional clear x or atlas destination x."},"y":{"type":"integer","description":"Optional clear y or atlas destination y."},"w":{"type":"integer","description":"Optional width."},"h":{"type":"integer","description":"Optional height."},"sx":{"type":"integer","description":"Optional copy source x."},"sy":{"type":"integer","description":"Optional copy source y."},"dx":{"type":"integer","description":"Optional copy destination x."},"dy":{"type":"integer","description":"Optional copy destination y."},"duration_ms":{"type":"integer","description":"Optional athlas_go/canvas runtime in milliseconds."},"cadence_ms":{"type":"integer","description":"Optional athlas_go/canvas minimum launch cadence in milliseconds."},"burst":{"type":"integer","description":"Optional athlas_go copies per cadence step."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_HYPER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","probe"],"description":"Hyper transport view to print."},"url":{"type":"string","description":"Optional URL to download into TRUEOSFS."},"path":{"type":"string","description":"Optional TRUEOSFS destination path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_LSD: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional TRUEOSFS path to list."},"paths":{"type":"array","items":{"type":"string"},"description":"Optional TRUEOSFS paths to list."},"long":{"type":"boolean","description":"Show file kind, ownership, byte size, and name."},"tree":{"type":"boolean","description":"Walk recursively from the path."},"table":{"type":"boolean","description":"Render the shell2 table view."},"oneline":{"type":"boolean","description":"Show one entry per line."},"directory_only":{"type":"boolean","description":"List directories themselves instead of their contents."},"color":{"type":"string","enum":["always","auto","never"],"description":"Color output mode."},"size":{"type":"string","enum":["default","short","bytes"],"description":"Size display mode."},"permission":{"type":"string","enum":["rwx","octal","attributes","disable"],"description":"Permission display mode."},"sort":{"type":"string","enum":["name","size","extension","none"],"description":"Sort entries."},"reverse":{"type":"boolean","description":"Reverse the selected sort."},"group_dirs":{"type":"string","enum":["none","first","last"],"description":"Group directories before or after files."},"depth":{"type":"integer","minimum":0,"description":"Maximum recursive depth."},"header":{"type":"boolean","description":"Show long-output headers."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_MV: &str = r#"{"type":"object","properties":{"src":{"type":"string","description":"Source TRUEOSFS path."},"dst":{"type":"string","description":"Destination TRUEOSFS path."},"regex":{"type":"string","description":"Optional -regx pattern. When set, src and dst are directories."}},"required":["src","dst"],"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_RM: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file or directory path."},"regex":{"type":"string","description":"Optional -regx pattern to match children under path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SET: &str = r#"{"type":"object","properties":{"width":{"type":"integer","minimum":50,"maximum":500,"description":"Shell line width."}},"required":["width"],"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pcibar","mem","cpu","turbo","ucode","pmu","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;

fn dispatch_acpi(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::acpi::try_parse(io, &mut args)
}

fn dispatch_7z(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::sevenz::try_parse(io, rest)
}

fn dispatch_install(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::install::try_parse(spawner, io, &mut args)
}

fn dispatch_hyper(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::hyper::try_parse(spawner, io, &mut args)
}

fn dispatch_lsd(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::lsd::try_parse(io, rest)
}

fn dispatch_mv(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::mv::try_parse(io, "mv", rest)
}

fn dispatch_move(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::mv::try_parse(io, "move", rest)
}

fn dispatch_rm(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(io, "rm", rest)
}

fn dispatch_remove(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(io, "remove", rest)
}

fn dispatch_delete(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(io, "delete", rest)
}

fn dispatch_del(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(io, "del", rest)
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

fn dispatch_bench(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::bench::try_parse(spawner, io, &mut args)
}

fn dispatch_c4(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::c4::try_parse(io, rest)
}

fn dispatch_disc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::disc::try_parse(io, &mut args)
}

fn dispatch_fslog(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::fslog::try_parse(io, rest)
}

fn dispatch_gpgpu(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::gpgpu::try_parse(spawner, io, &mut args)
}

fn dispatch_net(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let _ = spawner;
    let mut args = rest.split_whitespace();
    super::cmds::net::try_parse(io, &mut args)
}

fn dispatch_tlb(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::tlb::try_parse(spawner, io, &mut args)
}

fn dispatch_txt(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let _ = spawner;
    super::cmds::txt::try_parse(io, rest)
}

const BUILTIN_CMD_REGISTRY: &[BuiltinShell2CmdEntry] = &[
    BuiltinShell2CmdEntry {
        name: "7z",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: true,
        handler: dispatch_7z,
        tool_description: Some("Queue a kernel codec job that compresses a TRUEOSFS file as .7z."),
        tool_parameters_json: Some(TOOL_JSON_7Z),
    },
    BuiltinShell2CmdEntry {
        name: "acpi",
        mode: "cmd",
        color: None,
        advertised: true,
        handler: dispatch_acpi,
        tool_description: Some("Run ACPI power actions."),
        tool_parameters_json: Some(TOOL_JSON_ACPI),
    },
    BuiltinShell2CmdEntry {
        name: "bench",
        mode: "cmd",
        color: None,
        advertised: true,
        handler: dispatch_bench,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "c4",
        mode: "cmd",
        color: Some((255, 190, 90)),
        advertised: true,
        handler: dispatch_c4,
        tool_description: Some("Compile C4 source to Rust and TC4O, then run the TC4O VM object."),
        tool_parameters_json: Some(TOOL_JSON_C4),
    },
    BuiltinShell2CmdEntry {
        name: "disc",
        mode: "cmd",
        color: Some((255, 55, 255)),
        advertised: true,
        handler: dispatch_disc,
        tool_description: Some(
            "List top-level disk devices, format a disk, or print raw TRUEOSFS log records.",
        ),
        tool_parameters_json: Some(TOOL_JSON_DISC),
    },
    BuiltinShell2CmdEntry {
        name: "fslog",
        mode: "cmd",
        color: Some((255, 55, 255)),
        advertised: false,
        handler: dispatch_fslog,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "gpgpu",
        mode: "cmd",
        color: Some((120, 210, 255)),
        advertised: true,
        handler: dispatch_gpgpu,
        tool_description: Some("Run Intel GPGPU clear/copy staging-surface commands."),
        tool_parameters_json: Some(TOOL_JSON_GPGPU),
    },
    BuiltinShell2CmdEntry {
        name: "install",
        mode: "cmd",
        color: Some((255, 55, 255)),
        advertised: true,
        handler: dispatch_install,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "hyper",
        mode: "cmd",
        color: Some((120, 210, 255)),
        advertised: true,
        handler: dispatch_hyper,
        tool_description: Some("Inspect the kernel Hyper HTTP/HTTPS transport surface."),
        tool_parameters_json: Some(TOOL_JSON_HYPER),
    },
    BuiltinShell2CmdEntry {
        name: "lsd",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: true,
        handler: dispatch_lsd,
        tool_description: Some("List TRUEOSFS paths with the TRUEOS lsd adapter."),
        tool_parameters_json: Some(TOOL_JSON_LSD),
    },
    BuiltinShell2CmdEntry {
        name: "rm",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: true,
        handler: dispatch_rm,
        tool_description: Some("Remove a TRUEOSFS file or directory after confirmation."),
        tool_parameters_json: Some(TOOL_JSON_RM),
    },
    BuiltinShell2CmdEntry {
        name: "remove",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: false,
        handler: dispatch_remove,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "delete",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: false,
        handler: dispatch_delete,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "del",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: false,
        handler: dispatch_del,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "mv",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: true,
        handler: dispatch_mv,
        tool_description: Some("Move TRUEOSFS files or directory contents."),
        tool_parameters_json: Some(TOOL_JSON_MV),
    },
    BuiltinShell2CmdEntry {
        name: "move",
        mode: "cmd",
        color: Some((60, 220, 120)),
        advertised: false,
        handler: dispatch_move,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: Some((120, 210, 255)),
        advertised: true,
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
        advertised: true,
        handler: dispatch_tlb,
        tool_description: Some("Print one of the table and hardware inspection views."),
        tool_parameters_json: Some(TOOL_JSON_TLB),
    },
    BuiltinShell2CmdEntry {
        name: "txt",
        mode: "cmd",
        color: None,
        advertised: true,
        handler: dispatch_txt,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "update",
        mode: "cmd",
        color: Some((255, 55, 255)),
        advertised: true,
        handler: dispatch_update,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "set",
        mode: "cmd",
        color: None,
        advertised: true,
        handler: dispatch_set,
        tool_description: Some("Set the shell line width."),
        tool_parameters_json: Some(TOOL_JSON_SET),
    },
    BuiltinShell2CmdEntry {
        name: "smp",
        mode: "cmd",
        color: None,
        advertised: true,
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

    ParseOutcome::NotCommand
}

pub(crate) fn command_names_status_text() -> AllocString {
    let mut out = AllocString::new();

    let mut first = true;
    for entry in BUILTIN_CMD_REGISTRY.iter().filter(|entry| entry.advertised) {
        if !first {
            out.push(' ');
        }
        first = false;
        if let Some(color) = entry.color {
            let styled =
                alloc::format!("{}", super::term_style::paint(entry.name).bold().color(color));
            out.push_str(styled.as_str());
        } else {
            out.push_str(entry.name);
        }
    }

    out
}

pub(crate) fn command_registry_json() -> AllocString {
    let mut out = AllocString::from("{\"version\":1,\"commands\":[");
    let mut first = true;

    for entry in BUILTIN_CMD_REGISTRY {
        if !entry.advertised {
            continue;
        }
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

    out.push_str("]}");
    out
}
