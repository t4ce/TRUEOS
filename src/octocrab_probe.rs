//! Octocrab integration probe.
//!
//! This is deliberately separate from `tokio_probe`: enabling it pulls
//! Octocrab's Hyper/Tokio network stack so we can track the real porting
//! boundary without breaking the default kernel build.

pub(crate) fn log_boot_probe() {
    crate::log!(
        "octocrab_probe: wired octocrab 0.49.7; full build currently exercises hyper/tokio net\n"
    );

    let _ = octocrab::OctocrabBuilder::new;
}
