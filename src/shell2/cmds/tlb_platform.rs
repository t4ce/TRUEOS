use core::fmt::Write;

use alloc::string::String;

use crate::efi::smbios;

const NCT5585_TOKEN: &str = "NCT5585";
const MEI_FIRMWARE_TOKENS: [&str; 5] = ["$MEI", "MEI1", "MEI2", "MEI3", "MEI4"];

pub(crate) fn append_dump(out: &mut String) {
    writeln!(out, "=== Platform Mining Hints ===").unwrap();
    writeln!(
        out,
        "capture_policy=read-only firmware evidence correlation; strings are hints, not claimed devices"
    )
    .unwrap();
    append_smbios_hints(out);
    writeln!(out).unwrap();
}

fn append_smbios_hints(out: &mut String) {
    let table = match smbios::discover() {
        Ok(table) => table,
        Err(error) => {
            writeln!(
                out,
                "smbios_mining=unavailable reason={} detail={:?}",
                error.label(),
                error
            )
            .unwrap();
            return;
        }
    };

    let mut structures = table.structures();
    let mut nct_hits = 0usize;
    let mut mei_hits = 0usize;

    loop {
        let structure = match structures.next_structure() {
            Ok(Some(structure)) => structure,
            Ok(None) => break,
            Err(error) => {
                writeln!(out, "smbios_mining=parse-stopped detail={:?}", error).unwrap();
                break;
            }
        };

        for (string_index, raw) in structure.strings().enumerate() {
            let text = firmware_text(raw);
            let upper = text.to_ascii_uppercase();
            if upper.contains(NCT5585_TOKEN) {
                nct_hits = nct_hits.saturating_add(1);
                writeln!(
                    out,
                    "smbios_hint kind=nuvoton-superio-candidate type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
                    structure.type_id,
                    structure.type_name(),
                    structure.handle,
                    string_index + 1,
                    text
                )
                .unwrap();
            }

            if MEI_FIRMWARE_TOKENS
                .iter()
                .any(|token| upper.trim() == *token)
            {
                mei_hits = mei_hits.saturating_add(1);
                writeln!(
                    out,
                    "smbios_hint kind=mei-name type={} ({}) handle=0x{:04X} string={} value=\"{}\"",
                    structure.type_id,
                    structure.type_name(),
                    structure.handle,
                    string_index + 1,
                    text
                )
                .unwrap();
            }
        }
    }

    if nct_hits == 0 {
        writeln!(out, "nct5585_candidate=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "nct5585_candidate=present source=smbios confidence=firmware-advertised-only hits={}",
            nct_hits
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_candidate_surfaces=hwmon/thermal fan/tach/PWM PECI GPIO UART LED Port80"
        )
        .unwrap();
        writeln!(
            out,
            "nct5585_next_probe=Super-I/O config identity + logical-device map; requires explicit write-gated config-mode sequence and is not executed by TLB"
        )
        .unwrap();
    }

    if mei_hits == 0 {
        writeln!(out, "mei_firmware_names=not-seen-in-smbios").unwrap();
    } else {
        writeln!(
            out,
            "mei_firmware_names=present source=smbios confidence=naming-only hits={}",
            mei_hits
        )
        .unwrap();
        writeln!(
            out,
            "mei_next_probe=correlate firmware names with PCI MEI/HECI functions; SMBIOS names alone do not establish a transport"
        )
        .unwrap();
    }
}

fn firmware_text(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\"' => out.push_str("\\\""),
            value if value.is_ascii_graphic() || value == b' ' => out.push(value as char),
            value => {
                write!(out, "\\x{:02X}", value).unwrap();
            }
        }
    }
    out
}
