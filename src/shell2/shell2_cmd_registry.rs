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
const TOOL_JSON_DISC: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format"],"description":"disc action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_FSLOG: &str = r#"{"type":"object","properties":{"disk_id":{"type":"string","description":"Optional disk id to scan. Omit for the primary TRUEOSFS root."},"max":{"type":"integer","minimum":1,"maximum":4096,"description":"Maximum raw records to print."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_GPGPU: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","eot","vfe","offscreen","legacy","rowpaint","row2560","walkrow","tilewalker","rowburst","replay","triangle"],"description":"GPGPU action. Omit for visual row2560 scanout painting."},"variant":{"type":"integer","minimum":0,"maximum":10,"description":"Optional EOT catalog variant for `gpgpu eot` or `gpgpu vfe`. Defaults to 6."},"profile":{"type":"integer","minimum":0,"maximum":3,"description":"Optional VFE profile for `gpgpu vfe`: 0 legacy, 1 UOS dw3, 2 UOS dw5, 3 UOS both."},"group_x":{"type":"integer","minimum":0,"description":"Optional result-slot diagnostic group X dimension for offscreen/legacy. Omit or use 0 for the arena offscreen buffer probe."},"row":{"type":"integer","minimum":1,"maximum":1440,"description":"1-based scanout row. Normal shell form is `gpgpu ROW` or `gpgpu rowpaint ROW`."},"rows":{"type":"integer","minimum":1,"maximum":1440,"description":"Optional row count. Defaults to 5 for rowpaint, 1 for row2560, and 16 for tilewalker."},"x":{"type":"integer","minimum":0,"description":"Optional scanout x coordinate. For tilewalker, nonzero x selects the legacy stamp debug path."},"color":{"type":"integer","minimum":0,"maximum":16777215,"description":"Debug-only 24-bit RGB scanout color as 0xRRGGBB."},"stamps":{"oneOf":[{"type":"integer","minimum":1,"maximum":512},{"type":"string","enum":["full","row","all","max"]}],"description":"Optional walkrow/tilewalker stamp count. Supplying this selects the legacy stamp debug path."},"bands":{"oneOf":[{"type":"integer","minimum":1,"maximum":8},{"type":"string","enum":["full","row","all","max"]}],"description":"Optional rowburst band count. Defaults to 1."},"mode":{"type":"string","enum":["strict","loose","repair2","repair4","repair8","repair16","repair32","raw","raw-loose"],"description":"rowburst debug mode. strict/loose use one chunkstamp pass, repair modes add shifted chunkstamp passes, raw modes use the experimental artifact."},"verify":{"type":"boolean","description":"When true, poison and read back each stamp. For tilewalker this selects the legacy stamp debug path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_HYPER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","probe"],"description":"Hyper transport view to print."},"url":{"type":"string","description":"Optional URL to download into TRUEOSFS."},"path":{"type":"string","description":"Optional TRUEOSFS destination path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_LSD: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional TRUEOSFS path to list."},"long":{"type":"boolean","description":"Show file kind and byte size."},"tree":{"type":"boolean","description":"Walk recursively from the path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_MV: &str = r#"{"type":"object","properties":{"src":{"type":"string","description":"Source TRUEOSFS path."},"dst":{"type":"string","description":"Destination TRUEOSFS path."},"regex":{"type":"string","description":"Optional -regx pattern. When set, src and dst are directories."}},"required":["src","dst"],"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_RM: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file or directory path."},"regex":{"type":"string","description":"Optional -regx pattern to match children under path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SET: &str = r#"{"type":"object","properties":{"width":{"type":"integer","minimum":50,"maximum":500,"description":"Shell line width."}},"required":["width"],"additionalProperties":false}"#;
const TOOL_JSON_SHADER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["list","compile","demo","now"],"description":"Shader compiler service action."},"path":{"type":"string","description":"Optional /shader source path for compile or now."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pcibar","mem","cpu","turbo","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;

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

fn dispatch_shader(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::shader::try_parse(io, rest)
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

fn dispatch_gpgpu(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::gpgpu::try_parse(io, rest)
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
        tool_description: Some("List top-level disk devices or format a disk."),
        tool_parameters_json: Some(TOOL_JSON_DISC),
    },
    BuiltinShell2CmdEntry {
        name: "fslog",
        mode: "cmd",
        color: Some((255, 55, 255)),
        advertised: true,
        handler: dispatch_fslog,
        tool_description: Some("Print raw TRUEOSFS log records from the block device."),
        tool_parameters_json: Some(TOOL_JSON_FSLOG),
    },
    BuiltinShell2CmdEntry {
        name: "gpgpu",
        mode: "cmd",
        color: Some((120, 210, 255)),
        advertised: true,
        handler: dispatch_gpgpu,
        tool_description: Some(
            "Inspect GPGPU status or run live scanout rowpaint/tilewalker GPGPU probes.",
        ),
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
        name: "shader",
        mode: "cmd",
        color: Some((255, 190, 90)),
        advertised: true,
        handler: dispatch_shader,
        tool_description: Some("List or queue C4 shader files for the EU32 artifact compiler."),
        tool_parameters_json: Some(TOOL_JSON_SHADER),
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
