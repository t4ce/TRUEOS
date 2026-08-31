use alloc::string::String as AllocString;

use trueos_executor::Spawner;

use super::ShellBackend2;
use super::shell2_cmd::ParseOutcome;

pub(crate) type Shell2CmdHandler = fn(&Spawner, &'static dyn ShellBackend2, &str) -> ParseOutcome;

#[derive(Clone, Copy)]
struct BuiltinShell2CmdEntry {
    name: &'static str,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    mode: &'static str,
    color: Option<(u8, u8, u8)>,
    advertised: bool,
    handler: Shell2CmdHandler,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    tool_description: Option<&'static str>,
    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    tool_parameters_json: Option<&'static str>,
}

const STATUS_GREEN_RGB: (u8, u8, u8) = (60, 220, 120);
const STATUS_PINK_RGB: (u8, u8, u8) = (255, 55, 255);
const STATUS_BLUE_RGB: (u8, u8, u8) = (120, 210, 255);
const STATUS_NETWORK_RGB: (u8, u8, u8) = (70, 220, 210);
const STATUS_ORANGE_RGB: (u8, u8, u8) = (255, 190, 90);
const STATUS_GRAY_RGB: (u8, u8, u8) = (160, 168, 176);
const STATUS_DARK_RED_RGB: (u8, u8, u8) = (139, 0, 0);
const STATUS_RAINBOW_COLORS: [u8; 8] = [199, 208, 227, 121, 51, 39, 99, 201];

const TOOL_JSON_ACPI: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["reboot","S1","S2","S3","S4","S5"],"description":"ACPI action to run."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_AUD: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_BIOS: &str = r#"{"type":"object","properties":{"view":{"type":"string","enum":["all","status","services","setup","handoff","hints"],"description":"BIOS/UEFI control-plane view to print."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_CPP: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","status","stop","font","spirit","svg"],"description":"Inspect or stop the interactive C++/IGC gallery, stamp/present/rush/rush2 font RGBA, select Spirit's C++ repass, or control the SVG experiment. Omit action to launch the gallery."},"font_action":{"type":"string","enum":["stamp","present","rush","rush2","status","release"],"description":"Create an owned async RGBA stamp, present it through UI4, or control the staged Unicode glyph rush or UI4-native 8-worker rush2 within the 32-slot producer pool."},"rush_action":{"type":"string","enum":["start","stop"],"description":"Start or stop the font rush when action=font and font_action=rush; start is the default."},"rush2_action":{"type":"string","enum":["start","stop"],"description":"Start or stop the UI4-native semi-persistent 8-worker font rush2 within the 32-slot producer pool when action=font and font_action=rush2; start is the default."},"text":{"type":"string","maxLength":4096,"description":"UTF-8 text for action=font; newlines create rows."},"font":{"type":"integer","minimum":1,"maximum":3,"description":"Optional GPU font face for action=font."},"size":{"type":"number","minimum":4,"maximum":2048,"description":"Font pixel size for action=font."},"color":{"type":"string","description":"Font RGBA color encoded as RRGGBBAA."},"canvas":{"type":"string","description":"Optional WIDTHxHEIGHT RGBA8 canvas at or below the UHD/4K soft cap."},"background_id":{"type":"integer","enum":[0,2,3,4,5,6,7,8,9,10,11],"description":"Spirit background ID when action is spirit; 11 is the UTC MagicTimeCircle."},"shader_id":{"type":"integer","minimum":0,"maximum":15,"description":"Spirit sprite shader ID when action is spirit."},"svg_action":{"type":"string","enum":["start","status","stop"],"description":"SVG-experiment lifecycle action when action=svg."},"svg_demo":{"type":"string","enum":["basic","curves","holes"],"description":"Byte-embedded SVG outline experiment selected when action=svg."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_DISC: &str = r#"{"type":"object","properties":{"action":{"type":"string","enum":["list","format","ramdisc"],"description":"disc action to run."},"disk_id":{"type":"string","description":"Disk id string for action=format."},"size":{"type":"string","description":"Optional ramdisc size like 512MB or 1GiB for action=ramdisc."}},"required":["action"],"additionalProperties":false}"#;
const TOOL_JSON_GRID: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_VGPU: &str = r#"{"type":"object","properties":{"command":{"type":"string","enum":["status","test"],"description":"Inspect the vGPU broker or run a runtime test."},"test":{"type":"string","enum":["broker","abi","guc","compute","blit","all"],"description":"Runtime test selected when command=test."}},"required":["command"],"additionalProperties":false}"#;
const TOOL_JSON_HYPER: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["status","probe"],"description":"Hyper transport view to print."},"url":{"type":"string","description":"Optional URL to download into TRUEOSFS."},"path":{"type":"string","description":"Optional TRUEOSFS destination path."}},"required":[],"additionalProperties":false}"#;
#[cfg(feature = "trueos_lumen")]
const TOOL_JSON_LUM: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_NET: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["icmp","irc","nic","hostname"],"description":"net subcommand to run."},"target":{"type":"string","description":"Target host for net icmp."},"selector":{"type":"string","description":"Optional NIC selector like index, vid:pid, or bb:dd.f."},"host":{"type":"string","description":"Host for net irc."},"channel":{"type":"string","description":"Optional channel like #trueos for net irc."},"name":{"type":"string","description":"Optional hostname for net hostname."}},"required":["subcommand"],"additionalProperties":false}"#;
const TOOL_JSON_QJS: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_RAM: &str = r#"{"type":"object","properties":{"scope":{"type":"string","description":"Optional pmm, host, or numeric VM id. Omit to list all configured RAM scopes."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SHOT: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_SMP: &str = r#"{"type":"object","properties":{"slot":{"type":"integer","minimum":0,"description":"Optional SMP slot. Omit to list all slots."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_IMG: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"Optional image path to open as the first UI4 frame. Omit for img's resident interactive viewer."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SSH: &str = r#"{"type":"object","properties":{"endpoint":{"type":"string","description":"Optional SSH target in [user@]host[:port] form. Omit for SSH's resident interactive prompt."}},"required":[],"additionalProperties":false}"#;
const TOOL_JSON_SURF: &str = r#"{"type":"object","properties":{"subcommand":{"type":"string","enum":["https","http","file","html"],"description":"Surf input type."},"input":{"type":"string","description":"Host, URL, TRUEOSFS path, or inline HTML selected by subcommand."}},"required":["subcommand","input"],"additionalProperties":false}"#;
const TOOL_JSON_TLB: &str = r#"{"type":"object","properties":{"target":{"type":"string","enum":["pci","pcibar","mem","cpu","hfi","turbo","ucode","pmu","rapl","acpi","aml","facp","madt","hpet","mcfg","ssdt","uefi","smbios","x2apic","usb","usb_probe","dump"],"description":"Table or view to print."},"action":{"type":"string","enum":["store"],"description":"Optional RAPL action when target=rapl."},"signature":{"type":"string","minLength":4,"maxLength":4,"description":"Optional ACPI signature when target=acpi, for example SSDT or FACP."},"index":{"type":"integer","minimum":1,"description":"Optional 1-based instance index when target=acpi and the signature repeats."},"subcommand":{"type":"string","enum":["ec","symbol","prefix"],"description":"Optional AML subcommand when target=aml."},"path":{"type":"string","description":"Optional AML path or prefix when target=aml and subcommand is symbol or prefix."}},"required":["target"],"additionalProperties":false}"#;
const TOOL_JSON_TTS: &str = r#"{"type":"object","properties":{"text":{"type":"string","maxLength":8192,"description":"Text to synthesize asynchronously. The native backend performs G2P and splits it into ordered model chunks of at most 510 phonemes."},"voice":{"type":"string","description":"Kokoro voice name; defaults to af_heart."},"speed":{"type":"number","minimum":0.5,"maximum":2.0,"description":"Kokoro speech speed multiplier."}},"required":["text"],"additionalProperties":false}"#;
const TOOL_JSON_STT: &str = r#"{"type":"object","properties":{"path":{"type":"string","description":"TRUEOSFS path to a mono/stereo signed-16-bit PCM WAV file."},"language":{"type":"string","description":"Whisper language code or auto."},"translate":{"type":"boolean","description":"Translate recognized speech to English."}},"required":["path"],"additionalProperties":false}"#;
const TOOL_JSON_TD: &str = r#"{"type":"object","properties":{},"additionalProperties":false}"#;
const TOOL_JSON_VID: &str = r#"{"type":"object","properties":{"source":{"type":"string","enum":["fs","on","online"],"description":"Read an Annex-B asset from TRUEOSFS, or download the fixed online AVC1 MP4 asset."},"path":{"type":"string","description":"Optional TRUEOSFS Annex-B path when source=fs; defaults to x31_head_movie.annexb.h264."},"loop":{"type":"boolean","description":"Repeat playback while retaining the same UI4 Frame and window lifetime."}},"required":["source"],"additionalProperties":false}"#;
const TOOL_JSON_XHCI: &str = r#"{"type":"object","properties":{"command":{"type":"string","enum":["status","journal","stage","read","read64","write","write64","rmw"],"description":"xHCI laboratory operation."},"stage":{"type":"integer","minimum":1,"maximum":5,"description":"Cumulative diagnostic stage."},"port":{"type":"integer","minimum":1,"maximum":255,"description":"Physical root port for mutating stages."},"offset":{"type":"string","description":"BAR-relative register offset, decimal or 0x-prefixed."},"value":{"type":"string","description":"Raw register value, decimal or 0x-prefixed."},"clear_mask":{"type":"string","description":"Raw RMW clear mask."},"set_mask":{"type":"string","description":"Raw RMW set mask."},"arm":{"type":"boolean","description":"Explicitly arm a mutating operation."},"live":{"type":"boolean","description":"Acknowledge disruption of a physically connected target."},"fused":{"type":"boolean","description":"Explicitly permit targeting the fused LED port."},"depth":{"type":"integer","minimum":1,"maximum":3,"description":"Stage-five transition-tree depth."}},"required":["command"],"additionalProperties":false}"#;

fn dispatch_acpi(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::acpi::try_parse(io, &mut args)
}

fn dispatch_aud(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::aud::try_parse(spawner, io, rest)
}

fn dispatch_bios(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::bios::try_parse(io, rest)
}

fn dispatch_img(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::img::try_parse(spawner, io, rest)
}

fn dispatch_hyper(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::hyper::try_parse(spawner, io, &mut args)
}

#[cfg(feature = "trueos_lumen")]
fn dispatch_lum(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::lum::try_parse(spawner, io, rest)
}

fn dispatch_shot(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let rest = rest.trim();
    if matches!(rest, "help" | "-h" | "--help") {
        super::print_shell_line(
            io,
            "shot: capture the next Pipe-C/WD post-blend frame to trueosfs:/screenshots",
        );
        return ParseOutcome::Handled;
    }
    if !rest.is_empty() {
        super::print_shell_line(io, "shot: usage `shot`");
        return ParseOutcome::Handled;
    }
    match crate::ui4::request_wd_postblend_capture() {
        Ok(()) => {
            crate::intel::begin_transient_global_gt_boost(spawner, "shell2-shot");
            super::print_shell_line(
                io,
                "shot: armed; next WD frame will be saved under trueosfs:/screenshots",
            );
        }
        Err(reason) => super::print_shell_line(io, alloc::format!("shot: {reason}").as_str()),
    }
    ParseOutcome::Handled
}

fn dispatch_td(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::td::try_parse(spawner, io, rest)
}

fn dispatch_smp(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::smp::try_parse(io, &mut args)
}

fn dispatch_ssh(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::ssh::try_parse(spawner, io, rest)
}

fn dispatch_surf(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::surf::try_parse(spawner, io, rest)
}

fn dispatch_os(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::os::try_parse(spawner, io, rest)
}

fn dispatch_cpp(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::cpp::try_parse(spawner, io, rest)
}

fn dispatch_cry(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::cry::try_parse(spawner, io, rest)
}

fn dispatch_disc(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::disc::try_parse(io, &mut args)
}

fn dispatch_edit(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::edit::try_parse(spawner, io, rest)
}

fn dispatch_shell(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    super::cmds::shell::try_parse(spawner, io, rest)
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

fn dispatch_ram(_: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::ram::try_parse(io, &mut args)
}

fn dispatch_tlb(spawner: &Spawner, io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    super::cmds::tlb::try_parse(spawner, io, &mut args)
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

/// The authoritative Shell2 command registry. Its declaration order is also
/// the order of command names in the right-aligned Shell2 titlebar section.
const SHELL2_COMMAND_REGISTRY: &[BuiltinShell2CmdEntry] = &[
    BuiltinShell2CmdEntry {
        name: "acpi",
        mode: "cmd",
        color: Some(STATUS_DARK_RED_RGB),
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
        tool_description: Some(
            "Launch the Player Blueprint in VMX-minishell mode without its terminal TUI.",
        ),
        tool_parameters_json: Some(TOOL_JSON_AUD),
    },
    BuiltinShell2CmdEntry {
        name: "bios",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_bios,
        tool_description: Some("Inspect BIOS/UEFI control-plane state and handoff information."),
        tool_parameters_json: Some(TOOL_JSON_BIOS),
    },
    BuiltinShell2CmdEntry {
        name: "cpp",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_cpp,
        tool_description: Some(
            "Launch the keyboard-driven C++/IGC gallery, run the SVG experiment, request retained asynchronous RGBA8 font stamps, or select Spirit's 9x16 visual suite.",
        ),
        tool_parameters_json: Some(TOOL_JSON_CPP),
    },
    BuiltinShell2CmdEntry {
        name: "cry",
        mode: "cmd",
        color: Some(STATUS_PINK_RGB),
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
        tool_description: Some("List top-level disk devices, format a disk, or create a ramdisc."),
        tool_parameters_json: Some(TOOL_JSON_DISC),
    },
    BuiltinShell2CmdEntry {
        name: "img",
        mode: "tui",
        color: Some(STATUS_BLUE_RGB),
        advertised: true,
        handler: dispatch_img,
        tool_description: Some(
            "Open the resident UI4 image viewer. A path opens its first frame; use its prompt to add up to 32 decoded media frames.",
        ),
        tool_parameters_json: Some(TOOL_JSON_IMG),
    },
    BuiltinShell2CmdEntry {
        name: "edit",
        mode: "tui",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_edit,
        tool_description: Some("Open the app.db-backed terminal editor."),
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "grid",
        mode: "cmd",
        color: Some(STATUS_ORANGE_RGB),
        advertised: true,
        handler: dispatch_grid,
        tool_description: Some("Launch the online Gridpaper app."),
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
        name: "os",
        mode: "tui",
        color: Some(STATUS_PINK_RGB),
        advertised: true,
        handler: dispatch_os,
        tool_description: Some(
            "Open the OS administration TUI for disk installation or a live kernel update.",
        ),
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "shell",
        mode: "tui",
        color: Some(STATUS_PINK_RGB),
        advertised: true,
        handler: dispatch_shell,
        tool_description: Some(
            "Open a UI4 Shell2 session and enter the same session through the invoking Matrix terminal.",
        ),
        tool_parameters_json: None,
    },
    BuiltinShell2CmdEntry {
        name: "hyper",
        mode: "cmd",
        color: Some(STATUS_NETWORK_RGB),
        advertised: true,
        handler: dispatch_hyper,
        tool_description: Some("Inspect the kernel Hyper HTTP/HTTPS transport surface."),
        tool_parameters_json: Some(TOOL_JSON_HYPER),
    },
    BuiltinShell2CmdEntry {
        name: "surf",
        mode: "cmd",
        color: Some(STATUS_NETWORK_RGB),
        advertised: true,
        handler: dispatch_surf,
        tool_description: Some(
            "Render an HTTPS, HTTP, TRUEOSFS, or inline HTML source through Solara.",
        ),
        tool_parameters_json: Some(TOOL_JSON_SURF),
    },
    BuiltinShell2CmdEntry {
        name: "shot",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_shot,
        tool_description: Some(
            "Capture one Pipe-C/WD post-blend frame and save a diagnostic PNG to TRUEOSFS.",
        ),
        tool_parameters_json: Some(TOOL_JSON_SHOT),
    },
    BuiltinShell2CmdEntry {
        name: "td",
        mode: "cmd",
        color: Some(STATUS_GREEN_RGB),
        advertised: true,
        handler: dispatch_td,
        tool_description: Some("Launch termdir at the TRUEOSFS root with depth 2."),
        tool_parameters_json: Some(TOOL_JSON_TD),
    },
    #[cfg(feature = "trueos_lumen")]
    BuiltinShell2CmdEntry {
        name: "lum",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_lum,
        tool_description: Some("Open the replicatable Lumen Blueprint."),
        tool_parameters_json: Some(TOOL_JSON_LUM),
    },
    BuiltinShell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: Some(STATUS_NETWORK_RGB),
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
        name: "tlb",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_tlb,
        tool_description: Some("Print one of the table and hardware inspection views."),
        tool_parameters_json: Some(TOOL_JSON_TLB),
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
            "Queue serialized Kokoro synthesis and stream bounded stereo/48k PCM chunks into the live kernel playback lane.",
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
        name: "ram",
        mode: "cmd",
        color: Some(STATUS_GRAY_RGB),
        advertised: true,
        handler: dispatch_ram,
        tool_description: Some("Inspect current and recent physical and heap RAM use."),
        tool_parameters_json: Some(TOOL_JSON_RAM),
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
    use super::{TOOL_JSON_CPP, command_registry_json, starts_with_command};

    #[test]
    fn unrelated_command_length_may_land_inside_utf8() {
        let submitted = "cpp font stamp \"中国 § العربية 🦀\"";

        assert_eq!(starts_with_command(submitted, "os"), None);
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

    #[test]
    fn titlebar_accents_media_commands_and_acpi() {
        let status = titlebar_right_command_names_text();

        let rainbow_start = "\x1b[1;4;38;5;199m";
        assert_eq!(status.matches(rainbow_start).count(), 4);
        assert!(status.contains("\x1b[1;38;2;139;0;0macpi\x1b[0m"));
    }

    #[test]
    fn titlebar_groups_cry_and_display_only_backup_with_pink_commands() {
        let status = titlebar_right_command_names_text();
        let positions = ["cry", "backup", "os", "shell"].map(|label| {
            let token = alloc::format!("\x1b[1;38;2;255;55;255m{label}\x1b[0m");
            status.find(token.as_str()).unwrap()
        });

        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(!command_registry_json().contains("\"name\":\"backup\""));
    }

    #[test]
    fn titlebar_and_registry_omit_retired_commands() {
        let status = titlebar_right_command_names_text();
        let registry = command_registry_json();
        for label in ["gridp", "helio", "set"] {
            assert!(!status.contains(label), "titlebar contains retired {label}");
            assert!(
                !registry.contains(alloc::format!("\"name\":\"{label}\"").as_str()),
                "registry contains retired {label}"
            );
        }
        #[cfg(feature = "trueos_lumen")]
        assert!(status.contains("lum"));
        assert!(registry.contains("\"name\":\"bios\""));
    }

    #[test]
    fn titlebar_includes_green_file_surface_commands() {
        let status = titlebar_right_command_names_text();
        for label in ["td", "shot", "disc", "edit"] {
            assert!(status.contains(label), "titlebar is missing {label}");
        }

        let registry = command_registry_json();
        for retired in ["7z", "mv", "move", "rm", "remove", "delete", "del", "sha"] {
            assert!(!registry.contains(alloc::format!("\"name\":\"{retired}\"").as_str()));
        }
        assert!(registry.contains("\"name\":\"td\""));
        assert!(registry.contains("\"name\":\"edit\""));
    }

    #[test]
    fn cpp_tool_schema_keeps_gallery_launch_plain_and_font_rush_explicit() {
        assert!(!TOOL_JSON_CPP.contains("\"mode\":"));
        assert!(!TOOL_JSON_CPP.contains("\"duration_ms\":"));
        assert!(!TOOL_JSON_CPP.contains("\"action\":{\"type\":\"string\",\"enum\":[\"start\""));
        assert!(TOOL_JSON_CPP.contains(
            "\"font_action\":{\"type\":\"string\",\"enum\":[\"stamp\",\"present\",\"rush\",\"rush2\",\"status\",\"release\"]"
        ));
        assert!(
            TOOL_JSON_CPP
                .contains("\"rush_action\":{\"type\":\"string\",\"enum\":[\"start\",\"stop\"]")
        );
        assert!(
            TOOL_JSON_CPP
                .contains("\"rush2_action\":{\"type\":\"string\",\"enum\":[\"start\",\"stop\"]")
        );
        assert!(command_registry_json().contains("staged Unicode glyph rush"));
    }
}

pub(crate) fn try_dispatch(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    submitted: &str,
) -> ParseOutcome {
    for entry in SHELL2_COMMAND_REGISTRY {
        if let Some(rest) = starts_with_command(submitted, entry.name) {
            return (entry.handler)(spawner, io, rest);
        }
    }

    ParseOutcome::NotCommand
}

/// Render the command portion of Shell2's right-aligned titlebar section.
///
/// Every advertised entry in [`SHELL2_COMMAND_REGISTRY`] is included, so a
/// newly registered command cannot be omitted from this titlebar listing.
pub(crate) fn titlebar_right_command_names_text() -> AllocString {
    let mut out = AllocString::new();
    visit_titlebar_right_entries(|entry| {
        if !out.is_empty() {
            out.push(' ');
        }
        push_titlebar_right_entry(&mut out, entry);
    });

    out
}

/// Return complete, colorized command tokens that fit one titlebar segment.
/// The local UI4 frontend has fewer columns than the desktop terminal, so a
/// full legend must be shortened without splitting ANSI escape sequences.
pub(crate) fn titlebar_right_command_names_text_fitting(max_width: usize) -> AllocString {
    let full = titlebar_right_command_names_text();
    if super::ecma48::visible_width(full.as_str()) <= max_width {
        return full;
    }

    let content_width = max_width.saturating_sub(3);
    let mut out = AllocString::new();
    let mut truncated = false;
    visit_titlebar_right_entries(|entry| {
        if truncated {
            return;
        }
        let mut token = AllocString::new();
        push_titlebar_right_entry(&mut token, entry);

        let separator = if out.is_empty() { "" } else { " " };
        let candidate_width = super::ecma48::visible_width(out.as_str())
            .saturating_add(separator.len())
            .saturating_add(super::ecma48::visible_width(token.as_str()));
        if candidate_width > content_width {
            truncated = true;
            return;
        }
        out.push_str(separator);
        out.push_str(token.as_str());
    });
    if out.is_empty() {
        return AllocString::from("...");
    }
    out.push_str("...");
    out
}

fn visit_titlebar_right_entries(mut visit: impl FnMut(Option<&BuiltinShell2CmdEntry>)) {
    for entry in SHELL2_COMMAND_REGISTRY
        .iter()
        .filter(|entry| entry.advertised)
    {
        visit(Some(entry));
        if entry.name == "cry" {
            // `backup` is a titlebar-only visual label, not a shell command.
            visit(None);
        }
    }
}

fn push_titlebar_right_entry(out: &mut AllocString, entry: Option<&BuiltinShell2CmdEntry>) {
    let Some(entry) = entry else {
        push_colored_status_token(out, "backup", STATUS_PINK_RGB);
        return;
    };
    let label = status_command_label(entry);

    if matches!(entry.name, "cpp" | "vgpu" | "aud" | "vid") {
        push_static_rainbow_token(out, label);
    } else if let Some(color) = entry.color {
        push_colored_status_token(out, label, color);
    } else {
        out.push_str(label);
    }
}

fn push_colored_status_token(out: &mut AllocString, text: &str, color: (u8, u8, u8)) {
    let styled = alloc::format!("{}", super::term_style::paint(text).bold().color(color));
    out.push_str(styled.as_str());
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

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
pub(crate) fn command_registry_json() -> AllocString {
    let mut out = AllocString::from("{\"version\":1,\"commands\":[");
    let mut first = true;

    for entry in SHELL2_COMMAND_REGISTRY {
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
