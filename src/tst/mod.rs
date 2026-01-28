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

    // Keep this a concrete URL (not a bare specifier) so it directly validates
    // the CDN fetch-to-file + persistence layer.
    const URL: &str = "https://esm.sh/left-pad@1.3.0";

    // Write into the exact same cache filename as the QuickJS loader:
    // /qjs/cdn/<fnv1a64(url)>.mjs
    let url_bytes = URL.as_bytes();
    let hash = fnv1a64(url_bytes);
    let mut path = [0u8; 29];
    path[..9].copy_from_slice(b"/qjs/cdn/");
    let mut hex = [0u8; 16];
    write_hex_u64_16(&mut hex, hash);
    path[9..25].copy_from_slice(&hex);
    path[25..].copy_from_slice(b".mjs");

    let path_str = core::str::from_utf8(&path).unwrap_or("<non-utf8>");

    // Retry: USBMS/FAT may not be ready yet when the executor starts.
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
            crate::log!("fetch-smoke: ok url={} cache={}\n", URL, path_str);
            crate::log!("qjs-loader-smoke: starting\n");
            unsafe { trueos_qjs::trueos_smoke::run_module_loader_smoke() };
            crate::log!("qjs-loader-smoke: done\n");
            return;
        }

        crate::log!(
            "fetch-smoke: attempt={} rc={} ({}) url={} cache={}\n",
            attempt,
            rc,
            crate::surface::io::cabi::code_name(rc),
            URL,
            path_str
        );

        Timer::after_millis(500).await;
    }

    crate::log!("fetch-smoke: giving up after retries url={} cache={}\n", URL, path_str);
}
