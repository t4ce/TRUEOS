pub mod html;

use embassy_executor::task;

#[inline]
fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[inline]
fn write_hex_u64_16(out: &mut [u8; 16], v: u64) {
    for i in (0..16).rev() {
        let nibble = ((v >> (i * 4)) & 0xF) as u8;
        let c = match nibble {
            0..=9 => b'0' + nibble,
            _ => b'a' + (nibble - 10),
        };
        out[(15 - i) as usize] = c;
    }
}

#[task]
pub(crate) async fn boot_fetch_to_file_smoke_task() {
    use embassy_time::Timer;

    // Keep these as concrete URLs (not bare specifiers) so this directly validates
    // the CDN fetch-to-file + persistence layer.
    //
    // NOTE: `path` is now provided as a TRUEOS native module, so we don't need to
    // prefetch any external polyfill here (which also avoids DNS flakes during early boot).
    const URLS: [&str; 1] = ["https://esm.sh/left-pad@1.3.0"];

    // Retry: USBMS/FAT may not be ready yet when the executor starts.
    for &url in &URLS {
        let url_bytes = url.as_bytes();

        // Write into the exact same cache filename as the QuickJS loader:
        // /qjs/cdn/<fnv1a64(url)>.mjs
        let hash = fnv1a64(url_bytes);
        let mut path = [0u8; 29];
        path[..9].copy_from_slice(b"/qjs/cdn/");
        let mut hex = [0u8; 16];
        write_hex_u64_16(&mut hex, hash);
        path[9..25].copy_from_slice(&hex);
        path[25..].copy_from_slice(b".mjs");

        let path_str = core::str::from_utf8(&path).unwrap_or("<non-utf8>");

        let mut ok = false;
        for attempt in 1..=60u32 {
            let rc = unsafe {
                crate::surface::io::cabi::trueos_cabi_net_fetch_to_file(
                    url_bytes.as_ptr(),
                    url_bytes.len(),
                    path.as_ptr(),
                    path.len(),
                )
            };

            if rc == 0 {
                crate::log!("fetch-smoke: ok url={} cache={}\n", url, path_str);
                ok = true;
                break;
            }

            crate::log!(
                "fetch-smoke: attempt={} rc={} ({}) url={} cache={}\n",
                attempt,
                rc,
                crate::surface::io::cabi::code_name(rc),
                url,
                path_str
            );
            Timer::after_millis(500).await;
        }

        if !ok {
            crate::log!(
                "fetch-smoke: giving up after retries url={} cache={}\n",
                url,
                path_str
            );
            return;
        }
    }

    crate::log!("qjs-loader-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_module_loader_smoke() };
    crate::log!("qjs-loader-smoke: done\n");
}

#[task]
pub(crate) async fn boot_cheerio_smoke_task() {
    use embassy_time::Timer;

    // Give the network + USBMS/FAT some time to settle.
    Timer::after_millis(1500).await;

    crate::log!("qjs-cheerio-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_cheerio_smoke() };
    crate::log!("qjs-cheerio-smoke: done\n");
}
