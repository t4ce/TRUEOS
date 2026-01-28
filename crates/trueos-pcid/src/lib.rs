#![no_std]

mod data {
    include!(concat!(env!("OUT_DIR"), "/pciids_data.rs"));
}

fn is_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
}

fn hex_val(b: u8) -> Option<u16> {
    match b {
        b'0'..=b'9' => Some((b - b'0') as u16),
        b'a'..=b'f' => Some((b - b'a' + 10) as u16),
        b'A'..=b'F' => Some((b - b'A' + 10) as u16),
        _ => None,
    }
}

fn parse_hex4(s: &[u8]) -> Option<u16> {
    if s.len() < 4 || !is_hex(s[0]) || !is_hex(s[1]) || !is_hex(s[2]) || !is_hex(s[3]) {
        return None;
    }
    Some(
        (hex_val(s[0])? << 12)
            | (hex_val(s[1])? << 8)
            | (hex_val(s[2])? << 4)
            | (hex_val(s[3])?),
    )
}

fn trim_ascii_start(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn line_name_after_id<'a>(line: &'a [u8]) -> Option<&'a str> {
    // line: "XXXX  Name..." (or tab-prefixed variants)
    let line = trim_ascii_start(line);
    let mut i = 0;
    if parse_hex4(line.get(0..4)?).is_none() {
        return None;
    }
    i += 4;
    // Skip at least one space/tab
    while i < line.len() && (line[i] == b' ' || line[i] == b'\t') {
        i += 1;
    }
    if i >= line.len() {
        return None;
    }
    let name_bytes = &line[i..];
    core::str::from_utf8(name_bytes).ok()
}

/// Returns the vendor name for a PCI vendor ID (e.g. `0x8086`).
pub fn vendor_name(vendor_id: u16) -> Option<&'static str> {
    let bytes = data::PCI_IDS;

    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        // Skip comments.
        if line[0] == b'#' {
            continue;
        }
        // Vendor lines start at column 0 with 4 hex digits.
        if line[0] == b'\t' || line[0] == b' ' {
            continue;
        }
        let Some(id) = parse_hex4(line) else {
            continue;
        };
        if id == vendor_id {
            return line_name_after_id(line);
        }
    }

    None
}

/// Returns the device name for a vendor+device ID pair (e.g. `0x8086:0x100e`).
pub fn device_name(vendor_id: u16, device_id: u16) -> Option<&'static str> {
    let bytes = data::PCI_IDS;
    let mut in_vendor = false;

    for line in bytes.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        if line[0] == b'#' {
            continue;
        }

        if line[0] != b'\t' && line[0] != b' ' {
            // New vendor line.
            if let Some(id) = parse_hex4(line) {
                in_vendor = id == vendor_id;
            } else {
                in_vendor = false;
            }
            continue;
        }

        if !in_vendor {
            continue;
        }

        // Device lines are tab-indented once.
        if line[0] == b'\t' {
            let line2 = &line[1..];
            let Some(id) = parse_hex4(line2) else {
                continue;
            };
            if id == device_id {
                return line_name_after_id(line2);
            }
        }
    }

    None
}

pub fn pci_ids_bytes() -> &'static [u8] {
    data::PCI_IDS
}
