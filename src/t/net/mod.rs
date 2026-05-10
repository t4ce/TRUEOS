//! Tokio network integration for TRUEOS.
//!
//! This is the home for the contract between Tokio's socket model and TRUEOS
//! VNet readiness, Mio, socket2, Hickory, and Hyper surfaces.

extern crate alloc;

pub mod dns;
pub mod http;
pub mod http_stream;
pub mod https;
pub mod hyper_io;
pub mod ping;
pub mod vnet_stream;

use alloc::string::String;
use heapless::String as HString;

pub fn fetch_https_to_file_hyper(
    job: &'static str,
    url: &'static str,
    key: &'static str,
    timeout_ms: u32,
    max_bytes: usize,
) -> Result<(), i32> {
    crate::log!("t/net: {} hyper https begin url={} key={}\n", job, url, key);
    match crate::t::block_on_io(crate::t::net::https::fetch_https_to_file_hyper_async(
        url, key, timeout_ms, max_bytes,
    )) {
        Ok(result) => result,
        Err(_) => {
            crate::log!("t/net: {} hyper runtime build failed url={}\n", job, url);
            Err(-1)
        }
    }
}

pub async fn fetch_html_best_effort_shared(
    job: &'static str,
    url: HString<256>,
) -> Result<String, &'static str> {
    crate::log!("t/net: {} shared-tokio html begin url={}\n", job, url.as_str());
    match crate::t::run_on_shared_tokio(move || crate::r::net::html::fetch_html_best_effort(url))
        .await
    {
        Ok(result) => result,
        Err(_) => {
            crate::log!("t/net: {} shared tokio runtime unavailable\n", job);
            Err("shared tokio runtime unavailable")
        }
    }
}
