use embassy_executor::task;

fn is_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F')
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

fn sanitize_pci_ids(raw: &[u8]) -> alloc::vec::Vec<u8> {
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
            let Some(id) = hex4_lower(&rest[..4]) else { continue };
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
            let Some(id) = hex4_lower(&rest[..4]) else { continue };
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
            let Some(subvendor) = hex4_lower(&rest[..4]) else { continue };
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
            let Some(subdevice) = hex4_lower(&rest[j..j + 4]) else { continue };
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

/// Lookup a vendor name by vendor ID ($vid$) from a sanitized `pci.ids` blob.
///
/// Returns the vendor name bytes (typically UTF-8).
pub fn lookup_vendor_name_from_db<'a>(db: &'a [u8], vid: u16) -> Option<&'a [u8]> {
    let mut i: usize = 0;
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
        if line[0] == b'\t' {
            continue;
        }
        if line.len() < 6 {
            continue;
        }
        let Some(id) = hex4_to_u16(&line[..4]) else {
            continue;
        };
        if line[4] != b' ' {
            continue;
        }
        if id == vid {
            return Some(&line[5..]);
        }
    }
    None
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
            let Some(v) = hex4_to_u16(&rest[..4]) else { continue };
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
            let Some(d) = hex4_to_u16(&rest[..4]) else { continue };
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
/*
/// Convenience: read the cached database and do a vendor+device lookup.
pub fn lookup_vendor_device_cached(
    vid: u16,
    did: u16,
) -> Result<Option<(alloc::string::String, alloc::string::String)>, crate::disc::files::FsError> {
    use alloc::string::String;

    const PATH: &str = "/trueos/pci/pci.ids";
    let db = crate::surface::io::kfs::read_file(PATH)?;

    let Some((v, d)) = lookup_vendor_device_from_db(&db, vid, did) else {
        return Ok(None);
    };

    // Best-effort UTF-8 conversion for logs/UI.
    let v = String::from_utf8_lossy(v).into_owned();
    let d = String::from_utf8_lossy(d).into_owned();
    Ok(Some((v, d)))
}
*/
fn log_pci_enumeration_with_cached_ids(db: &[u8]) {
    // Re-enumerate here so the list reflects the system state after init.
    // (Enumeration is cheap and uses the same static cache the shell relies on.)
    crate::pci::enumerate_silent();
    crate::pci::log_devices_with_pci_ids(db);
}

/// Fetch and cache the `pci.ids` database on the USBMS FAT filesystem.
///
/// The download is skipped if the destination file already exists.
#[task]
pub(crate) async fn boot_cache_pci_ids_task() {
    use embassy_time::Timer;

    // Source: pciutils/pciids
    const URL: &str = "https://raw.githubusercontent.com/pciutils/pciids/master/pci.ids";

    // Persistent cache location on USBMS FAT.
    //
    // Requirement: keep this under the `/trueos/pci/` folder.
    const PATH: &str = "/trueos/pci/pci.ids";

    // Previous cache locations we may have used in older builds.
    const OLD_PATHS: [&str; 3] = [
        "/trueos/src/pci/pci.ids",
        "/trueos/pci.ids",
        "/§/pci.ids",
    ];

    // Retry: USBMS/FAT may not be ready when the executor starts.
    for attempt in 1..=60u32 {
        match crate::surface::io::kfs::exists(PATH) {
            Ok(true) => {
                crate::log!("pciids: cache hit path={}\n", PATH);
                if let Ok(db) = crate::surface::io::kfs::read_file(PATH) {
                    log_pci_enumeration_with_cached_ids(&db);
                }
                return;
            }
            Ok(false) => {}
            Err(_) => {}
        }

        // One-time migration from old locations (avoid redownload after upgrades).
        // We also sanitize during migration so the persistent cache stays normalized.
        for old in OLD_PATHS {
            match crate::surface::io::kfs::exists(old) {
                Ok(true) => {
                    if let Some((parent, _name)) = PATH.rsplit_once('/') {
                        if !parent.is_empty() {
                            let _ = crate::surface::io::kfs::create_dir_all(parent);
                        }
                    }

                    if let Ok(raw) = crate::surface::io::kfs::read_file(old) {
                        let cleaned = sanitize_pci_ids(&raw);
                        let tmp = alloc::format!("{}.tmp", PATH);
                        if crate::surface::io::kfs::write_file(tmp.as_str(), &cleaned).is_ok()
                            && crate::surface::io::kfs::rename(tmp.as_str(), PATH).is_ok()
                        {
                            let _ = crate::surface::io::kfs::remove(old);
                            crate::log!(
                                "pciids: migrated+sanitized old={} new={} bytes_in={} bytes_out={}\n",
                                old,
                                PATH,
                                raw.len(),
                                cleaned.len(),
                            );
                            if let Ok(db) = crate::surface::io::kfs::read_file(PATH) {
                                log_pci_enumeration_with_cached_ids(&db);
                            }
                            return;
                        }
                        let _ = crate::surface::io::kfs::remove(tmp.as_str());
                    }
                }
                Ok(false) => {}
                Err(_) => {}
            }
        }

        // Ensure the cache directory exists before downloading.
        // If USBMS/FAT isn't ready yet, don't waste network bandwidth.
        if let Some((parent, _name)) = PATH.rsplit_once('/') {
            if !parent.is_empty() {
                if let Err(e) = crate::surface::io::kfs::create_dir_all(parent) {
                    crate::log!(
                        "pciids: attempt={} fs_not_ready={:?} url={} path={}\n",
                        attempt,
                        e,
                        URL,
                        PATH
                    );
                    Timer::after_millis(500).await;
                    continue;
                }
            }
        }

        let raw = match crate::surface::io::cabi::net_fetch_https_body_blocking(URL, 30_000, 4 * 1024 * 1024) {
            Ok(b) => b,
            Err(rc) => {
                crate::log!(
                    "pciids: attempt={} rc={} ({}) url={} path={}\n",
                    attempt,
                    rc,
                    crate::surface::io::cabi::code_name(rc),
                    URL,
                    PATH
                );
                Timer::after_millis(500).await;
                continue;
            }
        };

        let cleaned = sanitize_pci_ids(&raw);
        let tmp = alloc::format!("{}.tmp", PATH);
        let write_res = crate::surface::io::kfs::write_file(tmp.as_str(), &cleaned);
        let rename_res = write_res.and_then(|_| crate::surface::io::kfs::rename(tmp.as_str(), PATH));
        if rename_res.is_ok() {
            crate::log!(
                "pciids: downloaded+sanitized ok url={} path={} bytes_in={} bytes_out={}\n",
                URL,
                PATH,
                raw.len(),
                cleaned.len(),
            );
            if let Ok(db) = crate::surface::io::kfs::read_file(PATH) {
                log_pci_enumeration_with_cached_ids(&db);
            }
            return;
        }

        let _ = crate::surface::io::kfs::remove(tmp.as_str());
        crate::log!(
            "pciids: attempt={} write_failed={:?} rename_failed={:?} url={} path={}\n",
            attempt,
            write_res.err(),
            rename_res.err(),
            URL,
            PATH
        );
        Timer::after_millis(500).await;
    }

    crate::log!("pciids: giving up after retries url={} path={}\n", URL, PATH);
}
