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

const STATUS_GREEN_RGB: (u8, u8, u8) = (60, 220, 120);
const STATUS_PINK_RGB: (u8, u8, u8) = (255, 55, 255);
const STATUS_BLUE_RGB: (u8, u8, u8) = (120, 210, 255);
const STATUS_ORANGE_RGB: (u8, u8, u8) = (255, 190, 90);
const STATUS_GRAY_RGB: (u8, u8, u8) = (160, 168, 176);
const STATUS_RAINBOW_COLORS: [u8; 8] = [199, 208, 227, 121, 51, 39, 99, 201];

const TOOL_JSON_ACPI: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["reboot","S1","S2","S3","S4","S5"],"description":"ACPI action to run."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_AUD: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_7Z: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS path. Non-.7z files compress to a sibling .7z archive; .7z archives extract beside the archive."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_C4: &str = r#"{"type":"object","properties":{"mode":{"type":"string","enum":["file","inline"],"description":"Compile from a TRUEOSFS file or inline C4 source."},"path":{"type":"string","description":"TRUEOSFS source path when mode=file."},"source":{"type":"string","description":"Inline C4 source when mode=inline."}},"required":["mode"],"additionalProperties":false}"#;
const TOOL_JSON_DIASHOW: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_DISC: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format","ramdisc","log"],"description":"disc action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format or optional disk id for action=log."},"size":{"type":"string","description":"Optional ramdisc size like 512MB or 1GiB for action=ramdisc."},"max":{"type":"integer","minimum":1,"maximum":4096,"description":"Maximum raw TRUEOSFS log records to print for action=log."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_ETC: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["diashow","gboy"],"description":"etc subcommand to run."},"path":{"type":"string","description":"TRUEOSFS Game Boy ROM path for subcommand=gboy."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_GBOY: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS Game Boy ROM path."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_GPGPU: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["canvas2d","canvas3d","artificial-fragment","smoke"],"description":"GPGPU command to run."},"canvas2d":{"type":"string","enum":["sprite","sprites64","mandel64"],"description":"Optional canvas2d mode."},"canvas3d":{"type":"string","enum":["cube","ico","para"],"description":"Optional canvas3d mode."},"duration_ms":{"type":"integer","description":"Optional canvas2d sprite runtime in milliseconds."},"cadence_ms":{"type":"integer","description":"Optional canvas2d sprite minimum launch cadence in milliseconds."},"count":{"type":"integer","minimum":1,"maximum":256,"description":"Optional canvas2d sprite descriptors per batch."},"present_every":{"type":"integer","minimum":1,"maximum":1024,"description":"Optional canvas2d sprite present interval."},"iterations":{"type":"integer","description":"Optional canvas2d mandel64 iteration count."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_HYPER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","probe"],"description":"Hyper transport view to print."},"url":{"type":"string","description":"Optional URL to download into TRUEOSFS."},"path":{"type":"string","description":"Optional TRUEOSFS destination path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_LSD: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional TRUEOSFS path to list."},"paths":{"type":"array","items":{"type":"string"},"description":"Optional TRUEOSFS paths to list."},"long":{"type":"boolean","description":"Show file kind, ownership, byte size, and name."},"tree":{"type":"boolean","description":"Walk recursively from the path."},"table":{"type":"boolean","description":"Render the shell2 table view."},"archive7z":{"type":"boolean","description":"Inspect a .7z archive and print its entries without extracting."},"oneline":{"type":"boolean","description":"Show one entry per line."},"directory_only":{"type":"boolean","description":"List directories themselves instead of their contents."},"color":{"type":"string","enum":["always","auto","never"],"description":"Color output mode."},"size":{"type":"string","enum":["default","short","bytes"],"description":"Size display mode."},"permission":{"type":"string","enum":["rwx","octal","attributes","disable"],"description":"Permission display mode."},"sort":{"type":"string","enum":["name","size","extension","none"],"description":"Sort entries."},"reverse":{"type":"boolean","description":"Reverse the selected sort."},"group_dirs":{"type":"string","enum":["none","first","last"],"description":"Group directories before or after files."},"depth":{"type":"integer","minimum":0,"description":"Maximum recursive depth."},"header":{"type":"boolean","description":"Show long-output headers."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_MV: &str = r#"{"type":"object","properties":{"src":{"type":"string","description":"Source TRUEOSFS path."},"dst":{"type":"string","description":"Destination TRUEOSFS path."},"regex":{"type":"string","description":"Optional -regx pattern. When set, src and dst are directories."}},"required":["src","dst"],"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_RENDER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["joker","oa","list"],"description":"Render probe command to run."},"variant":{"type":"string","description":"Optional joker variant, for example mesa, bt0, oa, slot0, payload-bary, or grf2."},"action":{"type":"string","description":"Optional OA action, for example status, ctx-on, ctx-off, oar-on, oar-off, full-on, or full-off."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_RM: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file or directory path."},"regex":{"type":"string","description":"Optional -regx pattern to match children under path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SET: &str = r#"{"type":"object","properties":{"width":{"type":"integer","minimum":50,"maximum":500,"description":"Shell line width."}},"required":["width"],"additionalProperties":false}"#;
const TOOL_JSON_SHA: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file to hash with SHA-256."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pcibar","mem","cpu","turbo","ucode","pmu","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;

fn dispatch_acpi(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::acpi::try_parse(io, &mut args)
}

fn dispatch_aud(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::aud::try_parse(spawner, io, rest)
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

fn dispatch_sha(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::sha::try_parse(io, rest)
}

fn dispatch_smp(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::smp::try_parse(io, &mut args)
}

fn dispatch_update(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::update::try_parse(spawner, io, &mut args)
}

fn dispatch_c4(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::c4::try_parse(io, rest)
}

fn dispatch_diashow(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::diashow::try_parse(io, rest)
}

fn dispatch_etc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let trimmed = rest.trim_start();
    if trimmed.is_empty() {
        super::print_shell_line(io, "etc: usage `etc diashow` | `etc gboy <rom.gb>`");
        return ParseOutcome::Handled;
    }

    let command_end = trimmed
        .char_indices()
        .find_map(|(idx, ch)| ch.is_whitespace().then_some(idx))
        .unwrap_or(trimmed.len());
    let command = &trimmed[..command_end];
    let tail = trimmed[command_end..].trim_start();

    if command.eq_ignore_ascii_case("diashow") {
        super::cmds::diashow::try_parse(io, tail)
    } else if command.eq_ignore_ascii_case("gboy") {
        super::cmds::gboy::try_parse(io, tail)
    } else if command.eq_ignore_ascii_case("help") {
        super::print_shell_line(io, "etc: commands `diashow`, `gboy <rom.gb>`");
        ParseOutcome::Handled
    } else {
        super::print_shell_line(io, alloc::format!("etc: unknown subcommand `{command}`").as_str());
        ParseOutcome::Handled
    }
}

fn dispatch_disc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::disc::try_parse(io, &mut args)
}

fn dispatch_fslog(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::fslog::try_parse(io, rest)
}

fn dispatch_gboy(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::gboy::try_parse(io, rest)
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

fn dispatch_render(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::render::try_parse(io, &mut args)
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
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_7z,
        tool_description: Some(
            "Queue a kernel codec job that compresses a TRUEOSFS file or extracts a .7z archive.",
        ),
        tool_parameters_json: Some(TOOL_JSON_7Z),
    },
    BuiltinShell2CmdEntry {
        name: "acpi",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_acpi,
        tool_description: Some("Run ACPI power actions."),
        tool_parameters_json: Some(TOOL_JSON_ACPI),
    },
    BuiltinShell2CmdEntry {
        name: "aud",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_aud,
        tool_description: Some("Queue /aud.m4a from TRUEOSFS root through the AP1 audio service."),
        tool_parameters_json: Some(TOOL_JSON_AUD),
    },
    BuiltinShell2CmdEntry {
        name: "c4",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_c4,
        tool_description: Some("Compile C4 source to Rust and TC4O, then run the TC4O VM object."),
        tool_parameters_json: Some(TOOL_JSON_C4),
    },
    BuiltinShell2CmdEntry {
        name: "disc",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_disc,
        tool_description: Some(
            "List top-level disk devices, format a disk, create a ramdisc, or print raw TRUEOSFS log records.",
        ),
        tool_parameters_json: Some(TOOL_JSON_DISC),
    },
    BuiltinShell2CmdEntry {
        name: "diashow",
        mode: "cmd",
        color: Some(STATUS_PINK_RGB),
        advertised: false,
        handler: dispatch_diashow,
        tool_description: Some(
            "Decode up to 200 /diashow/*.jpeg files with the kernel zune JPEG path and present them centered on the primary scanout from AP1.",
        ),
        tool_parameters_json: Some(TOOL_JSON_DIASHOW),
    },
    BuiltinShell2CmdEntry {
        name: "etc",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_etc,
        tool_description: Some("Run miscellaneous commands such as diashow and gboy."),
        tool_parameters_json: Some(TOOL_JSON_ETC),
    },
    BuiltinShell2CmdEntry {
        name: "fslog",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_fslog,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "gboy",
        mode: "cmd",
        color: Some(STATUS_PINK_RGB),
        advertised: false,
        handler: dispatch_gboy,
        tool_description: Some(
            "Load a Game Boy ROM from TRUEOSFS and present it on the Intel backend.",
        ),
        tool_parameters_json: Some(TOOL_JSON_GBOY),
    },
    BuiltinShell2CmdEntry {
        name: "gpgpu",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_gpgpu,
        tool_description: Some("Run Intel GPGPU clear/copy staging-surface commands."),
        tool_parameters_json: Some(TOOL_JSON_GPGPU),
    },
    BuiltinShell2CmdEntry {
        name: "install",
        mode: "cmd",
        color: Some(STATUS_PINK_RGB),
        advertised: true,
        handler: dispatch_install,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "hyper",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_hyper,
        tool_description: Some("Inspect the kernel Hyper HTTP/HTTPS transport surface."),
        tool_parameters_json: Some(TOOL_JSON_HYPER),
    },
    BuiltinShell2CmdEntry {
        name: "lsd",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_lsd,
        tool_description: Some("List TRUEOSFS paths with the TRUEOS lsd adapter."),
        tool_parameters_json: Some(TOOL_JSON_LSD),
    },
    BuiltinShell2CmdEntry {
        name: "rm",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_rm,
        tool_description: Some("Remove a TRUEOSFS file or directory after confirmation."),
        tool_parameters_json: Some(TOOL_JSON_RM),
    },
    BuiltinShell2CmdEntry {
        name: "sha",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_sha,
        tool_description: Some("Hash a TRUEOSFS file with SHA-256."),
        tool_parameters_json: Some(TOOL_JSON_SHA),
    },
    BuiltinShell2CmdEntry {
        name: "render",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_render,
        tool_description: Some("Run Intel render bring-up joker probes."),
        tool_parameters_json: Some(TOOL_JSON_RENDER),
    },
    BuiltinShell2CmdEntry {
        name: "remove",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_remove,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "delete",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_delete,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "del",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_del,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "mv",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_mv,
        tool_description: Some("Move TRUEOSFS files or directory contents."),
        tool_parameters_json: Some(TOOL_JSON_MV),
    },
    BuiltinShell2CmdEntry {
        name: "move",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_move,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
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
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_tlb,
        tool_description: Some("Print one of the table and hardware inspection views."),
        tool_parameters_json: Some(TOOL_JSON_TLB),
    },
    BuiltinShell2CmdEntry {
        name: "txt",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_txt,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "update",
        mode: "cmd",
        color: Some(STATUS_PINK_RGB),
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
        color: Some(STATUS_GRAY_RGB),
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
    const STATUS_ORDER: &[&str] = &[
        "7z", "lsd", "rm", "mv", "sha", "disc", "install", "update", "hyper", "net", "c4", "txt",
        "gpgpu", "aud", "acpi", "tlb", "smp", "etc",
    ];

    let mut out = AllocString::new();

    let mut first = true;
    for name in STATUS_ORDER {
        let Some(entry) = BUILTIN_CMD_REGISTRY
            .iter()
            .find(|entry| entry.advertised && entry.name == *name)
        else {
            continue;
        };

        if !first {
            out.push(' ');
        }
        first = false;
        push_status_command_name(&mut out, entry);
    }

    out
}

fn push_status_command_name(out: &mut AllocString, entry: &BuiltinShell2CmdEntry) {
    let label = status_command_label(entry);

    if matches!(entry.name, "gpgpu" | "aud") {
        push_static_rainbow_token(out, label);
    } else if let Some(color) = entry.color {
        let styled = alloc::format!("{}", super::term_style::paint(label).bold().color(color));
        out.push_str(styled.as_str());
    } else {
        out.push_str(label);
    }
}

fn status_command_label(entry: &BuiltinShell2CmdEntry) -> &'static str {
    match entry.name {
        "aud" => "audio",
        _ => entry.name,
    }
}

fn push_static_rainbow_token(out: &mut AllocString, text: &str) {
    for (idx, ch) in text.chars().enumerate() {
        let mut glyph = [0u8; 4];
        let glyph = ch.encode_utf8(&mut glyph);
        let color = STATUS_RAINBOW_COLORS[idx % STATUS_RAINBOW_COLORS.len()];
        let styled = if (idx & 1) == 0 {
            alloc::format!(
                "{}",
                super::term_style::paint(glyph)
                    .bold()
                    .underline()
                    .color(color)
            )
        } else {
            alloc::format!("{}", super::term_style::paint(glyph).bold().color(color))
        };
        out.push_str(styled.as_str());
    }
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
