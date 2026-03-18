use alloc::string::String as AllocString;

struct Shell2CmdEntry {
    name: &'static str,
    mode: &'static str,
    color: Option<(u8, u8, u8)>,
}

const SHELL2_CMD_REGISTRY: &[Shell2CmdEntry] = &[
    Shell2CmdEntry {
        name: "bench",
        mode: "cmd",
        color: None,
    },
    Shell2CmdEntry {
        name: "files",
        mode: "cmd",
        color: None,
    },
    Shell2CmdEntry {
        name: "install",
        mode: "cmd",
        color: Some((255, 55, 255)),
    },
    Shell2CmdEntry {
        name: "net",
        mode: "cmd",
        color: None,
    },
    Shell2CmdEntry {
        name: "tlb",
        mode: "cmd",
        color: None,
    },
    Shell2CmdEntry {
        name: "txt",
        mode: "cmd",
        color: None,
    },
    Shell2CmdEntry {
        name: "update",
        mode: "cmd",
        color: Some((255, 55, 255)),
    },
    Shell2CmdEntry {
        name: "set",
        mode: "cmd",
        color: None,
    },
];

pub(crate) fn command_names_status_text() -> AllocString {
    let mut out = AllocString::new();
    for (idx, entry) in SHELL2_CMD_REGISTRY.iter().enumerate() {
        if idx != 0 {
            out.push(' ');
        }
        if let Some(color) = entry.color {
            let styled = alloc::format!(
                "{}",
                super::ecma48::style(entry.name).bold().fg(color)
            );
            out.push_str(styled.as_str());
        } else {
            out.push_str(entry.name);
        }
    }
    out
}

pub(crate) fn command_registry_json() -> AllocString {
    let mut out = AllocString::from("{\"version\":1,\"commands\":[");
    for (idx, entry) in SHELL2_CMD_REGISTRY.iter().enumerate() {
        if idx != 0 {
            out.push(',');
        }
        out.push_str("{\"name\":\"");
        out.push_str(entry.name);
        out.push_str("\",\"mode\":\"");
        out.push_str(entry.mode);
        out.push_str("\"}");
    }
    out.push_str("]}");
    out
}
