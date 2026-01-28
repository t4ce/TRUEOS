use embassy_executor::task;

/// Fetch and cache the `pci.ids` database on the USBMS FAT filesystem.
///
/// The download is skipped if the destination file already exists.
#[task]
pub(crate) async fn boot_cache_pci_ids_task() {
    use embassy_time::Timer;

    // Source: pciutils/pciids
    const URL: &str = "https://raw.githubusercontent.com/pciutils/pciids/master/pci.ids";

    // Kernel-wide "sources" folder for persisted assets.
    const PATH: &str = "/trueos/src/pci/pci.ids";

    let url_bytes = URL.as_bytes();
    let path_bytes = PATH.as_bytes();

    // Retry: USBMS/FAT may not be ready when the executor starts.
    for attempt in 1..=60u32 {
        let rc = unsafe {
            crate::surface::io::cabi::trueos_cabi_net_fetch_to_file(
                url_bytes.as_ptr(),
                url_bytes.len(),
                path_bytes.as_ptr(),
                path_bytes.len(),
            )
        };

        if rc == 0 {
            crate::log!("pciids: cached ok url={} path={}\n", URL, PATH);
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
