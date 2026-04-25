//! Direct Hyper integration probe.
//!
//! This lets the kernel own a first-class Hyper dependency directly beside
//! Tokio instead of relying on the temporary Octocrab path.

pub(crate) fn log_boot_probe() {
    crate::log!(
        "hyper_probe: wired hyper 1.9 client/http1 surface directly beside tokio\n"
    );

    let _ = hyper::client::conn::http1::Builder::new;
    let _ = hyper::Method::GET;
    let _ = hyper::Version::HTTP_11;
}