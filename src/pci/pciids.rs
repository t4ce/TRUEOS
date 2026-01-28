use embassy_executor::task;

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
    // Note: this uses a VFAT LFN directory name (U+00A7 SECTION SIGN).
    const PATH: &str = "/§/pci.ids";
    const OLD_PATH: &str = "/trueos/src/pci/pci.ids";

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

        // One-time migration from the old location (avoid redownload after upgrades).
        match crate::surface::io::kfs::exists(OLD_PATH) {
            Ok(true) => {
                if let Some((parent, _name)) = PATH.rsplit_once('/') {
                    if !parent.is_empty() {
                        let _ = crate::surface::io::kfs::create_dir_all(parent);
                    }
                }
                if crate::surface::io::kfs::rename(OLD_PATH, PATH).is_ok() {
                    crate::log!("pciids: migrated cache old={} new={}\n", OLD_PATH, PATH);
                    return;
                }
            }
            Ok(false) => {}
            Err(_) => {}
        }

        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_net_fetch_to_file(
                url_bytes.as_ptr(),
                url_bytes.len(),
                path_bytes.as_ptr(),
                path_bytes.len(),
            )
        };

        if rc == 0 {
            crate::log!("pciids: downloaded ok url={} path={}\n", URL, PATH);
            return;
        }

        crate::log!(
            "pciids: attempt={} rc={} ({}) url={} path={}\n",
            attempt,
            rc,
            crate::surface::io::cabi::code_name(rc),
            URL,
            PATH
        );
        Timer::after_millis(500).await;
    }

    crate::log!("pciids: giving up after retries url={} path={}\n", URL, PATH);
}
