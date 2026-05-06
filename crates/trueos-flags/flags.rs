#![no_std]
#![allow(non_snake_case)]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

// ISO 3166 Country Codes
const CODES_URL: &str = "https://flagcdn.com/en/codes.json";
const CODES_PATH: &str = "/flags/codes.json";

static CODES_READY: AtomicBool = AtomicBool::new(false);

unsafe extern "C" {
    fn trueos_cabi_fs_read_file(
        path_ptr: *const u8,
        path_len: usize,
        out_ptr: *mut u8,
        out_cap: usize,
    ) -> isize;
    fn trueos_cabi_net_fetch_start(
        url_ptr: *const u8,
        url_len: usize,
        path_ptr: *const u8,
        path_len: usize,
    ) -> u32;
    fn trueos_cabi_net_fetch_wait(op_id: u32, timeout_ms: u64) -> i32;
    fn trueos_cabi_net_fetch_result(op_id: u32) -> i32;
    fn trueos_cabi_net_fetch_discard(op_id: u32) -> i32;
}

fn read_file(path: &str) -> Result<Vec<u8>, i32> {
    let len = unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), core::ptr::null_mut(), 0) };
    if len < 0 {
        return Err(len as i32);
    }
    let mut out = Vec::with_capacity(len as usize);
    out.resize(len as usize, 0);
    let got = unsafe { trueos_cabi_fs_read_file(path.as_ptr(), path.len(), out.as_mut_ptr(), out.len()) };
    if got < 0 {
        return Err(got as i32);
    }
    out.truncate(got as usize);
    Ok(out)
}

fn fetch_to_file_once(url: &str, path: &str) -> Result<(), i32> {
    let op = unsafe { trueos_cabi_net_fetch_start(url.as_ptr(), url.len(), path.as_ptr(), path.len()) };
    if op == 0 {
        return Err(-1);
    }

    let wait_rc = unsafe { trueos_cabi_net_fetch_wait(op, 30_000) };
    if wait_rc != 0 {
        let _ = unsafe { trueos_cabi_net_fetch_discard(op) };
        return Err(wait_rc);
    }

    let result_rc = unsafe { trueos_cabi_net_fetch_result(op) };
    let _ = unsafe { trueos_cabi_net_fetch_discard(op) };
    if result_rc != 0 {
        return Err(result_rc);
    }

    Ok(())
}

fn ensure_codes_ready() -> Result<String, i32> {
    if let Ok(bytes) = read_file(CODES_PATH) {
        let s = String::from_utf8_lossy(bytes.as_slice()).into_owned();
        CODES_READY.store(true, Ordering::Release);
        return Ok(s);
    }

    fetch_to_file_once(CODES_URL, CODES_PATH)?;
    let bytes = read_file(CODES_PATH)?;
    let s = String::from_utf8_lossy(bytes.as_slice()).into_owned();
    CODES_READY.store(true, Ordering::Release);
    Ok(s)
}

fn normalize_country_code(countrycode: &str) -> Option<String> {
    let trimmed = countrycode.trim();
    if trimmed.len() != 2 {
        return None;
    }
    if !trimmed.as_bytes().iter().all(u8::is_ascii_alphabetic) {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn codes_contains(codes_json: &str, code: &str) -> bool {
    let needle = format!("\"{}\"", code);
    codes_json.contains(needle.as_str())
}

/// Returns the SVG text for a 2-letter country code (for example: "ua").
///
/// This uses TRUEOS async net-fetch ops under the hood and waits for completion once,
/// then returns only the SVG string content.
pub fn getFlagSVG(countrycode: &str) -> String {
    let Some(code) = normalize_country_code(countrycode) else {
        return String::new();
    };

    let codes_json = if CODES_READY.load(Ordering::Acquire) {
        match read_file(CODES_PATH) {
            Ok(bytes) => String::from_utf8_lossy(bytes.as_slice()).into_owned(),
            Err(_) => String::new(),
        }
    } else {
        match ensure_codes_ready() {
            Ok(s) => s,
            Err(_) => return String::new(),
        }
    };

    if !codes_contains(codes_json.as_str(), code.as_str()) {
        return String::new();
    }

    let svg_path = format!("/flags/{}.svg", code);
    if read_file(svg_path.as_str()).is_err() {
        let url = format!("https://flagcdn.com/{}.svg", code);
        if fetch_to_file_once(url.as_str(), svg_path.as_str()).is_err() {
            return String::new();
        }
    }

    match read_file(svg_path.as_str()) {
        Ok(bytes) => String::from_utf8_lossy(bytes.as_slice()).into_owned(),
        Err(_) => String::new(),
    }
}
