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
const STATUS_GREEN_SQUARE_BRACKET_RGB: (u8, u8, u8) = (78, 232, 136);
const STATUS_PINK_RGB: (u8, u8, u8) = (255, 55, 255);
const STATUS_BLUE_RGB: (u8, u8, u8) = (120, 210, 255);
const STATUS_NETWORK_RGB: (u8, u8, u8) = (70, 220, 210);
const STATUS_ORANGE_RGB: (u8, u8, u8) = (255, 190, 90);
const STATUS_GRAY_RGB: (u8, u8, u8) = (160, 168, 176);
const STATUS_RAINBOW_COLORS: [u8; 8] = [199, 208, 227, 121, 51, 39, 99, 201];

const TOOL_JSON_ACPI: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["reboot","S1","S2","S3","S4","S5"],"description":"ACPI action to run."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_7Z: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS path. Non-.7z files compress to a sibling .7z archive; .7z archives extract beside the archive."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_CPP: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["start","list","status","stop","font","spirit","svg"],"description":"Launch, inspect, or stop the C++/IGC suite, stamp/present font RGBA, select Spirit's C++ repass, or control the SVG experiment."},"mode":{"type":"string","enum":["gallery","aurora","julia","sdf","voronoi","retro-sun","audio","particle"],"description":"C++ for OpenCL workload to display."},"font_action":{"type":"string","enum":["stamp","present","status","release"],"description":"Create an owned async RGBA stamp or present it directly through UI4."},"text":{"type":"string","maxLength":4096,"description":"UTF-8 text for action=font; newlines create rows."},"font":{"type":"integer","minimum":1,"maximum":3,"description":"Optional GPU font face for action=font."},"size":{"type":"number","minimum":4,"maximum":2048,"description":"Font pixel size for action=font."},"color":{"type":"string","description":"Font RGBA color encoded as RRGGBBAA."},"canvas":{"type":"string","description":"Optional WIDTHxHEIGHT RGBA8 canvas at or below the UHD/4K soft cap."},"duration_ms":{"type":"integer","minimum":0,"description":"Demo lifetime in milliseconds; zero runs until stopped."},"cadence_ms":{"type":"integer","minimum":1,"maximum":60000,"description":"Target GPU launch cadence in milliseconds."},"publish_every":{"type":"integer","minimum":1,"maximum":1024,"description":"Publish every Nth retired GPU frame."},"background_id":{"type":"integer","enum":[0,2,3,4,5,6,7,8,9,10,11],"description":"Spirit background ID when action is spirit; 11 is the UTC MagicTimeCircle."},"shader_id":{"type":"integer","minimum":0,"maximum":15,"description":"Spirit sprite shader ID when action is spirit."},"svg_action":{"type":"string","enum":["start","status","stop"],"description":"SVG-experiment lifecycle action when action=svg."},"svg_demo":{"type":"string","enum":["basic","curves","holes"],"description":"Byte-embedded SVG outline experiment selected when action=svg."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_DISC: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format","ramdisc","log"],"description":"disc action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format or optional disk id for action=log."},"size":{"type":"string","description":"Optional ramdisc size like 512MB or 1GiB for action=ramdisc."},"max":{"type":"integer","minimum":1,"maximum":4096,"description":"Maximum raw TRUEOSFS log records to print for action=log."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_GRID: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_VGPU: &str = r#"{"type":"object","properties":{"command":{"type":"string","enum":["status","test"],"description":"Inspect the vGPU broker or run a runtime test."},"test":{"type":"string","enum":["broker","abi","guc","compute","font","all"],"description":"Runtime test selected when command=test."}},"required":["command"],"additionalProperties":false}"#;
const TOOL_JSON_HYPER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","probe"],"description":"Hyper transport view to print."},"url":{"type":"string","description":"Optional URL to download into TRUEOSFS."},"path":{"type":"string","description":"Optional TRUEOSFS destination path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_LSD: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional TRUEOSFS path to list."},"paths":{"type":"array","items":{"type":"string"},"description":"Optional TRUEOSFS paths to list."},"long":{"type":"boolean","description":"Show file kind, ownership, byte size, and name."},"tree":{"type":"boolean","description":"Walk recursively from the path."},"table":{"type":"boolean","description":"Render the shell2 table view."},"archive7z":{"type":"boolean","description":"Inspect a .7z archive and print its entries without extracting."},"oneline":{"type":"boolean","description":"Show one entry per line."},"directory_only":{"type":"boolean","description":"List directories themselves instead of their contents."},"color":{"type":"string","enum":["always","auto","never"],"description":"Color output mode."},"size":{"type":"string","enum":["default","short","bytes"],"description":"Size display mode."},"permission":{"type":"string","enum":["rwx","octal","attributes","disable"],"description":"Permission display mode."},"sort":{"type":"string","enum":["name","size","extension","none"],"description":"Sort entries."},"reverse":{"type":"boolean","description":"Reverse the selected sort."},"group_dirs":{"type":"string","enum":["none","first","last"],"description":"Group directories before or after files."},"depth":{"type":"integer","minimum":0,"description":"Maximum recursive depth."},"header":{"type":"boolean","description":"Show long-output headers."}},"required":[],"additionalProperties":false}"#;
#[cfg(feature = "trueos_lumen")]
const TOOL_JSON_LUM: &str = r#"{"type":"object","properties":{"prompt":{"type":"string","description":"One sentence for the LFM2.5 assistant."}},"required":["prompt"],"additionalProperties":false}"#;
const TOOL_JSON_MV: &str = r#"{"type":"object","properties":{"src":{"type":"string","description":"Source TRUEOSFS path."},"dst":{"type":"string","description":"Destination TRUEOSFS path."},"regex":{"type":"string","description":"Optional -regx pattern. When set, src and dst are directories."}},"required":["src","dst"],"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_QJS: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_RAPL: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["store"],"description":"Store the current in-memory RAPL history in TRUEOSFS."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_RM: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file or directory path."},"regex":{"type":"string","description":"Optional -regx pattern to match children under path."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SET: &str = r#"{"type":"object","properties":{"width":{"type":"integer","minimum":50,"maximum":500,"description":"Shell line width."}},"required":["width"],"additionalProperties":false}"#;
const TOOL_JSON_SHA: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS file to hash with SHA-256."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SSH: &str = r#"{"type":"object","properties":{"endpoint":{"type":"string","description":"SSH target in [user@]host[:port] form. Port 22 is used when omitted."}},"required":["endpoint"],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pcibar","mem","cpu","turbo","ucode","pmu","rapl","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","smbios","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;
const TOOL_JSON_TXT: &str = r#"{"type":"object","properties":{"file":{"type":"string","description":"Optional file path to open in the Blueprint terminal editor."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_TTS: &str = r#"{"type":"object","properties":{"text":{"type":"string","description":"Text to synthesize and play through the kernel HDA lane."},"voice":{"type":"string","description":"Kokoro voice name; defaults to af_heart."},"speed":{"type":"number","minimum":0.25,"maximum":4.0,"description":"Speech speed multiplier."}},"required":["text"],"additionalProperties":false}"#;
const TOOL_JSON_STT: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS path to a mono/stereo signed-16-bit PCM WAV file."},"language":{"type":"string","description":"Whisper language code or auto."},"translate":{"type":"boolean","description":"Translate recognized speech to English."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_VID: &str = r#"{"type":"object","properties":{"source":{"type":"string","enum":["fs","on","online"],"description":"Read an Annex-B asset from TRUEOSFS, or download the fixed online AVC1 MP4 asset."},"path":{"type":"string","description":"Optional TRUEOSFS Annex-B path when source=fs; defaults to x31_head_movie.annexb.h264."},"loop":{"type":"boolean","description":"Repeat playback while retaining the same UI4 Frame and window lifetime."}},"required":["source"],"additionalProperties":false}"#;
const TOOL_JSON_XHCI: &str = r#"{"type":"object","properties":{"command":{"type":"string","enum":["status","journal","stage","read","read64","write","write64","rmw"],"description":"xHCI laboratory operation."},"stage":{"type":"integer","minimum":1,"maximum":5,"description":"Cumulative diagnostic stage."},"port":{"type":"integer","minimum":1,"maximum":255,"description":"Physical root port for mutating stages."},"offset":{"type":"string","description":"BAR-relative register offset, decimal or 0x-prefixed."},"value":{"type":"string","description":"Raw register value, decimal or 0x-prefixed."},"clear_mask":{"type":"string","description":"Raw RMW clear mask."},"set_mask":{"type":"string","description":"Raw RMW set mask."},"arm":{"type":"boolean","description":"Explicitly arm a mutating operation."},"live":{"type":"boolean","description":"Acknowledge disruption of a physically connected target."},"fused":{"type":"boolean","description":"Explicitly permit targeting the fused LED port."},"depth":{"type":"integer","minimum":1,"maximum":3,"description":"Stage-five transition-tree depth."}},"required":["command"],"additionalProperties":false}"#;

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

#[cfg(feature = "trueos_lumen")]
fn dispatch_lum(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::lumen::try_parse(spawner, io, rest)
}

fn dispatch_mv(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::mv::try_parse(io, "mv", rest)
}

fn dispatch_move(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::mv::try_parse(io, "move", rest)
}

fn dispatch_rm(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(spawner, io, "rm", rest)
}

fn dispatch_remove(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(spawner, io, "remove", rest)
}

fn dispatch_delete(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(spawner, io, "delete", rest)
}

fn dispatch_del(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rm::try_parse(spawner, io, "del", rest)
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

fn dispatch_ssh(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::ssh::try_parse(spawner, io, rest)
}

fn dispatch_update(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::update::try_parse(spawner, io, &mut args)
}

fn dispatch_cpp(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::cpp::try_parse(spawner, io, rest)
}

fn dispatch_cry(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::cry::try_parse(io, rest)
}

fn dispatch_disc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::disc::try_parse(io, &mut args)
}

fn dispatch_fslog(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::fslog::try_parse(io, rest)
}

fn dispatch_grid(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::grid::try_parse(spawner, io, rest)
}

fn dispatch_vgpu(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::vgpu::try_parse(io, rest)
}

fn dispatch_net(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let _ = spawner;
    let mut args = rest.split_whitespace();
    super::cmds::net::try_parse(io, &mut args)
}

fn dispatch_qjs(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::qjs::try_parse(spawner, io, rest)
}

fn dispatch_rapl(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::rapl::try_parse(spawner, io, rest)
}

fn dispatch_tlb(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::tlb::try_parse(spawner, io, &mut args)
}

fn dispatch_txt(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::txt::try_parse(spawner, io, rest)
}

fn dispatch_tts(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::ttstt::try_parse_tts(spawner, io, rest)
}

fn dispatch_stt(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::ttstt::try_parse_stt(spawner, io, rest)
}

fn dispatch_vid(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::vid::try_parse(spawner, io, rest)
}

fn dispatch_xhci(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::xhci::try_parse(spawner, io, rest)
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
        name: "cpp",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_cpp,
        tool_description: Some(
            "Launch the C++/IGC demos, run the SVG experiment, request retained asynchronous RGBA8 font stamps, or select Spirit's 9x16 visual suite.",
        ),
        tool_parameters_json: Some(TOOL_JSON_CPP),
    },
    BuiltinShell2CmdEntry {
        name: "cry",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_cry,
        tool_description: None,
        tool_parameters_json: None,
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
        name: "fslog",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: false,
        handler: dispatch_fslog,
        tool_description: None,
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "grid",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_grid,
        tool_description: Some("Open the Gridpaper Blueprint."),
        tool_parameters_json: Some(TOOL_JSON_GRID),
    },
    BuiltinShell2CmdEntry {
        name: "vgpu",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_vgpu,
        tool_description: Some("Inspect and validate the mediated virtual GPU boundary."),
        tool_parameters_json: Some(TOOL_JSON_VGPU),
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
    #[cfg(feature = "trueos_lumen")]
    BuiltinShell2CmdEntry {
        name: "lum",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_lum,
        tool_description: Some(
            "Reply to one quoted sentence with the CPU + Intel C++/IGC LFM2.5 assistant.",
        ),
        tool_parameters_json: Some(TOOL_JSON_LUM),
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
        name: "qjs",
        mode: "tui",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_qjs,
        tool_description: Some(
            "Open the persistent QuickJS scripting workbench. Exit with ESC or :quit.",
        ),
        tool_parameters_json: Some(TOOL_JSON_QJS),
    },
    BuiltinShell2CmdEntry {
        name: "rapl",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_rapl,
        tool_description: Some("Store the current in-memory RAPL history in TRUEOSFS."),
        tool_parameters_json: Some(TOOL_JSON_RAPL),
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
        tool_description: Some("Open a file in the txt Blueprint terminal editor."),
        tool_parameters_json: Some(TOOL_JSON_TXT),
    },
    BuiltinShell2CmdEntry {
        name: "xhci",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_xhci,
        tool_description: Some(
            "Run the quarantined live-owner xHCI register laboratory and transition-tree probes.",
        ),
        tool_parameters_json: Some(TOOL_JSON_XHCI),
    },
    BuiltinShell2CmdEntry {
        name: "tts",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_tts,
        tool_description: Some(
            "Synthesize text through the resident AP2+ TTSTT CPU service and queue PCM to Intel HDA.",
        ),
        tool_parameters_json: Some(TOOL_JSON_TTS),
    },
    BuiltinShell2CmdEntry {
        name: "stt",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_stt,
        tool_description: Some(
            "Transcribe a TRUEOSFS PCM WAV through the resident AP2+ TTSTT CPU service.",
        ),
        tool_parameters_json: Some(TOOL_JSON_STT),
    },
    BuiltinShell2CmdEntry {
        name: "vid",
        mode: "cmd",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_vid,
        tool_description: Some(
            "Play a TRUEOSFS Annex-B asset or the fixed online H.264 asset through VDBOX and the UI4 double-Frame path.",
        ),
        tool_parameters_json: Some(TOOL_JSON_VID),
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
    BuiltinShell2CmdEntry {
        name: "ssh",
        mode: "tui",
        color: Some(STATUS_NETWORK_RGB),
        advertised: true,
        handler: dispatch_ssh,
        tool_description: Some(
            "Open an authenticated SSH-2 PTY session through the ssh Blueprint.",
        ),
        tool_parameters_json: Some(TOOL_JSON_SSH),
    },
];

fn starts_with_command<'a>(submitted: &'a str, name: &str) -> Option<&'a str> {
    // Registry names are ASCII, but submitted text is unrestricted UTF-8.
    // A byte length belonging to some unrelated command name can fall inside
    // a multibyte scalar in `submitted`; checked slicing must reject that entry
    // instead of panicking before the matching command is reached.
    let head = submitted.get(..name.len())?;
    let tail = submitted.get(name.len()..)?;
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

#[cfg(test)]
mod tests {
    use super::starts_with_command;

    #[test]
    fn unrelated_command_length_may_land_inside_utf8() {
        let submitted = "cpp font stamp \"中国 § العربية 🦀\"";

        assert_eq!(starts_with_command(submitted, "install"), None);
        assert_eq!(
            starts_with_command(submitted, "cpp"),
            Some(" font stamp \"中国 § العربية 🦀\"")
        );
    }

    #[test]
    fn command_match_still_requires_a_token_boundary() {
        assert_eq!(starts_with_command("cppish", "cpp"), None);
        assert_eq!(starts_with_command("CPP\tfont status", "cpp"), Some("\tfont status"));
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
        "7z", "lsd", "rm", "mv", "sha", "disc", "install", "update", "hyper", "net", "qjs", "ssh",
        "txt", "grid", "tts", "stt", "cpp", "vgpu", "vid", "cry", "acpi", "rapl", "tlb", "smp",
        "etc",
    ];

    let mut out = AllocString::new();

    let mut first = true;
    for name in STATUS_ORDER {
        // `rm` and `mv` remain separate commands; only their statusbar glyphs overlap.
        if *name == "mv" {
            continue;
        }
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
        if entry.name == "rm" {
            push_rm_mv_status_token(&mut out);
        } else {
            push_status_command_name(&mut out, entry);
        }
    }

    out
}

fn push_rm_mv_status_token(out: &mut AllocString) {
    for ch in "(r[m)v]".chars() {
        let mut glyph = [0u8; 4];
        let glyph = ch.encode_utf8(&mut glyph);
        let color = if matches!(ch, '[' | ']') {
            STATUS_GREEN_SQUARE_BRACKET_RGB
        } else {
            STATUS_GREEN_RGB
        };
        let styled = alloc::format!("{}", super::term_style::paint(glyph).bold().color(color));
        out.push_str(styled.as_str());
    }
}

fn push_status_command_name(out: &mut AllocString, entry: &BuiltinShell2CmdEntry) {
    let label = status_command_label(entry);

    if entry.name == "cpp" {
        push_static_rainbow_token(out, label);
    } else if let Some(color) = entry.color {
        let styled = alloc::format!("{}", super::term_style::paint(label).bold().color(color));
        out.push_str(styled.as_str());
    } else {
        out.push_str(label);
    }
}

fn status_command_label(entry: &BuiltinShell2CmdEntry) -> &'static str {
    entry.name
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
