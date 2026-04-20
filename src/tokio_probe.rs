//! Stage-1 Tokio probe for TRUEOS.
//!
//! This wires Tokio's single-thread runtime surface (`rt`) so we can probe
//! BSP / VM-hull assumptions incrementally without approaching Tokio's
//! multi-thread scheduler yet.

#[inline]
fn touch_rt_surface() {
    // Touch a concrete runtime symbol so the probe is tied to Tokio's
    // single-thread runtime surface, not just the crate's root.
    let _ = tokio::runtime::Builder::new_current_thread;
}

pub(crate) fn log_boot_probe() {
    touch_rt_surface();
    crate::log!(
        "tokio_probe: wired tokio 1.52.1 with feature rt via zkvm std-ABI shim (single-thread runtime probe)\n"
    );
}
