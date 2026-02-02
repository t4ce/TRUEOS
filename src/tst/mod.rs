pub mod html;
pub mod http_trueosfs;
pub mod smoke_fs;
#[cfg(feature = "tst-challenge")]
pub mod sched_challenge;

use embassy_executor::task;

#[task]
pub(crate) async fn boot_fetch_to_file_smoke_task() {
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
        let hash = {
            let mut h: u64 = 0xcbf29ce484222325;
            for &b in url_bytes {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        };
        let mut path = [0u8; 29];
        path[..9].copy_from_slice(b"/qjs/cdn/");
        let mut hex = [0u8; 16];
        for i in (0..16).rev() {
            let nibble = ((hash >> (i * 4)) & 0xF) as u8;
            let c = match nibble {
                0..=9 => b'0' + nibble,
                _ => b'a' + (nibble - 10),
            };
            hex[(15 - i) as usize] = c;
        }
        path[9..25].copy_from_slice(&hex);
        path[25..].copy_from_slice(b".mjs");

        let path_str = core::str::from_utf8(&path).unwrap_or("<non-utf8>");

        match crate::v::net::https::fetch_https_to_file_async(
            url,
            path_str,
            30_000,
            4 * 1024 * 1024,
        )
        .await
        {
            Ok(()) => {
                crate::log!("fetch-smoke: ok url={} cache={}\n", url, path_str);
            }
            Err(rc) => {
                crate::log!("fetch-smoke: failed rc={} url={} cache={}\n", rc, url, path_str);
                return;
            }
        }
    }

    crate::log!("qjs-loader-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_module_loader_smoke() };
    crate::log!("qjs-loader-smoke: done\n");
}

#[task]
pub(crate) async fn boot_cheerio_smoke_task() {
    crate::log!("qjs-cheerio-smoke: starting\n");
    unsafe { trueos_qjs::trueos_smoke::run_cheerio_smoke() };
    crate::log!("qjs-cheerio-smoke: done\n");
}
