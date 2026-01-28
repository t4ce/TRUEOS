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

    let url_bytes = URL.as_bytes();
    let path_bytes = PATH.as_bytes();

    // Retry: USBMS/FAT may not be ready when the executor starts.
    for attempt in 1..=60u32 {
        match crate::surface::io::kfs::exists(PATH) {
            Ok(true) => {
                crate::log!("pciids: cache hit path={}\n", PATH);
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
