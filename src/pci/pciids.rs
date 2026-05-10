extern crate alloc;

use core::sync::atomic::{AtomicU8, Ordering};

pub const PCI_IDS_URL: &str = "https://raw.githubusercontent.com/pciutils/pciids/master/pci.ids";
pub const PCI_IDS_KEY: &str = "trueos/pci/pci.ids";
const PCI_IDS_TIMEOUT_MS: u32 = 120_000;
const PCI_IDS_MAX_BYTES: usize = 4 * 1024 * 1024;

const PCIIDS_GIT_IDLE: u8 = 0;
const PCIIDS_GIT_STARTED: u8 = 1;
const PCIIDS_GIT_FETCHING: u8 = 2;
const PCIIDS_GIT_VERIFYING: u8 = 3;
const PCIIDS_GIT_STORED: u8 = 4;
const PCIIDS_GIT_FAILED: u8 = 5;
const PCIIDS_GIT_VERIFY_MISSING: u8 = 6;
const PCIIDS_GIT_VERIFY_ERROR: u8 = 7;

static PCIIDS_GIT_STATE: AtomicU8 = AtomicU8::new(PCIIDS_GIT_IDLE);

fn set_pciids_git_state(state: u8) {
    PCIIDS_GIT_STATE.store(state, Ordering::Release);
}

pub fn download_once() -> Result<(), i32> {
    crate::log!(
        "pciids_git: fetch begin transport=hyper-https url={} key={} timeout_ms={} max_bytes={}\n",
        PCI_IDS_URL,
        PCI_IDS_KEY,
        PCI_IDS_TIMEOUT_MS,
        PCI_IDS_MAX_BYTES
    );
    crate::t::net::fetch_https_to_file_hyper(
        "pciids_git",
        PCI_IDS_URL,
        PCI_IDS_KEY,
        PCI_IDS_TIMEOUT_MS,
        PCI_IDS_MAX_BYTES,
    )
}

#[embassy_executor::task]
pub async fn pciids_git_task() {
    set_pciids_git_state(PCIIDS_GIT_STARTED);
    crate::log!(
        "pciids_git: task start marker ready=0x{:08X} url={} key={}\n",
        crate::r::readiness::mask(),
        PCI_IDS_URL,
        PCI_IDS_KEY
    );
    set_pciids_git_state(PCIIDS_GIT_FETCHING);
    match download_once() {
        Ok(()) => {
            set_pciids_git_state(PCIIDS_GIT_VERIFYING);
            match load_raw_from_root_blocking() {
                Ok(Some(raw)) => {
                    set_pciids_git_state(PCIIDS_GIT_STORED);
                    crate::log!(
                        "pciids_git: finished marker key={} bytes={} state=stored\n",
                        PCI_IDS_KEY,
                        raw.len()
                    );
                }
                Ok(None) => {
                    set_pciids_git_state(PCIIDS_GIT_VERIFY_MISSING);
                    crate::log!(
                        "pciids_git: verify missing marker key={} after fetch\n",
                        PCI_IDS_KEY
                    );
                }
                Err(err) => {
                    set_pciids_git_state(PCIIDS_GIT_VERIFY_ERROR);
                    crate::log!(
                        "pciids_git: verify error marker key={} err={:?}\n",
                        PCI_IDS_KEY,
                        err
                    );
                }
            }
        }
        Err(rc) => {
            set_pciids_git_state(PCIIDS_GIT_FAILED);
            crate::log!(
                "pciids_git: failed marker rc={} url={} state=fetch_failed\n",
                rc,
                PCI_IDS_URL
            );
        }
    }
}

pub fn load_raw_from_root_blocking(
) -> Result<Option<alloc::vec::Vec<u8>>, crate::disc::block::Error> {
    let mut last_err: Option<crate::disc::block::Error> = None;

    // Try every mounted TRUEOSFS root (newest first) so a valid pci.ids on an
    // older root still works if the primary root switched later in boot.
    for root in crate::r::fs::trueosfs::list_roots() {
        let Some(disk) = crate::disc::block::device_handle(root.disk_id) else {
            continue;
        };
        match crate::wait::spawn_and_wait_local(async move {
            crate::r::fs::trueosfs::file_out_async(disk, PCI_IDS_KEY).await
        }) {
            Ok(Some(raw)) => return Ok(Some(raw)),
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }
    }

    if let Some(e) = last_err {
        return Err(e);
    }
    Ok(None)
}

pub fn load_sanitized_from_root_blocking(
) -> Result<Option<alloc::vec::Vec<u8>>, crate::disc::block::Error> {
    let Some(raw) = load_raw_from_root_blocking()? else {
        return Ok(None);
    };
    Ok(Some(sanitize_pci_ids(&raw)))
}

fn is_hex(b: u8) -> bool {
    b.is_ascii_hexdigit()
}

fn hex4_lower(bytes: &[u8]) -> Option<[u8; 4]> {
    if bytes.len() != 4 || !bytes.iter().all(|&b| is_hex(b)) {
        return None;
    }
    let mut out = [0u8; 4];
    out.copy_from_slice(bytes);
    for b in &mut out {
        *b = b.to_ascii_lowercase();
    }
    Some(out)
}

fn trim_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_first() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn trim_trailing_ascii_ws(mut s: &[u8]) -> &[u8] {
    while let Some((&b, rest)) = s.split_last() {
        if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
            s = rest;
        } else {
            break;
        }
    }
    s
}

fn collapse_ascii_ws_into(out: &mut alloc::vec::Vec<u8>, s: &[u8]) {
    let s = trim_ascii_ws(s);
    let mut prev_space = false;
    for &b in s {
        let is_ws = b == b' ' || b == b'\t' || b == b'\r' || b == b'\n';
        if is_ws {
            prev_space = true;
            continue;
        }
        if prev_space && !out.is_empty() && *out.last().unwrap() != b' ' {
            out.push(b' ');
        }
        prev_space = false;
        out.push(b);
    }
}

pub fn sanitize_pci_ids(raw: &[u8]) -> alloc::vec::Vec<u8> {
    // Goal: keep only vendor/device/subsystem entries with their indentation.
    // - drop blank lines and comments
    // - normalize indentation to 0/1/2 leading tabs
    // - normalize IDs to lowercase
    // - collapse whitespace in names
    use alloc::vec::Vec;

    let mut out: Vec<u8> = Vec::with_capacity(raw.len().min(4 * 1024 * 1024));

    let mut i: usize = 0;
    while i < raw.len() {
        // Find next line.
        let start = i;
        while i < raw.len() && raw[i] != b'\n' {
            i += 1;
        }
        let mut line = &raw[start..i];
        if i < raw.len() && raw[i] == b'\n' {
            i += 1;
        }
        // Strip a trailing '\r' from CRLF.
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }

        let line = trim_trailing_ascii_ws(line);
        if line.is_empty() {
            continue;
        }

        // Comment-only lines (allow leading whitespace).
        let mut k: usize = 0;
        while k < line.len() && (line[k] == b' ' || line[k] == b'\t') {
            k += 1;
        }
        if k >= line.len() {
            continue;
        }
        if line[k] == b'#' {
            continue;
        }

        // Indent is encoded as leading tabs in pci.ids.
        // Clamp to 0/1/2.
        let mut indent: usize = 0;
        let mut p: usize = 0;
        while p < line.len() && line[p] == b'\t' {
            indent += 1;
            p += 1;
        }
        let indent = indent.min(2);
        let rest = trim_ascii_ws(&line[p..]);
        if rest.is_empty() {
            continue;
        }

        // Skip non vendor/device/subsystem sections (e.g. classes starting with 'C').
        if indent == 0 {
            if rest.len() < 6 {
                continue;
            }
            let Some(id) = hex4_lower(&rest[..4]) else {
                continue;
            };
            // Require whitespace after the vendor ID.
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let name = trim_ascii_ws(&rest[4..]);
            if name.is_empty() {
                continue;
            }
            out.extend_from_slice(&id);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        } else if indent == 1 {
            if rest.len() < 6 {
                continue;
            }
            let Some(id) = hex4_lower(&rest[..4]) else {
                continue;
            };
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let name = trim_ascii_ws(&rest[4..]);
            if name.is_empty() {
                continue;
            }
            out.push(b'\t');
            out.extend_from_slice(&id);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        } else {
            // Subsystem lines: <subvendor> <subdevice> <name>
            // Example: "\t\t0ccd 0000  MN-Core 2 16GB"
            // Accept one or more whitespace separators.
            if rest.len() < 11 {
                continue;
            }
            let Some(subvendor) = hex4_lower(&rest[..4]) else {
                continue;
            };
            if rest[4] != b' ' && rest[4] != b'\t' {
                continue;
            }
            let mut j = 4;
            while j < rest.len() && (rest[j] == b' ' || rest[j] == b'\t') {
                j += 1;
            }
            if j + 4 > rest.len() {
                continue;
            }
            let Some(subdevice) = hex4_lower(&rest[j..j + 4]) else {
                continue;
            };
            j += 4;
            if j >= rest.len() || (rest[j] != b' ' && rest[j] != b'\t') {
                continue;
            }
            let name = trim_ascii_ws(&rest[j..]);
            if name.is_empty() {
                continue;
            }
            out.push(b'\t');
            out.push(b'\t');
            out.extend_from_slice(&subvendor);
            out.push(b' ');
            out.extend_from_slice(&subdevice);
            out.push(b' ');
            collapse_ascii_ws_into(&mut out, name);
            out.push(b'\n');
        }
    }

    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn hex4_to_u16(bytes: &[u8]) -> Option<u16> {
    if bytes.len() != 4 {
        return None;
    }
    let a = hex_nibble(bytes[0])? as u16;
    let b = hex_nibble(bytes[1])? as u16;
    let c = hex_nibble(bytes[2])? as u16;
    let d = hex_nibble(bytes[3])? as u16;
    Some((a << 12) | (b << 8) | (c << 4) | d)
}

/// Lookup a `(vendor_name, device_name)` tuple by vendor+device IDs.
///
/// Works on the sanitized `pci.ids` format produced by `sanitize_pci_ids()`:
/// - vendor lines: `vvvv <name>`
/// - device lines: `\tdddd <name>`
/// - subsystem lines are ignored here.
pub fn lookup_vendor_device_from_db<'a>(
    db: &'a [u8],
    vid: u16,
    did: u16,
) -> Option<(&'a [u8], &'a [u8])> {
    let mut i: usize = 0;
    let mut in_vendor = false;
    let mut seen_vendor = false;
    let mut vendor_name: Option<&'a [u8]> = None;

    while i < db.len() {
        let start = i;
        while i < db.len() && db[i] != b'\n' {
            i += 1;
        }
        let mut line = &db[start..i];
        if i < db.len() && db[i] == b'\n' {
            i += 1;
        }
        if let Some((&b'\r', rest)) = line.split_last() {
            line = rest;
        }
        if line.is_empty() {
            continue;
        }

        // Determine indent (0/1/2 tabs) and the remaining payload.
        let mut p: usize = 0;
        while p < line.len() && line[p] == b'\t' {
            p += 1;
        }
        let indent = core::cmp::min(p, 2);
        let rest = &line[p..];

        if indent == 0 {
            if seen_vendor {
                // We already passed the matching vendor section without finding the device.
                return None;
            }
            if rest.len() < 6 {
                continue;
            }
            let Some(v) = hex4_to_u16(&rest[..4]) else {
                continue;
            };
            if rest[4] != b' ' {
                continue;
            }
            if v == vid {
                in_vendor = true;
                seen_vendor = true;
                vendor_name = Some(&rest[5..]);
            } else {
                in_vendor = false;
                vendor_name = None;
            }
            continue;
        }

        if indent == 1 {
            if !in_vendor {
                continue;
            }
            let vend = vendor_name?;
            if rest.len() < 6 {
                continue;
            }
            let Some(d) = hex4_to_u16(&rest[..4]) else {
                continue;
            };
            if rest[4] != b' ' {
                continue;
            }
            if d == did {
                return Some((vend, &rest[5..]));
            }
            continue;
        }
    }
    None
}
