//! Tokio network integration for TRUEOS.
//!
//! This is the home for the contract between Tokio's socket model and TRUEOS
//! VNet readiness, Mio, socket2, Hickory, and Hyper surfaces.

pub fn fetch_https_to_file(
    job: &'static str,
    url: &'static str,
    key: &'static str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    crate::log!("r/t/net: {} tokio https begin url={} key={}\n", job, url, key);
    match crate::r::t::block_on_io(crate::r::net::https::fetch_https_to_file_async(
        url, key, timeout_ms, max_bytes,
    )) {
        Ok(result) => result,
        Err(_) => {
            crate::log!("r/t/net: {} tokio runtime build failed url={}\n", job, url);
            Err(-1)
        }
    }
}
